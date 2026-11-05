//! 投影工坊子进程（--schem-worker）：把原理图解析与编辑会话隔离到独立进程，
//! 数百 MB 级的解析峰值与常驻会话不再压在主进程；工坊关闭 / 缩略图用完即退，
//! 静默状态下该子进程不存在。
//!
//! 协议（stdio JSON Lines，一行一条）：
//!   请求  {"i":<id>,"c":"<channel>","a":<args>}
//!   响应  {"i":<id>,"ok":true,"v":<value>} | {"i":<id>,"ok":false,"e":"<msg>"}
//!
//! 生命周期：stdin EOF（主进程退出）或「无在途请求且无存活会话」时自动退出；
//! 主进程侧发现管道断裂会把死 worker 丢弃，下一次调用自动重启
//! （崩溃会丢会话，由 UI 报错引导重新打开文件——与进程内崩溃同一语义）。

use serde_json::Value;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::{Arc, Condvar, Mutex, OnceLock};

// ---------------- 主进程侧：懒启动 + 多路复用代理 ----------------

static WORKER: OnceLock<Mutex<Option<Worker>>> = OnceLock::new();

fn worker_slot() -> &'static Mutex<Option<Worker>> {
    WORKER.get_or_init(|| Mutex::new(None))
}

struct Worker {
    child: Child,
    stdin: ChildStdin,
    next_id: AtomicU64,
}

impl Drop for Worker {
    fn drop(&mut self) {
        // 正常路径是 worker 自退出；这里兜底替换/丢弃场景
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// 转发一条会话命令到子进程并等待响应。请求按 id 多路复用，
/// 长 open（几秒）不会阻塞并发的 readChunk（与进程内语义一致）。
pub fn call(channel: &str, args: Value) -> Result<Value, String> {
    let mut slot = worker_slot().lock().map_err(|_| "worker 锁已损坏")?;
    if slot.is_none() {
        *slot = Some(spawn()?);
    }
    let worker = slot.as_mut().unwrap();
    let id = worker.next_id.fetch_add(1, Ordering::SeqCst) + 1;
    let (tx, rx) = sync_channel::<Result<Value, String>>(1);
    PENDING
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap()
        .insert(id, tx);
    let request = serde_json::json!({ "i": id, "c": channel, "a": args });
    let mut line = serde_json::to_string(&request).map_err(|e| e.to_string())?;
    line.push('\n');
    if worker.stdin.write_all(line.as_bytes()).is_err() || worker.stdin.flush().is_err() {
        // 管道断裂：子进程已死，丢弃；下一次调用自动重启
        *slot = None;
        return Err("投影工坊服务已重启，请重新打开文件".into());
    }
    drop(slot); // 释放槽锁，并发命令可以继续写入
    match rx.recv() {
        Ok(result) => result,
        Err(_) => {
            // 响应通道断开 = reader 线程退出 = 子进程死亡
            if let Ok(mut slot) = worker_slot().lock() {
                *slot = None;
            }
            Err("投影工坊服务已重启，请重新打开文件".into())
        }
    }
}

static PENDING: OnceLock<Mutex<HashMap<u64, SyncSender<Result<Value, String>>>>> = OnceLock::new();

fn spawn() -> Result<Worker, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut command = Command::new(exe);
    command
        .arg("--schem-worker")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("无法启动投影工坊子进程：{e}"))?;
    let stdin = child.stdin.take().ok_or("无法接通子进程 stdin")?;
    let stdout = child.stdout.take().ok_or("无法接通子进程 stdout")?;
    let pending = PENDING.get_or_init(|| Mutex::new(HashMap::new()));
    std::thread::spawn(move || {
        let pending = pending;
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            let Ok(value) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            let Some(id) = value.get("i").and_then(|v| v.as_u64()) else {
                continue;
            };
            let response = if value.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
                Ok(value.get("v").cloned().unwrap_or(Value::Null))
            } else {
                Err(value
                    .get("e")
                    .and_then(|v| v.as_str())
                    .unwrap_or("投影工坊子进程返回未知错误")
                    .to_string())
            };
            if let Some(sender) = pending.lock().unwrap().remove(&id) {
                let _ = sender.send(response);
            }
        }
        // reader 结束 = 子进程退出：清空等待者（SyncSender 被 drop → recv 返回 Err）
        pending.lock().unwrap().clear();
    });
    Ok(Worker {
        child,
        stdin,
        next_id: AtomicU64::new(0),
    })
}

// ---------------- 子进程侧：stdio 服务循环 ----------------

