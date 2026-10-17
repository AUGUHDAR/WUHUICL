//! 常驻联机进程：一条会话一个进程，NDJSON 事件转成界面事件。

use serde_json::{json, Value};
use std::io::BufRead;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use tauri::{Emitter, Manager};

static CHILD: Mutex<Option<Child>> = Mutex::new(None);
static ALIVE: AtomicBool = AtomicBool::new(false);
static SNAPSHOT: Mutex<Option<Value>> = Mutex::new(None);
static APP: OnceLock<tauri::AppHandle> = OnceLock::new();

pub fn bind(app: &tauri::AppHandle) {
    let _ = APP.set(app.clone());
}

pub fn alive() -> bool {
    ALIVE.load(Ordering::SeqCst)
}

fn idle_snapshot() -> Value {
    json!({ "state": "idle", "room": null, "connection": null, "startedAt": 0,
            "lastError": null, "logs": [] })
}

pub fn snapshot() -> Value {
    SNAPSHOT
        .lock()
        .unwrap()
        .clone()
        .unwrap_or_else(idle_snapshot)
}

fn set_snapshot(v: Value) {
    *SNAPSHOT.lock().unwrap() = Some(v);
}

fn emit(payload: Value) {
    let Some(app) = APP.get() else { return };
    let Some(win) = app.get_webview_window("main") else {
        return;
    };
    let _ = win.emit_to("main", "vlcli:event", payload);
}

/// 纯函数：把一行 NDJSON 折进会话态，返回要推给界面的事件。
pub fn fold_line(snapshot: &Value, line: &str) -> (Value, Option<Value>) {
    let trimmed = line.trim();
    if trimmed.is_empty() || !trimmed.starts_with('{') {
        return (snapshot.clone(), None);
    }
    let Ok(parsed) = serde_json::from_str::<Value>(trimmed) else {
        return (snapshot.clone(), None);
    };
    if let Some(ok) = parsed.get("ok").and_then(|v| v.as_bool()) {
        if ok {
            let data = parsed.get("data").cloned().unwrap_or(Value::Null);
            return (
                snapshot.clone(),
                Some(json!({ "kind": "result", "data": data })),
            );
        }
        let mut next = snapshot.clone();
        next["lastError"] = parsed["message"].clone();
        return (
            next,
            Some(json!({ "kind": "error", "data": {
            "code": parsed["code"].clone(), "message": parsed["message"].clone() } })),
        );
    }
    match parsed["event"].as_str().unwrap_or("") {
        "session:state" => {
            let mut next = snapshot.clone();
            next["state"] = parsed["state"].clone();
            if parsed.get("room").is_some() {
                next["room"] = parsed["room"].clone();
            }
            (next, Some(json!({ "kind": "session", "data": parsed })))
        }
        "conn:state" => {
            let mut next = snapshot.clone();
            next["connection"] = json!({ "phase": parsed["phase"], "status": parsed["status"],
                "address": parsed["address"], "detail": parsed["detail"] });
            (next, Some(json!({ "kind": "conn", "data": parsed })))
        }
        "net:log" => {
            let level = parsed["level"].as_str().unwrap_or("info");
            let msg = parsed["msg"].as_str().unwrap_or("");
            let entry = json!({ "ts": parsed["ts"], "level": level, "text": format!("[{}] {}", level, msg) });
            let mut next = snapshot.clone();
            let mut logs = next["logs"].as_array().cloned().unwrap_or_default();
            logs.push(entry);
            if logs.len() > 200 {
                logs.drain(0..logs.len() - 200);
            }
            next["logs"] = Value::Array(logs);
            (next, Some(json!({ "kind": "log", "data": parsed })))
        }
        "signal" => (
            snapshot.clone(),
            Some(json!({ "kind": "signal", "data": parsed })),
        ),
        _ => (snapshot.clone(), None),
    }
}