/// 子进程入口（main.rs 检测 --schem-worker 后调用；不返回）。
/// 每条请求起独立线程处理：长解析期间 cancel 命令仍可被处理（与进程内语义一致）。
pub fn run() -> ! {
    let stdout: Arc<Mutex<std::io::Stdout>> = Arc::new(Mutex::new(std::io::stdout()));
    let in_flight = Arc::new((Mutex::new(0usize), Condvar::new()));
    let mut line = String::new();
    {
        let stdin = std::io::stdin();
        let mut reader = BufReader::new(stdin.lock());
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => break, // stdin EOF：主进程已退出
                Ok(_) => {}
            }
            let Ok(request) = serde_json::from_str::<Value>(line.trim()) else {
                continue;
            };
            let Some(id) = request.get("i").and_then(|v| v.as_u64()) else {
                continue;
            };
            let Some(channel) = request.get("c").and_then(|v| v.as_str()) else {
                continue;
            };
            let args = request.get("a").cloned().unwrap_or(Value::Null);
            let channel = channel.to_string();
            {
                let (count, signal) = &*in_flight;
                let mut guard = count.lock().unwrap();
                *guard += 1;
                signal.notify_all();
            }
            let stdout = stdout.clone();
            let in_flight = in_flight.clone();
            std::thread::spawn(move || {
                // catch_unwind：请求线程崩掉也要回一条错误响应，否则调用方在
                // unwind profile（dev）下会永久挂起；release 的 panic=abort 直接退进程也能兜住
                let result = std::panic::catch_unwind(|| dispatch(&channel, &args))
                    .unwrap_or_else(|_| Err("投影工坊处理请求时崩溃".into()));
                let response = match result {
                    Ok(value) => serde_json::json!({ "i": id, "ok": true, "v": value }),
                    Err(error) => serde_json::json!({ "i": id, "ok": false, "e": error }),
                };
                if let Ok(mut body) = serde_json::to_string(&response) {
                    body.push('\n');
                    if let Ok(mut out) = stdout.lock() {
                        let _ = out.write_all(body.as_bytes());
                        let _ = out.flush();
                    }
                }
                release_in_flight(&in_flight);
            });
        }
    }
    // 主进程退出：等在途请求收尾后结束（release_in_flight 会在空闲时自行 exit）
    let (count, signal) = &*in_flight;
    let mut guard = count.lock().unwrap();
    while *guard > 0 {
        guard = signal.wait(guard).unwrap();
    }
    std::process::exit(0);
}

fn release_in_flight(in_flight: &Arc<(Mutex<usize>, Condvar)>) {
    let (count, signal) = &**in_flight;
    let remaining = {
        let mut guard = count.lock().unwrap();
        *guard -= 1;
        let remaining = *guard;
        if remaining == 0 {
            signal.notify_all();
        }
        remaining
    };
    // 用完即卸载：无在途请求且无存活会话/取消令牌时退出进程
    if remaining == 0
        && crate::schematic_preview::sessions_idle()
        && crate::schematic_preview::cancellations_idle()
    {
        std::process::exit(0);
    }
}

fn dispatch(channel: &str, args: &Value) -> Result<Value, String> {
    use base64::Engine as _;
    use crate::schematic_preview as sp;
    let b64 =
        |bytes: Vec<u8>| Value::String(base64::engine::general_purpose::STANDARD.encode(bytes));
    match channel {
        "schempreview:open" => {
            let parsed: sp::OpenQuery = serde_json::from_value(args.clone()).map_err(|e| e.to_string())?;
            match parsed.source {
                sp::SchematicSource::External { path } => {
                    sp::open_external(&path, &parsed.request_id)
                }
                _ => Err("投影工坊子进程只接受外部文件源".into()),
            }
        }
        "schempreview:readChunk" => {
            let parsed: sp::ChunkQuery = serde_json::from_value(args.clone()).map_err(|e| e.to_string())?;
            sp::read_chunk(&parsed.session_id, &parsed.region_id, parsed.position).map(b64)
        }
        "schempreview:blockInfo" => {
            let parsed: sp::ChunkQuery = serde_json::from_value(args.clone()).map_err(|e| e.to_string())?;
            sp::block_info(&parsed.session_id, &parsed.region_id, parsed.position)
                .and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string()))
        }
        "schempreview:applyEdits" => {
            let parsed: sp::EditQuery = serde_json::from_value(args.clone()).map_err(|e| e.to_string())?;
            sp::apply_edits(&parsed.session_id, parsed.edits, parsed.target_state)
                .and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string()))
        }
        "schempreview:transform" => {
            let parsed: sp::TransformQuery =
                serde_json::from_value(args.clone()).map_err(|e| e.to_string())?;
            sp::transform(&parsed.session_id, parsed.transform)
                .and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string()))
        }
        "schempreview:exportSponge" => {
            let session_id = args
                .get("sessionId")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            sp::export_sponge_cmd(session_id).map(b64)
        }
        "schempreview:exportLitematic" => {
            let session_id = args
                .get("sessionId")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            sp::export_litematic_cmd(session_id).map(b64)
        }
        "schempreview:cancel" => {
            let request_id = args
                .get("requestId")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            sp::cancel(request_id).map(|_| Value::Null)
        }
        "schempreview:close" => {
            let session_id = args
                .get("sessionId")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            sp::close(session_id).map(|_| Value::Null)
        }
        other => Err(format!("投影工坊子进程未实现通道：{other}")),
    }
}