pub fn start(bin: &Path, command: &str, payload: &str) -> Result<Value, String> {
    if alive() {
        return Err("已有一个联机任务在跑，请先结束它".into());
    }
    if !bin.is_file() {
        return Err("联机组件尚未就绪".into());
    }
    let mut cmd = Command::new(bin);
    let mut argv: Vec<String> = command.split_whitespace().map(str::to_string).collect();
    argv.extend([
        "--json".into(),
        payload.into(),
        "--events".into(),
        "ndjson".into(),
        "--timeout".into(),
        "0".into(),
    ]);
    cmd.args(&argv)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("联机组件启动失败：{}", e))?;
    let stdout = child.stdout.take().ok_or("联机组件输出不可读")?;
    set_snapshot(
        json!({ "state": "starting", "room": null, "connection": null,
        "startedAt": crate::download::now_ms(), "lastError": null, "logs": [] }),
    );
    ALIVE.store(true, Ordering::SeqCst);
    *CHILD.lock().unwrap() = Some(child);

    std::thread::spawn(move || {
        for line in std::io::BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            let (next, event) = {
                let mut guard = SNAPSHOT.lock().unwrap();
                let current = guard.clone().unwrap_or_else(idle_snapshot);
                let folded = fold_line(&current, &line);
                *guard = Some(folded.0.clone());
                folded
            };
            let kind = event
                .as_ref()
                .and_then(|e| e["kind"].as_str())
                .unwrap_or("")
                .to_string();
            if let Some(event) = event {
                emit(event);
            }
            let _ = next;
            if kind == "error" {
                break;
            }
        }
        ALIVE.store(false, Ordering::SeqCst);
        let mut guard = SNAPSHOT.lock().unwrap();
        let mut cleared = guard.clone().unwrap_or_else(idle_snapshot);
        cleared["state"] = Value::String("idle".into());
        cleared["connection"] = Value::Null;
        *guard = Some(cleared);
        drop(guard);
        emit(json!({ "kind": "closed" }));
    });
    Ok(json!({ "ok": true }))
}

/// 结束会话。只能强杀，房间靠服务端心跳过期。
pub fn stop() -> Value {
    if let Some(mut child) = CHILD.lock().unwrap().take() {
        let _ = child.kill();
        let _ = child.wait();
    }
    ALIVE.store(false, Ordering::SeqCst);
    set_snapshot(idle_snapshot());
    json!({ "ok": true, "roomMayLinger": true })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Value {
        idle_snapshot()
    }

    #[test]
    fn ignores_blank_and_noise() {
        let (snap, event) = fold_line(&base(), "");
        assert!(event.is_none());
        assert_eq!(snap["state"], "idle");
        let (_, event) = fold_line(&base(), "not json");
        assert!(event.is_none());
    }

    #[test]
    fn result_line_becomes_result_event() {
        let (snap, event) = fold_line(&base(), r#"{"ok":true,"data":{"code":"AB12CD"}}"#);
        assert_eq!(event.unwrap()["kind"], "result");
        assert_eq!(snap["state"], "idle");
    }

    #[test]
    fn failure_records_last_error() {
        let (snap, event) = fold_line(
            &base(),
            r#"{"ok":false,"code":"ROOM_NOT_FOUND","message":"房号不存在"}"#,
        );
        assert_eq!(event.unwrap()["kind"], "error");
        assert_eq!(snap["lastError"], "房号不存在");
    }

    #[test]
    fn conn_state_exposes_local_address() {
        let (snap, event) = fold_line(
            &base(),
            r#"{"event":"conn:state","phase":"p2p","status":"success","address":"127.0.0.1:51234"}"#,
        );
        assert_eq!(event.unwrap()["kind"], "conn");
        assert_eq!(snap["connection"]["address"], "127.0.0.1:51234");
        assert_eq!(snap["connection"]["status"], "success");
    }

    #[test]
    fn session_state_updates_room() {
        let (snap, _) = fold_line(
            &base(),
            r#"{"event":"session:state","state":"hosting","room":{"code":"123456"}}"#,
        );
        assert_eq!(snap["state"], "hosting");
        assert_eq!(snap["room"]["code"], "123456");
    }

    #[test]
    fn log_ring_caps_at_200() {
        let mut snap = base();
        for i in 0..260 {
            let line = format!(
                r#"{{"event":"net:log","level":"info","msg":"m{}","ts":{}}}"#,
                i, i
            );
            snap = fold_line(&snap, &line).0;
        }
        assert_eq!(snap["logs"].as_array().unwrap().len(), 200);
        assert_eq!(snap["logs"].as_array().unwrap()[199]["text"], "[info] m259");
    }

    #[test]
    fn unknown_event_is_dropped() {
        let (_, event) = fold_line(&base(), r#"{"event":"mystery","x":1}"#);
        assert!(event.is_none());
    }
}
