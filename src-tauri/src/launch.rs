//! 游戏启动引擎（对应原版 core/launch.ts、gameSession.ts、gracefulClose.ts、
//! launchUiState.ts、commandWorld.ts、gameWindow.ts、gameWindowSize.ts、gameExit.ts）。
//!
//! 通道：game:launch / game:kill / game:restart / game:restartCancel
//!
//! 进程创建方式（对应 gracefulClose.ts 的 koffi CreateProcessW 方案）：
//! 原版因 Electron 会把 libuv 子进程放进 Job Object，退出时树杀，被迫用
//! CreateProcessW 直调逃逸。Tauri/Rust 的 std::process 不创建 Job Object，
//! 子进程天然不随父进程退出而终止；因此用 std::process::Command +
//! CREATE_NO_WINDOW（无控制台闪现）+ 匿名管道回传 stdout/stderr，等价于原版
//! 「脱离式创建、启动器退出游戏继续运行、日志管道回流」的全部语义。
//! 句柄继承（bInheritHandles）语义由 OS 管道继承自动完成：只有写端进入子进程。

use crate::accounts::uuid_v4;
use crate::crashanalyzer;
use crate::installer;
use crate::settings;
use crate::tasks;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// 启动准备无进展超时（launch.ts ProgressDeadline 60000）
const DEADLINE_MS: u64 = 60_000;
/// 正常关窗等待（gameSession.stopGracefully 30000）
const GRACEFUL_STOP_MS: u64 = 30_000;
/// 强杀后等待退出确认（gameSession.stop 8000）
const FORCE_STOP_MS: u64 = 8_000;
const GAME_WIDTH_MIN: i64 = 854;
const GAME_WIDTH_MAX: i64 = 7680;
const GAME_HEIGHT_MIN: i64 = 480;
const GAME_HEIGHT_MAX: i64 = 4320;

// ---------------- 会话（对应 gameSession.ts GameSession，多开支持） ----------------

pub struct ChildProc {
    pub pid: u32,
    pub child: Mutex<std::process::Child>,
    pub exit_code: Mutex<Option<i32>>,
}

pub struct SessionEntry {
    pub token: String,
    pub version_id: String,
    pub folder: String,
    pub launch_id: String,
    pub server_address: Option<String>,
    pub stopping: AtomicBool,
    pub stop_approval: Mutex<Option<String>>,
    pub exited: AtomicBool,
    pub intentional_stop: AtomicBool,
    pub child: Mutex<Option<Arc<ChildProc>>>,
}

#[derive(Clone)]
struct Invocation {
    version_id: String,
    folder: String,
    launch_id: String,
    server_address: Option<String>,
    create_command_world: bool,
}

struct RestartPending {
    invocation: Invocation,
    force_token: Option<String>,
    waiting: bool,
}

struct Globals {
    sessions: Mutex<Vec<Arc<SessionEntry>>>,
    restart_pending: Mutex<Option<RestartPending>>,
    /// launchUiState：launchId → 最近 launching/running 状态（exited/error 移除）
    active_states: Mutex<Vec<Value>>,
    last_launch: Mutex<Option<Value>>,
}

static G: Mutex<Option<Globals>> = Mutex::new(None);

fn with_globals<T>(f: impl FnOnce(&mut Globals) -> T) -> T {
    let mut guard = G.lock().unwrap();
    let gg = guard.get_or_insert_with(|| Globals {
        sessions: Mutex::new(Vec::new()),
        restart_pending: Mutex::new(None),
        active_states: Mutex::new(Vec::new()),
        last_launch: Mutex::new(None),
    });
    f(gg)
}

/// 重启中的 launchId（退出状态标记 intentionalRestart 用）
static RESTARTING: Mutex<Option<String>> = Mutex::new(None);

// ---------------- 对外查询（供其他模块 / boot 路由复用） ----------------

/// 运行中的版本 id 集合（versions:rename 等占用校验用，对应 getRunningVersionIds）
pub fn running_version_ids() -> Vec<String> {
    with_globals(|gg| {
        gg.sessions
            .lock()
            .unwrap()
            .iter()
            .filter(|s| !s.exited.load(Ordering::SeqCst))
            .map(|s| s.version_id.clone())
            .collect()
    })
}

pub fn is_version_running(id: &str) -> bool {
    running_version_ids().iter().any(|v| v == id)
}

/// 运行中的游戏进程 pid（内存整理要跳过正在玩的游戏，对应原版 getRunningGamePids）
pub fn running_game_pids() -> Vec<u32> {
    with_globals(|gg| {
        gg.sessions
            .lock()
            .unwrap()
            .iter()
            .filter(|s| !s.exited.load(Ordering::SeqCst))
            .filter_map(|s| {
                s.child
                    .lock()
                    .unwrap()
                    .as_ref()
                    .map(|proc| proc.pid)
            })
            .collect()
    })
}

/// 最近一次启动上下文（exportLogs 用，对应 getLastLaunch）
pub fn last_launch() -> Option<Value> {
    with_globals(|gg| gg.last_launch.lock().unwrap().clone())
}

/// launchUiState.activeLaunchStates（boot:renderer-ready 重放用）
pub fn active_launch_states() -> Vec<Value> {
    with_globals(|gg| gg.active_states.lock().unwrap().clone())
}

/// launch.ts restoreRunningGame（boot:renderer-ready 路由接入点）
pub fn restore_running_game(app: &AppHandle) -> Option<Value> {
    let file = settings::user_data_dir(app).join("running-game.json");
    let raw = std::fs::read(file).ok()?;
    let record: Value = serde_json::from_slice(&raw).ok()?;
    let pid = record["pid"].as_u64()?;
    let version_id = record["versionId"].as_str()?.to_string();
    if pid == 0 || !process_alive(pid as u32) {
        let _ = std::fs::remove_file(settings::user_data_dir(app).join("running-game.json"));
        return None;
    }
    with_globals(|gg| {
        *gg.last_launch.lock().unwrap() = Some(json!({
            "versionId": version_id,
            "javaPath": "",
            "startedAt": record["startedAt"],
            "effectiveGameDir": record["effectiveGameDir"],
            "logDir": record["logDir"],
            "pid": pid
        }));
    });
    send_state(
        app,
        json!({
            "status": "running",
            "text": "检测到正在运行的游戏（启动器重启后恢复）",
            "versionId": version_id
        }),
    );
    Some(record)
}

fn process_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    const STILL_ACTIVE: u32 = 259;
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return false;
        }
        let mut code: u32 = 0;
        let ok = GetExitCodeProcess(handle, &mut code) != 0;
        CloseHandle(handle);
        // STILL_ACTIVE(259) 表示进程仍在运行（对应原版 gracefulClose.ts:312 的同一判定）
        ok && code == STILL_ACTIVE
    }
}

// ---------------- 事件推送 ----------------

/// ipc.ts sendState：rememberLaunchState 持久化 + event:launchState
fn send_state(app: &AppHandle, state: Value) {
    with_globals(|gg| {
        let status = state["status"].as_str().unwrap_or("").to_string();
        let key = state["launchId"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| {
                format!(
                    "{}:{}",
                    state["folder"].as_str().unwrap_or(""),
                    state["versionId"].as_str().unwrap_or("")
                )
            });
        let mut map = gg.active_states.lock().unwrap();
        map.retain(|s| {
            let k = s["launchId"]
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| {
                    format!(
                        "{}:{}",
                        s["folder"].as_str().unwrap_or(""),
                        s["versionId"].as_str().unwrap_or("")
                    )
                });
            k != key
        });
        if status == "running" || status == "launching" {
            map.push(state.clone());
        }
    });
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.emit_to("main", "event:launchState", state);
    }
}

fn emit_progress(app: &AppHandle, event: Value) {
    tasks::emit_progress(app, event);
}

// ---------------- invoke 入口 ----------------

/// 返回 None 表示本模块不认识该通道（由路由继续分发）；Some(result) 即最终结果。
pub fn invoke(app: &AppHandle, channel: &str, args: &[Value]) -> Option<Result<Value, String>> {
    match channel {
        "game:launch" => Some(game_launch(app, args)),
        "game:kill" => Some(kill_game(args)),
        "game:restart" => Some(restart_game(app, args)),
        "game:restartCancel" => Some(cancel_restart()),
        _ => None,
    }
}

fn arg_str(args: &[Value], i: usize) -> Option<String> {
    args.get(i)
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .filter(|s| !s.is_empty())
}

/// game:launch（ipc.ts gameLaunch）：校验 + launching 状态 + 后台管线；立即返回
fn game_launch(app: &AppHandle, args: &[Value]) -> Result<Value, String> {
    let version_id = arg_str(args, 0)
        .filter(|s| !s.trim().is_empty())
        .ok_or("请选择有效的游戏实例")?;
    let server_address = args
        .get(1)
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let requested_folder = arg_str(args, 2);
    let create_command_world = args.get(3).and_then(|v| v.as_bool()).unwrap_or(false);

    let s = settings::get(app);
    let folder = requested_folder
        .or_else(|| s["activeFolder"].as_str().map(str::to_string).filter(|f| !f.is_empty()))
        .or_else(|| s["gameDir"].as_str().map(str::to_string))
        .ok_or("目标游戏文件夹未注册")?;
    let registered = s["folders"]
        .as_array()
        .map(|a| {
            a.iter()
                .any(|f| f["path"].as_str().map(|p| installer::same_path(p, &folder)).unwrap_or(false))
        })
        .unwrap_or(false);
    if !registered {
        return Err("目标游戏文件夹未注册".into());
    }
    let installed = crate::versions::scan_folder_public(&folder, true);
    let exists = installed.iter().any(|v| {
        v["id"].as_str() == Some(version_id.as_str()) && v["failed"] != true && v["incomplete"] != true
    });
    if !exists {
        return Err("目标实例不存在或不完整，请刷新版本列表".into());
    }

    let launch_id = uuid_v4();
    let invocation = Invocation {
        version_id: version_id.clone(),
        folder: folder.clone(),
        launch_id: launch_id.clone(),
        server_address: server_address.clone(),
        create_command_world,
    };
    send_state(
        app,
        json!({
            "status": "launching",
            "text": "正在准备启动…",
            "versionId": version_id,
            "folder": folder,
            "launchId": launch_id
        }),
    );
    // 记录最近一次启动调用上下文（重启复用；对应 launch.ts invocation 全局）
    *LAST_INVOCATION.lock().unwrap() = Some(invocation);

    let app2 = app.clone();
    let inv = LAST_INVOCATION.lock().unwrap().clone().unwrap();
    // 对应原版 launch.ts:654-663：开启「启动前整理内存」时先整理，再启动
    let organize_before = s["memoryOrganizeBeforeLaunch"].as_bool() == Some(true);
    std::thread::spawn(move || {
        if organize_before {
            send_state(
                &app2,
                json!({
                    "status": "launching",
                    "text": "正在整理内存…",
                    "versionId": inv.version_id,
                    "folder": inv.folder,
                    "launchId": inv.launch_id
                }),
            );
            if let Err(e) = crate::misc::run_memory_organize() {
                // 整理失败不阻断启动，只记日志
                eprintln!("启动前内存整理失败：{e}");
            }
        }
        if let Err(e) = launch_pipeline(&app2, &inv) {
            record_launch_preparation_error(&inv.version_id, &e);
            send_state(
                &app2,
                json!({
                    "status": "error",
                    "text": e,
                    "versionId": inv.version_id,
                    "folder": inv.folder,
                    "launchId": inv.launch_id
                }),
            );
        }
    });
    Ok(Value::Null)
}

static LAST_INVOCATION: Mutex<Option<Invocation>> = Mutex::new(None);

fn record_launch_preparation_error(version_id: &str, message: &str) {
    with_globals(|gg| {
        let mut ll = gg.last_launch.lock().unwrap();
        let matches = ll
            .as_ref()
            .map(|l| l["versionId"].as_str() == Some(version_id))
            .unwrap_or(false);
        if !matches {
            *ll = Some(json!({
                "versionId": version_id,
                "javaPath": "",
                "startedAt": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
            }));
        }
        if let Some(l) = ll.as_mut() {
            l["spawnError"] = json!(message);
            l["endedAt"] = json!(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
        }
    });
}

// ---------------- 无进展 deadline（launch.ts ProgressDeadline） ----------------

struct Deadline {
    last: Mutex<std::time::Instant>,
    aborted: AtomicBool,
}

impl Deadline {
    fn new() -> Self {
        Self { last: Mutex::new(std::time::Instant::now()), aborted: AtomicBool::new(false) }
    }
    fn touch(&self) {
        *self.last.lock().unwrap() = std::time::Instant::now();
    }
    fn check(&self) -> Result<(), String> {
        if self.aborted.load(Ordering::SeqCst) {
            return Err("启动准备已连续 60 秒没有进展，已取消本次启动；正在收尾，请稍后重试。".into());
        }
        if self.last.lock().unwrap().elapsed().as_millis() as u64 > DEADLINE_MS {
            self.aborted.store(true, Ordering::SeqCst);
            return Err("启动准备已连续 60 秒没有进展，已取消本次启动；正在收尾，请稍后重试。".into());
        }
        Ok(())
    }
}

// ---------------- 启动管线 ----------------

fn launch_pipeline(app: &AppHandle, inv: &Invocation) -> Result<(), String> {
    if with_globals(|gg| gg.restart_pending.lock().unwrap().is_some()) {
        return Err("正在重启游戏，请稍后再启动".into());
    }
    let token = reserve_session(inv);
    let deadline = Deadline::new();
    let result = launch_owned(app, inv, token.clone(), &deadline);
    if result.is_err() {
        release_session(&token);
    }
    result
}

/// 日志三落盘（latest.log / stdout.log / stderr.log，对应 launch.ts logStream 组）
struct LogSink {
    latest: Mutex<Option<std::fs::File>>,
    stdout: Mutex<Option<std::fs::File>>,
    stderr: Mutex<Option<std::fs::File>>,
}

impl LogSink {
    fn open(dir: &Path) -> LogSink {
        let mk = |name: &str| std::fs::File::create(dir.join(name)).ok();
        LogSink {
            latest: Mutex::new(mk("latest.log")),
            stdout: Mutex::new(mk("stdout.log")),
            stderr: Mutex::new(mk("stderr.log")),
        }
    }
    fn write(which: &Mutex<Option<std::fs::File>>, line: &str) {
        if let Ok(mut guard) = which.lock() {
            if let Some(f) = guard.as_mut() {
                let _ = f.write_all(format!("{}\n", line).as_bytes());
            }
        }
    }
}

/// gameExit.ts GameExitEvidence：按日志行归类退出原因
#[derive(Default)]
struct ExitEvidence {
    stopping: AtomicBool,
    shutdown_watchdog: AtomicBool,
    other_failure: AtomicBool,
    /// 加载器级致命错误（FormattedException / ModResolutionException 等）：
    /// 实测 Fabric 0.20 打出这类错误后进程挂住不退出，看门狗据此收尸
    fatal_bootstrap: AtomicBool,
    /// 看门狗已收尸（退出文本标注用）
    killed_by_watchdog: AtomicBool,
    reports: Mutex<(u32, u32)>, // (reports, describedReports)
}

impl ExitEvidence {
    fn observe(&self, line: &str) {
        if regex::Regex::new(
            r"net\.fabricmc\.loader\.impl\.FormattedException|ModResolutionException|Mod discovery failed!|org\.spongepowered\.asm\.mixin\.transformer\.MixinTransformerError",
        )
        .unwrap()
        .is_match(line)
        {
            self.fatal_bootstrap.store(true, Ordering::SeqCst);
        }
        if regex::Regex::new(r"\[(?:Render thread|Client thread)/INFO\](?:\s*\[[^\]]+\])?:\s*Stopping!\s*$")
            .unwrap()
            .is_match(line)
        {
            self.stopping.store(true, Ordering::SeqCst);
        }
        if regex::Regex::new(r"^---- Minecraft Crash Report ----\s*$")
            .unwrap()
            .is_match(line)
        {
            self.reports.lock().unwrap().0 += 1;
        }
        let (reports, described) = {
            let r = self.reports.lock().unwrap();
            *r
        };
        if reports > described && regex::Regex::new(r"^Description:\s*").unwrap().is_match(line) {
            self.reports.lock().unwrap().1 += 1;
            if self.stopping.load(Ordering::SeqCst)
                && line.trim() == "Description: Client shutdown from post-main"
            {
                self.shutdown_watchdog.store(true, Ordering::SeqCst);
            } else {
                self.other_failure.store(true, Ordering::SeqCst);
            }
        }
        if regex::Regex::new(
            r#"A fatal error has been detected by the Java Runtime Environment|Encountered an unexpected exception|Reported exception thrown!|Exception in thread "(?:main|Render thread|Client thread)""#,
        )
        .unwrap()
        .is_match(line)
        {
            self.other_failure.store(true, Ordering::SeqCst);
        }
    }

    fn classify(&self, code: i32, intentional: bool) -> &'static str {
        if intentional {
            return "stopped";
        }
        if code == 0 {
            return "normal";
        }
        // -8 + Stopping! + watchdog 描述 = 退出清理超时（win32）
        let (reports, described) = *self.reports.lock().unwrap();
        if code == -8
            && self.stopping.load(Ordering::SeqCst)
            && self.shutdown_watchdog.load(Ordering::SeqCst)
            && !self.other_failure.load(Ordering::SeqCst)
            && reports == described
        {
            return "shutdown-timeout";
        }
        "abnormal"
    }
}

#[allow(clippy::too_many_arguments)]
fn launch_owned(app: &AppHandle, inv: &Invocation, token: String, deadline: &Deadline) -> Result<(), String> {
    let version_id: &str = &inv.version_id;
    let folder: &str = &inv.folder;
    let s = settings::get(app);
    let mirror = installer::mirror_pref(app).to_string();

    // 日志落盘：<folder>/.wuhuicl/logs/<uuid>/（对应原版 gameDir/kamucl-logs/<uuid>）
    let launch_log_dir = Path::new(&folder).join(installer::DATA_DIR_NAME).join("logs").join(uuid_v4());
    let _ = std::fs::create_dir_all(&launch_log_dir);
    let sink = Arc::new(LogSink::open(&launch_log_dir));
    let session_started_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let effective_dir_hint = effective_game_dir(app, folder, version_id).unwrap_or_else(|_| folder.to_string());
    with_globals(|gg| {
        *gg.last_launch.lock().unwrap() = Some(json!({
            "versionId": version_id,
            "javaPath": "",
            "startedAt": session_started_at,
            "effectiveGameDir": effective_dir_hint,
            "logDir": launch_log_dir.to_string_lossy()
        }));
    });

    // send_log(line, which)：which 0 = latest.log + event:launchLog；1/2 = stdout/stderr 落盘
    let send_log = {
        let app2 = app.clone();
        let sink2 = sink.clone();
        move |line: &str, which: u8| match which {
            1 => LogSink::write(&sink2.stdout, line),
            2 => LogSink::write(&sink2.stderr, line),
            _ => {
                LogSink::write(&sink2.latest, line);
                if let Some(win) = app2.get_webview_window("main") {
                    let _ = win.emit_to("main", "event:launchLog", line);
                }
            }
        }
    };
    let log = {
        let send_log = send_log.clone();
        move |line: String| send_log(&line, 0)
    };
    let evidence = Arc::new(ExitEvidence::default());
    // 游戏输出尾行环形缓冲（崩溃分析用，PCL Watcher 的最近 501 行语义）
    let out_tail: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    let mut spawned = false;
    let pipeline_started = std::time::Instant::now();
    let result = (|| -> Result<(), String> {
        // a0) 自愈：版本链 json / 链底客户端 jar 缺失时自动补全（launch.ts a0）
        let mut base_id_probe = version_id.to_string();
        let mut chain_broken = false;
        {
            let mut visited: Vec<String> = vec![version_id.to_string()];
            let mut cur = installer::read_version_json(app, folder, version_id);
            loop {
                match cur {
                    Err(_) => {
                        if base_id_probe == *version_id {
                            return Err(format!("版本 {} 文件丢失，请在游戏版本页重新安装", version_id));
                        }
                        chain_broken = true;
                        break;
                    }
                    Ok(j) => match j.get("inheritsFrom").and_then(|v| v.as_str()) {
                        Some(parent) => {
                            if visited.contains(&parent.to_string()) || visited.len() >= 32 {
                                return Err("版本继承链存在循环或超过 32 层".into());
                            }
                            base_id_probe = parent.to_string();
                            visited.push(base_id_probe.clone());
                            cur = installer::read_version_json(app, folder, &base_id_probe);
                        }
                        None => break,
                    },
                }
            }
        }
        let base_in_versions = installer::version_json_path_in(folder, &base_id_probe).exists();
        let jar_probe = if base_in_versions {
            installer::version_jar_path_in(folder, &base_id_probe)
        } else {
            installer::base_version_jar_path_in(&installer::default_folder_path(app), &base_id_probe)
        };
        if chain_broken || !jar_probe.exists() {
            let own = installer::read_version_json(app, folder, version_id).ok();
            let flattened = own
                .as_ref()
                .map(|j| j.get("_flattenedAt").is_some() && j.get("inheritsFrom").is_none())
                .unwrap_or(false);
            if flattened && !chain_broken {
                // 自包含实例：仅补客户端 jar，绝不重写合并后的 json
                emit_progress(app, json!({ "stage": "repair", "progress": 0, "text": "检测到游戏本体缺失，正在自动补全…" }));
                repair_client_jar(app, folder, &base_id_probe, &jar_probe, &mirror, deadline)?;
                emit_progress(app, json!({ "stage": "repair", "progress": 1, "text": "文件补全完成" }));
            } else {
                emit_progress(app, json!({ "stage": "repair", "progress": 0, "text": format!("检测到游戏文件缺失，正在自动补全 {}…", base_id_probe) }));
                let real_id = own
                    .as_ref()
                    .and_then(|j| j.get("_mcVersion").and_then(|v| v.as_str()).map(str::to_string))
                    .or_else(|| {
                        installer::read_version_json(app, folder, &base_id_probe)
                            .ok()
                            .and_then(|b| b.get("_mcVersion").and_then(|v| v.as_str()).map(str::to_string))
                    })
                    .unwrap_or_else(|| base_id_probe.clone());
                if !is_minecraft_version_id(&real_id) {
                    return Err("无法可靠识别实例的 Minecraft 版本，不能自动补全游戏文件；请检查版本信息或重新安装".into());
                }
                let dest = if base_id_probe != *version_id && !base_in_versions { "base" } else { "versions" };
                let rec = dummy_task_record();
                deadline.touch();
                // installVanilla 复用 installer.rs（自定义命名实例以 baseIdProbe 作实例名）
                let real_id2 = real_id.clone();
                let final_name = if dest == "versions" && base_id_probe != real_id2 {
                    Some(base_id_probe.clone())
                } else {
                    None
                };
                let app3 = app.clone();
                let emit = move |event: &Value| {
                    emit_progress(&app3, event.clone());
                };
                installer::install_vanilla(app, &rec, &emit, folder, &real_id2, dest, final_name.as_deref(), false)?;
                emit_progress(app, json!({ "stage": "repair", "progress": 1, "text": "文件补全完成" }));
            }
        }

        // a0.1) 目录判定（instances.ts instanceDirectoryState）+ 默认中文 lang
        let effective_dir = effective_game_dir(app, folder, version_id)?;
        std::fs::create_dir_all(&effective_dir).map_err(|e| e.to_string())?;
        let opt_file = Path::new(&effective_dir).join("options.txt");
        if !opt_file.exists() {
            let _ = std::fs::write(&opt_file, "lang:zh_cn\n");
        }
        // 实际 MC 版本（决定选项编码与按键格式）；正式版用版本链底 id，实例自带 _mcVersion 优先
        let options_mc_version = installer::read_version_json(app, folder, version_id)
            .ok()
            .and_then(|j| j.get("_mcVersion").and_then(|v| v.as_str()).map(str::to_string))
            .or_else(|| {
                installer::resolve_version_chain(app, folder, version_id)
                    .ok()
                    .map(|(merged, base_id)| {
                        merged
                            .get("_mcVersion")
                            .and_then(|v| v.as_str())
                            .map(str::to_string)
                            .unwrap_or(base_id)
                    })
            })
            .unwrap_or_default();
        // 默认游戏选项同步（总开关开启时覆盖实例 options.txt 的玩家修改项，其余行原样保留）
        let (applied_options, unsupported_options) =
            crate::gameoptions::sync(app, &effective_dir, &options_mc_version);
        if !applied_options.is_empty() {
            log(format!(
                "[WUHUICL] 已同步 {} 项默认游戏选项到 options.txt",
                applied_options.len()
            ));
        }
        if !unsupported_options.is_empty() {
            log(format!(
                "[WUHUICL] Minecraft {} 不支持以下默认选项，已跳过：{}",
                options_mc_version,
                unsupported_options.join("、")
            ));
        }
        // 默认按键同步（keySync 开启且版本 ≥1.13 时覆盖 key_* 项）
        if crate::settings::get(app)["keySync"].as_bool().unwrap_or(false)
            && crate::gameoptions::mc_version_at_least_pub(&options_mc_version, "1.13")
        {
            if crate::misc::sync_keys_to_game_dir(app, &effective_dir) {
                log("[WUHUICL] 已同步默认按键到 options.txt".into());
            }
        }

        // a) 版本链合并
        emit_progress(app, json!({ "stage": "launch", "progress": 0, "text": "解析版本信息" }));
        deadline.check()?;
        let (merged, base_id) = installer::resolve_version_chain(app, folder, version_id)?;
        if merged.get("mainClass").and_then(|v| v.as_str()).map(|v| v.is_empty()).unwrap_or(true) {
            return Err("版本 json 缺少 mainClass，文件可能损坏".into());
        }
        let instance_config = installer::read_version_json(app, folder, version_id)?;
        let instance_mc_version = instance_config
            .get("_mcVersion")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| base_id.clone());
        let client_jar = installer::client_jar_path(app, folder, &base_id);

        // b) 账号（mslogin.rs 全量账号，含 accessToken）
        let account = crate::mslogin::selected_full(app).ok_or("尚未选择账号，请先在账号页添加并选择一个账号")?;
        let valid_account = crate::mslogin::get_valid_account(app, account.clone())?;
        set_launch_account(valid_account.clone());

        // 客户端 jar sha1 校验 + 修复（launch.ts prepareClient 的校验部分）
        if let Some(sha1) = merged
            .get("downloads")
            .and_then(|d| d.get("client"))
            .and_then(|c| c.get("sha1"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
        {
            if client_jar.exists() {
                if let Ok(actual) = installer::sha1_file_hex(&client_jar) {
                    if actual != sha1.to_lowercase() {
                        emit_progress(app, json!({ "stage": "repair", "progress": 0, "text": "修复游戏本体" }));
                        repair_client_jar(app, folder, &base_id, &client_jar, &mirror, deadline)?;
                    }
                }
            }
        }
        deadline.touch();

        // 外置登录 javaagent（yggdrasil）
        let external_auth_args = crate::mslogin::launch_arguments(app, &valid_account)?;
        deadline.touch();

        // c) Java（launch.ts Java 环境）
        emit_progress(app, json!({ "stage": "java", "progress": 0, "text": "检查 Java 环境" }));
        deadline.check()?;
        let java_path = resolve_launch_java(app, &merged, &instance_config, &instance_mc_version, deadline)?;

        // d) classpath 与 natives 解压
        emit_progress(app, json!({ "stage": "launch", "progress": 0.5, "text": "准备运行库与 natives" }));
        deadline.check()?;
        let natives_path = installer::natives_dir_in(folder, version_id);
        std::fs::create_dir_all(&natives_path).map_err(|e| e.to_string())?;
        let (artifacts, natives) = resolved_libraries(app, &merged);
        repair_missing_libraries(app, &merged, &mirror, deadline)?;
        // 资源补全（PCL DlClientFix 的 assets 部分）：缺索引/缺对象都在启动前补齐，
        // 否则缺音效贴图时游戏能开但内容残缺
        {
            let index_id = merged
                .get("assetIndex")
                .and_then(|a| a.get("id"))
                .and_then(|v| v.as_str())
                .or_else(|| merged.get("assets").and_then(|v| v.as_str()))
                .unwrap_or("")
                .to_string();
            let index_url = merged
                .get("assetIndex")
                .and_then(|a| a.get("url"))
                .and_then(|v| v.as_str())
                .map(str::to_string);
            if !index_id.is_empty() {
                match installer::asset_repair_tasks(app, None, &index_id, index_url.as_deref(), &mirror) {
                    Ok(items) if !items.is_empty() => {
                        let count = items.len();
                        emit_progress(app, json!({ "stage": "repair", "progress": 0, "text": format!("补全资源文件 0/{}", count) }));
                        let app2 = app.clone();
                        let progress = move |d: usize, t: usize, _speed: f64, _detail: &installer::DownloadDetail| {
                            emit_progress(
                                &app2,
                                json!({
                                    "stage": "repair",
                                    "progress": d as f64 / t.max(1) as f64,
                                    "text": format!("补全资源文件 {}/{}", d, t.max(1))
                                }),
                            );
                        };
                        installer::download_all(app, None, items, &mirror, &progress)?;
                    }
                    Ok(_) => {}
                    Err(e) => {
                        // 资源缺失不阻断启动（与原版一致：assets 可后台补），只记日志
                        let _ = e;
                    }
                }
            }
        }
        deadline.touch();
        extract_natives(&natives, &natives_path)?;
        let mut classpath_entries: Vec<String> = Vec::new();
        for p in artifacts.iter().chain(natives.iter()).chain(std::iter::once(&client_jar)) {
            let key = p.to_string_lossy().to_lowercase();
            if !classpath_entries.iter().any(|e| e.to_lowercase() == key) {
                classpath_entries.push(p.to_string_lossy().to_string());
            }
        }
        let classpath = classpath_entries.join(";");

        // 资源目录（prepareLaunchAssets 的定位部分；缺失资源由安装管线补全）
        let assets_root = installer::assets_dir(app);
        let index_id = merged
            .get("assetIndex")
            .and_then(|a| a.get("id"))
            .and_then(|v| v.as_str())
            .or_else(|| merged.get("assets").and_then(|v| v.as_str()))
            .unwrap_or("legacy")
            .to_string();
        let game_assets = if index_id == "legacy" || index_id == "pre-1.6" {
            installer::virtual_legacy_dir(app).to_string_lossy().to_string()
        } else {
            String::new()
        };
        log(format!("[WUHUICL] 游戏资源：{}；索引：{}", assets_root.display(), index_id));

        // 变量表（launch.ts vars）：供 ${...} 替换
        let launcher_version = app.package_info().version.to_string();
        let vars = json!({
            "version_name": version_id,
            "game_directory": effective_dir,
            "assets_root": assets_root.to_string_lossy(),
            "assets_index_name": index_id,
            "game_assets": game_assets,
            "natives_directory": natives_path.to_string_lossy(),
            "launcher_name": "WUHUICL",
            "launcher_version": launcher_version,
            "classpath": classpath,
            "library_directory": installer::libraries_dir(app).to_string_lossy(),
            "classpath_separator": ";",
            "version_type": "WUHUICL",
            "clientid": "",
            "auth_xuid": ""
        });
        *LAUNCH_VARS.lock().unwrap() = Some(vars);

        // e) 内存 / JVM 参数（json 自带 -Djava.library.path 时不再重复）
        let total_mem_mb = total_mem_mb();
        let mem: u64 = if s["memoryAuto"].as_bool().unwrap_or(true) {
            auto_memory_mb(total_mem_mb)
        } else {
            ((s["memoryMB"].as_i64().unwrap_or(4096).max(512)) as i64).min(total_mem_mb as i64).max(0) as u64
        };
        let user_jvm = split_args(s["jvmArgs"].as_str().unwrap_or(""));
        let client_jar_name = client_jar
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let json_jvm_args: Vec<String> = expand_entries(merged.get("arguments").and_then(|a| a.get("jvm")).and_then(|v| v.as_array()))
            .into_iter()
            .map(|a| a.trim().to_string())
            .map(|a| {
                // forge ignoreList 需精确匹配 -cp 上的原版客户端 jar 文件名
                if a.starts_with("-DignoreList=") && !a.split(',').any(|x| x.trim() == client_jar_name) {
                    format!("{},{}", a, client_jar_name)
                } else {
                    a
                }
            })
            .collect();
        let json_has = |prefix: &str| json_jvm_args.iter().any(|a| a.starts_with(prefix));
        let json_has_cp = json_jvm_args.iter().any(|a| a == "-cp");
        let natives_str = natives_path.to_string_lossy().to_string();
        let mut jvm_args: Vec<String> = vec![
            format!("-Xmx{}M", mem),
            format!("-Xms{}M", mem.min(1024)),
            "-XX:+UseG1GC".into(),
            "-XX:+ParallelRefProcEnabled".into(),
            "-XX:MaxGCPauseMillis=200".into(),
            "-Dfile.encoding=UTF-8".into(),
        ];
        if !json_has("-Djava.library.path=") {
            jvm_args.push(format!("-Djava.library.path={}", natives_str));
        }
        if !json_has("-Djna.tmpdir=") {
            jvm_args.push(format!("-Djna.tmpdir={}", natives_str));
        }
        // 外置登录 javaagent 与预取元数据必须位于主类之前
        jvm_args.extend(external_auth_args.iter().cloned());
        // 版本 json 自带的 JVM 参数（forge 的 -p ${classpath} 等依赖它）
        jvm_args.extend(json_jvm_args.iter().cloned());
        jvm_args.extend(user_jvm.iter().cloned());

        // 游戏参数（新版 arguments.game / 旧版 minecraftArguments）
        let game_arr = merged.get("arguments").and_then(|a| a.get("game")).and_then(|v| v.as_array());
        let mut game_args: Vec<String> = if game_arr.map(|a| !a.is_empty()).unwrap_or(false) {
            expand_entries(game_arr)
        } else if let Some(ma) = merged.get("minecraftArguments").and_then(|v| v.as_str()) {
            ma.split_whitespace().map(substitute).collect()
        } else {
            return Err("版本 json 缺少游戏参数，文件可能损坏".into());
        };

        // e2) 实例覆盖 > 全局设置；maximized 使用当前显示器工作区
        let instance_res = instance_config.get("_resolution").filter(|v| v.is_object()).cloned();
        let resolution = resolve_game_resolution(s.get("resolution"), instance_res.as_ref())?;
        let work_area = if resolution["mode"].as_str() == Some("maximized") {
            work_area_size()
        } else {
            None
        };
        let window_args = build_game_window_arguments(game_args, &resolution, work_area.as_ref());
        game_args = window_args.args;

        // e3) Quick Play（1.20+ / 23w14a+）与命令世界
        let mut singleplayer_world: Option<String> = None;
        if inv.create_command_world {
            let world = create_command_world(&effective_dir, &client_jar)?;
            log(format!("[WUHUICL] 已新建允许命令的创造测试世界：{}", world.1));
            singleplayer_world = Some(world.0);
        }
        if let Some(world) = &singleplayer_world {
            if !Path::new(&effective_dir).join("saves").join(world).join("level.dat").exists() {
                return Err("待进入的测试世界不存在，未创建重复世界".into());
            }
            game_args.push("--quickPlaySingleplayer".into());
            game_args.push(world.clone());
        } else if let Some(addr) = &inv.server_address {
            if supports_quick_play_multiplayer(&instance_mc_version) {
                game_args.push("--quickPlayMultiplayer".into());
                game_args.push(addr.clone());
            } else {
                log(format!("[WUHUICL] Minecraft {} 不支持 Quick Play，已仅启动实例", instance_mc_version));
            }
        }

        // g) 组装最终命令（json 自带 -cp 时不再重复；forge -p 是模块路径仍需 -cp）
        let mut args: Vec<String> = jvm_args.clone();
        if !json_has_cp {
            args.push("-cp".into());
            args.push(classpath.clone());
        }
        args.push(merged.get("mainClass").and_then(|v| v.as_str()).unwrap_or("").to_string());
        args.extend(game_args.iter().cloned());

        // 日志脱敏（accessToken / clientToken / userProperties / prefetched 元数据）
        let access_token = valid_account.get("accessToken").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let client_token = valid_account.get("clientToken").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let user_properties = serialize_user_properties(valid_account.get("userProperties"));
        let private_values: Vec<String> = [access_token.clone(), client_token.clone(), user_properties.clone()]
            .into_iter()
            .filter(|v| !v.is_empty())
            .collect();
        let log_args: Vec<String> = args
            .iter()
            .map(|a| {
                if a.starts_with("-Dauthlibinjector.yggdrasil.prefetched=") {
                    return "-Dauthlibinjector.yggdrasil.prefetched=<metadata>".to_string();
                }
                if private_values.iter().any(|p| p == a) {
                    return "***".to_string();
                }
                if !access_token.is_empty() && a.contains(&access_token) {
                    return a.replace(&access_token, "***");
                }
                a.clone()
            })
            .collect();
        let command_summary = format!(
            "{} {}",
            java_path,
            log_args.iter().map(|a| windows_quote(a)).collect::<Vec<_>>().join(" ")
        );
        log(format!(
            "[WUHUICL] 游戏窗口: mode={}{}",
            window_args.mode,
            match (&window_args.width, &window_args.height) {
                (Some(w), Some(h)) => format!(", width={}, height={}", w, h),
                _ => ", fullscreen=true".to_string(),
            }
        ));
        log(format!("[WUHUICL] 启动命令: {}", command_summary));
        emit_progress(app, json!({ "stage": "launch", "progress": 1, "text": "启动游戏进程" }));
        deadline.touch();

        // g) 启动进程（脱离式：Tauri/Rust 子进程不在 Job Object 内，天然与启动器生命周期解耦）
        // 崩溃报告基线：启动前快照 crash-reports，退出后 diff（美西螈 process.rs 口径）
        let crash_before = snapshot_crash_reports(&effective_dir);
        let mut cmd = std::process::Command::new(&java_path);
        cmd.args(&args)
            .current_dir(&effective_dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = cmd.spawn().map_err(|e| format!("进程启动失败：{}", e))?;
        let pid = child.id();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let proc = Arc::new(ChildProc {
            pid,
            child: Mutex::new(child),
            exit_code: Mutex::new(None),
        });
        attach_session(&token, &proc);
        spawned = true;

        // stdout/stderr 按行转发（launch.ts makeLinePusher）+ 尾行环形缓冲
        let ev = evidence.clone();
        let pipe = send_log.clone();
        let tail = out_tail.clone();
        spawn_reader(stdout, move |line| {
            ev.observe(&line);
            pipe(&line, 1);
            pipe(&line, 0);
            {
                let mut t = tail.lock().unwrap();
                t.push(line);
                if t.len() > 500 {
                    t.remove(0);
                }
            }
        });
        let ev = evidence.clone();
        let pipe = send_log.clone();
        let tail = out_tail.clone();
        spawn_reader(stderr, move |line| {
            ev.observe(&line);
            pipe(&line, 2);
            pipe(&line, 0);
            {
                let mut t = tail.lock().unwrap();
                t.push(line);
                if t.len() > 500 {
                    t.remove(0);
                }
            }
        });

        // 启动挂死看门狗（美西螈 15s 初始化超时的放宽版）：加载器打出致命错误后
        // 部分版本会挂住不退出（实测 Fabric 0.20 Mod discovery failed!），
        // 20 秒后仍在运行即收尸并走崩溃分析
        {
            let ev_wd = evidence.clone();
            let proc_wd = proc.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(20));
                if !ev_wd.fatal_bootstrap.load(Ordering::SeqCst) {
                    return;
                }
                // 还活着才收尸
                let alive = {
                    let mut guard = proc_wd.child.lock().unwrap();
                    guard.try_wait().map(|s| s.is_none()).unwrap_or(false)
                };
                if !alive {
                    return;
                }
                ev_wd.killed_by_watchdog.store(true, Ordering::SeqCst);
                let mut guard = proc_wd.child.lock().unwrap();
                let _ = guard.kill();
            });
        }

        with_globals(|gg| {
            *gg.last_launch.lock().unwrap() = Some(json!({
                "versionId": version_id,
                "javaPath": java_path,
                "startedAt": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                "effectiveGameDir": effective_dir,
                "logDir": launch_log_dir.to_string_lossy(),
                "commandSummary": command_summary,
                "windowMode": window_args.mode,
                "windowWidth": window_args.width,
                "windowHeight": window_args.height,
                "pid": pid
            }));
        });
        persist_running_game(
            app,
            &json!({
                "pid": pid,
                "versionId": version_id,
                "effectiveGameDir": effective_dir,
                "logDir": launch_log_dir.to_string_lossy(),
                "startedAt": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
            }),
        );

        // 运行状态（launch.ts 在进程创建成功后立即置 running）
        send_state(
            app,
            json!({
                "status": "running",
                "text": "游戏进程已启动",
                "versionId": version_id,
                "folder": folder,
                "launchId": inv.launch_id
            }),
        );
        // 设置项：游戏成功运行 1.5s 后关闭启动器窗口（ipc.ts closeAfterLaunch）
        if s["closeAfterLaunch"].as_bool().unwrap_or(false) {
            let app2 = app.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(1500));
                if let Some(win) = app2.get_webview_window("main") {
                    let _ = win.close();
                }
            });
        }

        // 窗口尺寸观察（gameWindowSize.ts；settings.rememberGameWindowSize 开启时）
        let remember_window = s["rememberGameWindowSize"].as_bool().unwrap_or(false);
        let watcher_stop = Arc::new(AtomicBool::new(false));
        let latest_size: Arc<Mutex<Option<(i64, i64)>>> = Arc::new(Mutex::new(None));
        if remember_window {
            let stop = watcher_stop.clone();
            let latest = latest_size.clone();
            std::thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    if let Some(sample) = read_owned_game_window(pid) {
                        if sample.0 >= GAME_WIDTH_MIN
                            && sample.0 <= GAME_WIDTH_MAX
                            && sample.1 >= GAME_HEIGHT_MIN
                            && sample.1 <= GAME_HEIGHT_MAX
                        {
                            *latest.lock().unwrap() = Some(sample);
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
            });
        }

        // 退出监视（对应 proc.on('close')）
        let app2 = app.clone();
        let token2 = token.clone();
        let version2 = version_id.to_string();
        let folder2 = folder.to_string();
        let launch_id2 = inv.launch_id.clone();
        let ev2 = evidence.clone();
        let latest2 = latest_size.clone();
        let stop2 = watcher_stop.clone();
        let crash_dir2 = effective_dir.clone();
        let out_tail2 = out_tail.clone();
        let send_log2 = send_log.clone();
        std::thread::spawn(move || {
            // 轮询等待退出（对应 DetachedGameProcess.waitForExit 的 250ms 零超时探测；
            // 不长期持有 child 锁，保证 kill/强杀路径能随时进入）
            let code = loop {
                {
                    let mut guard = proc.child.lock().unwrap();
                    if let Ok(Some(status)) = guard.try_wait() {
                        break status.code().unwrap_or(-1);
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            };
            *proc.exit_code.lock().unwrap() = Some(code);
            // 对应 launch.ts:860-863：游戏退出即写 exitCode + endedAt，
            // 直连页 latest_lan_port 靠它判旧端口失效（缺了就会一直报已关闭世界的端口）
            with_globals(|gg| {
                let mut ll = gg.last_launch.lock().unwrap();
                if let Some(l) = ll.as_mut() {
                    if l["pid"].as_u64() == Some(proc.pid as u64) {
                        l["exitCode"] = json!(code);
                        l["endedAt"] = json!(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
                    }
                }
            });
            stop2.store(true, Ordering::SeqCst);
            let intentional_restart = take_intentional_restart(&launch_id2);
            let intentional = {
                let entry = find_session(&token2);
                let stopping = entry.map(|e| e.intentional_stop.load(Ordering::SeqCst)).unwrap_or(false);
                stopping || intentional_restart
            };
            let exit_kind = ev2.classify(code, intentional);
            release_session(&token2);
            if code == 0 || intentional {
                if remember_window {
                    if let Some((w, h)) = latest2.lock().unwrap().clone() {
                        let _ = commit_window_size(&app2, &folder2, &version2, w, h);
                    }
                }
            }
            let app3 = app2.clone();
            let file = settings::user_data_dir(&app3).join("running-game.json");
            let _ = std::fs::remove_file(file);
            let mut text = if exit_kind == "shutdown-timeout" {
                "游戏已关闭；退出清理超时，日志已保留".to_string()
            } else {
                format!("游戏已退出 (code={})", code)
            };
            let mut crash_analysis: Option<Value> = None;
            // 崩溃分析（PCL ModCrash.vb 移植）：新崩溃报告/最新日志/输出尾行 → 规则表归因
            if exit_kind == "abnormal" {
                let crash_path = newest_new_crash(&crash_dir2, &crash_before);
                let tail_lines = out_tail2.lock().unwrap().clone();
                let reasons =
                    crashanalyzer::analyze(Path::new(&crash_dir2), crash_path.as_deref(), &tail_lines);
                let reasons = if reasons.is_empty()
                    || (reasons.len() == 1 && reasons[0].title == crashanalyzer::REASON_NONE)
                {
                    Vec::new()
                } else {
                    reasons
                };
                if !reasons.is_empty() {
                    let top = crashanalyzer::reason_text(&reasons[0]);
                    let suffix = if ev2.killed_by_watchdog.load(Ordering::SeqCst) {
                        "（加载器启动失败后进程无响应，已由启动器结束）"
                    } else {
                        ""
                    };
                    text = format!("游戏崩溃：{}{}", top, suffix);
                    crash_analysis = Some(crashanalyzer::to_json(&reasons));
                    for r in &reasons {
                        send_log2(&format!("[WUHUICL] 崩溃分析：{}", crashanalyzer::reason_text(r)), 0);
                    }
                } else if let Some(desc) = crash_path.as_deref().and_then(crash_description) {
                    text = format!("游戏崩溃：{}", desc);
                } else if ev2.other_failure.load(Ordering::SeqCst) {
                    text = format!("游戏异常退出（疑似崩溃，code={}，日志已保留）", code);
                }
            }
            send_state(
                &app2,
                json!({
                    "status": "exited",
                    "code": code,
                    "exitKind": exit_kind,
                    "intentionalRestart": intentional_restart,
                    "intentionalStop": intentional,
                    "text": text,
                    "crashAnalysis": crash_analysis,
                    "versionId": version2,
                    "folder": folder2,
                    "launchId": launch_id2
                }),
            );
        });
        Ok(())
    })();

    if !spawned {
        release_session(&token);
    }
    let _ = log(format!("[WUHUICL] 启动准备结束（{}ms）", pipeline_started.elapsed().as_millis()));
    result
}

fn take_intentional_restart(launch_id: &str) -> bool {
    RESTARTING.lock().unwrap().take().map(|id| id == launch_id).unwrap_or(false)
}

// ---------------- 崩溃报告快照（美西螈 process.rs:43-96 的 crash-reports 前后对比） ----------------

type CrashSnapshot = Vec<(String, u64, u64)>; // (文件名, 大小, mtime 秒)

fn snapshot_crash_reports(game_dir: &str) -> CrashSnapshot {
    let dir = Path::new(game_dir).join("crash-reports");
    let mut out: CrashSnapshot = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let p = e.path();
            if !p.is_file() {
                continue;
            }
            let Ok(meta) = e.metadata() else { continue };
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            out.push((e.file_name().to_string_lossy().to_string(), meta.len(), mtime));
        }
    }
    out
}

/// 退出后发现新崩溃报告 → 返回其路径
fn newest_new_crash(game_dir: &str, before: &CrashSnapshot) -> Option<PathBuf> {
    let dir = Path::new(game_dir).join("crash-reports");
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    for e in std::fs::read_dir(&dir).ok()?.flatten() {
        let p = e.path();
        if !p.is_file() {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        let meta = e.metadata().ok()?;
        let mtime = meta.modified().ok()?;
        let known = before
            .iter()
            .any(|(n, size, t)| {
                *n == name
                    && *size == meta.len()
                    && mtime
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() == *t)
                        .unwrap_or(false)
            });
        if known {
            continue;
        }
        if newest.as_ref().map(|(t, _)| mtime > *t).unwrap_or(true) {
            newest = Some((mtime, p));
        }
    }
    newest.map(|(_, p)| p)
}

/// 崩溃报告的 Description 行
fn crash_description(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    text.lines()
        .find(|l| l.trim_start().starts_with("Description: "))
        .map(|l| l.trim_start()["Description: ".len()..].trim().to_string())
}

/// 兼容旧调用：最新新报告的 Description
fn describe_new_crash(game_dir: &str, before: &CrashSnapshot) -> Option<String> {
    let path = newest_new_crash(game_dir, before)?;
    let desc = crash_description(&path)?;
    Some(format!(
        "{}（崩溃报告 {}）",
        desc,
        path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
    ))
}

// ---------------- 会话操作 ----------------

fn reserve_session(inv: &Invocation) -> String {
    let token = uuid_v4();
    let entry = Arc::new(SessionEntry {
        token: token.clone(),
        version_id: inv.version_id.clone(),
        folder: inv.folder.clone(),
        launch_id: inv.launch_id.clone(),
        server_address: inv.server_address.clone(),
        stopping: AtomicBool::new(false),
        stop_approval: Mutex::new(None),
        exited: AtomicBool::new(false),
        intentional_stop: AtomicBool::new(false),
        child: Mutex::new(None),
    });
    with_globals(|gg| gg.sessions.lock().unwrap().push(entry));
    token
}

fn find_session(token: &str) -> Option<Arc<SessionEntry>> {
    with_globals(|gg| gg.sessions.lock().unwrap().iter().find(|s| s.token == token).cloned())
}

fn attach_session(token: &str, proc: &Arc<ChildProc>) {
    if let Some(entry) = find_session(token) {
        *entry.child.lock().unwrap() = Some(proc.clone());
    }
}

fn release_session(token: &str) -> bool {
    with_globals(|gg| {
        let mut sessions = gg.sessions.lock().unwrap();
        if let Some(pos) = sessions.iter().position(|s| s.token == token) {
            if sessions[pos].stopping.load(Ordering::SeqCst) {
                sessions[pos].intentional_stop.store(true, Ordering::SeqCst);
            }
            sessions.remove(pos);
            true
        } else {
            false
        }
    })
}

fn token_of(version_id: &str) -> Option<String> {
    with_globals(|gg| {
        let list = gg.sessions.lock().unwrap();
        list.iter()
            .filter(|s| !s.exited.load(Ordering::SeqCst))
            .rev()
            .find(|s| s.version_id == version_id)
            .map(|s| s.token.clone())
    })
}

fn dummy_task_record() -> Arc<tasks::TaskRecord> {
    Arc::new(tasks::TaskRecord {
        id: format!("launch-{}", uuid_v4()),
        title: "启动准备".into(),
        kind: "launch",
        cancelled: Arc::new(AtomicBool::new(false)),
        paused: Arc::new(AtomicBool::new(false)),
        status: Mutex::new(tasks::TaskStatus::Running),
    })
}

// ---------------- Java 选择（launch.ts Java 环境 + javaCompatibility） ----------------

/// javaCompatibility.ts releaseJavaMajor：Mojang 官方发行带
fn release_java_major(id: &str) -> Option<i64> {
    if regex::Regex::new(r"^26\.1(?:\.\d+)?$").unwrap().is_match(id) {
        return Some(25);
    }
    if regex::Regex::new(r"^(?:[ab]\d|c\d|rd-|inf-)").unwrap().is_match(id) {
        return Some(8);
    }
    let caps = regex::Regex::new(r"^1\.(\d+)(?:\.(\d+))?$").unwrap().captures(id)?;
    let minor: i64 = caps.get(1)?.as_str().parse().ok()?;
    let patch: i64 = caps.get(2).map(|m| m.as_str().parse().unwrap_or(0)).unwrap_or(0);
    if minor > 21 {
        return None;
    }
    if minor == 21 || (minor == 20 && patch >= 5) {
        return Some(21);
    }
    if minor >= 18 {
        return Some(17);
    }
    if minor == 17 {
        Some(16)
    } else {
        Some(8)
    }
}

fn declared_java_major(json: &Value) -> Option<i64> {
    let n = json
        .get("javaVersion")
        .and_then(|j| j.get("majorVersion"))
        .and_then(|v| v.as_i64())?;
    (8..=99).contains(&n).then_some(n)
}

/// launch.ts resolveJavaRequirement 的移植（模组/ASM 约束为 TODO 钩子）
pub(crate) fn resolve_java_requirement(merged: &Value, mc_version: &str, instance_config: &Value) -> Result<(i64, Option<i64>), String> {
    let major = declared_java_major(merged)
        .or_else(|| release_java_major(mc_version))
        .or_else(|| declared_java_major(instance_config));
    match major {
        Some(major) => {
            let libs = merged.get("libraries").and_then(|l| l.as_array()).cloned().unwrap_or_default();
            let replacement_bootstrap = libs.iter().any(|l| {
                l.get("name")
                    .and_then(|v| v.as_str())
                    .map(|n| {
                        let n = n.to_lowercase();
                        n.contains("cleanroom") || n.contains("lwjgl3ify")
                    })
                    .unwrap_or(false)
            }) || merged
                .get("mainClass")
                .and_then(|v| v.as_str())
                .map(|c| c.to_lowercase().contains("cleanroom"))
                .unwrap_or(false);
            let old_forge = regex::Regex::new(r"^1\.(?:[0-9]|1[0-2])(?:\.|$)")
                .unwrap()
                .is_match(mc_version)
                && major == 8
                && !replacement_bootstrap;
            Ok((major, if old_forge { Some(8) } else { None }))
        }
        None => Err(format!(
            "无法确认 {} 的 Java 需求；请补全官方版本元数据或在实例元数据中声明 Java 版本。",
            mc_version
        )),
    }
}

fn resolve_launch_java(
    app: &AppHandle,
    merged: &Value,
    instance_config: &Value,
    mc_version: &str,
    deadline: &Deadline,
) -> Result<String, String> {
    let s = settings::get(app);
    let (need, maximum) = resolve_java_requirement(merged, mc_version, instance_config)?;
    let version_java = instance_config.get("_javaPath").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let automatic = instance_config.get("_javaAuto").and_then(|v| v.as_bool()).unwrap_or(false)
        || (version_java.is_empty()
            && (s["javaAuto"].as_bool().unwrap_or(true) || s["javaPath"].as_str().unwrap_or("").is_empty()));
    let arch = host_arch();
    let ensure = |need: i64| -> Result<String, String> {
        // 自动管理：本机扫描选择；没有就自动下载 Mojang 官方运行时（PCL/美西螈同款）
        let list = crate::javaman::scan_quick(app, false);
        let java = match crate::javaman::select_by_major(&list, need, &arch) {
            Some(j) => j["path"].as_str().unwrap_or("").to_string(),
            None => {
                let exe = crate::javaman::ensure_runtime(app, need, &arch)
                    .map_err(|e| format!("{}；请在设置中选择 Java 或关闭自动管理", e))?;
                // 新装运行时必须重扫才能进列表（并刷新探测缓存）
                let _ = crate::javaman::scan_quick(app, true);
                exe.to_string_lossy().to_string()
            }
        };
        Ok(java)
    };
    let java_path: String;
    if instance_config.get("_javaAuto").and_then(|v| v.as_bool()).unwrap_or(false) {
        java_path = ensure(need)?;
        emit_progress(app, json!({ "stage": "java", "progress": 1, "text": format!("使用自动管理的 Java {}", need) }));
    } else if !version_java.is_empty() {
        if !Path::new(&version_java).exists() {
            return Err(format!("该版本指定的 Java 不存在（{}），请在版本设置中重新选择", version_java));
        }
        java_path = version_java;
        emit_progress(app, json!({ "stage": "java", "progress": 1, "text": "使用该版本指定的 Java" }));
    } else if s["javaAuto"].as_bool().unwrap_or(true) {
        java_path = ensure(need)?;
        emit_progress(app, json!({ "stage": "java", "progress": 1, "text": format!("使用自动管理的 Java {}", need) }));
    } else if !s["javaPath"].as_str().unwrap_or("").is_empty() {
        let p = s["javaPath"].as_str().unwrap_or("").to_string();
        if !Path::new(&p).exists() {
            return Err("手动指定的 Java 路径不存在，请在设置中重新选择".into());
        }
        java_path = p;
        emit_progress(app, json!({ "stage": "java", "progress": 1, "text": "使用手动指定的 Java" }));
    } else {
        let list = crate::javaman::scan_quick(app, false);
        let found = list
            .iter()
            .find(|j| {
                j["major"].as_i64() == Some(need)
                    && j["is64Bit"].as_bool().unwrap_or(false)
                    && (arch.is_empty() || j["architecture"].as_str().map(|a| a == arch).unwrap_or(false))
            })
            .map(|j| j["path"].as_str().unwrap_or("").to_string());
        match found {
            Some(p) if !p.is_empty() => {
                java_path = p;
                emit_progress(app, json!({ "stage": "java", "progress": 1, "text": format!("使用本机 Java {}", need) }));
            }
            _ => {
                return Err(format!(
                    "该版本需要 Java {} (64位)，但未找到（Java 自动管理已关闭）。请在设置中选择 Java 或开启自动管理",
                    need
                ))
            }
        }
    }
    deadline.touch();

    // 兼容性校验（javaCompatibility.javaCompatibilityError 的移植）
    let info = crate::javaman::probe(app, &java_path);
    let incompatibility = match &info {
        Some(info) => java_compatibility_error(info, need, maximum, &arch, automatic),
        None => Some("Java 无法正常运行".to_string()),
    };
    if let Some(err) = incompatibility {
        return Err(format!("所选 Java 不适配：{}；请修改实例设置或开启自动管理", err));
    }
    Ok(java_path)
}

fn java_compatibility_error(info: &Value, need: i64, maximum: Option<i64>, arch: &str, automatic: bool) -> Option<String> {
    if !info["is64Bit"].as_bool().unwrap_or(false)
        || (!arch.is_empty() && info["architecture"].as_str().map(|a| a != arch).unwrap_or(true))
    {
        return Some(if arch.is_empty() { "需要 64 位 Java".into() } else { format!("需要 64 位 {} Java", arch) });
    }
    let major = info["major"].as_i64().unwrap_or(0);
    if major < need {
        return Some(format!("至少需要 Java {}，当前为 Java {}", need, major));
    }
    if let Some(max) = maximum {
        if major > max {
            return Some(format!("旧版 Forge 需要 Java {}，当前为 Java {}", max, major));
        }
    }
    if automatic && major != need {
        return Some(format!("自动管理需要推荐的 Java {}，当前为 Java {}", need, major));
    }
    None
}

// ---------------- classpath / natives ----------------

/// 对应 versions.ts resolvedLibraries：复用 collect_libraries 的统一收集
/// （downloads 块与 maven 坐标两种声明都要收，Fabric 库没有 downloads 块，
///  漏收会导致 ClassNotFoundException: KnotClient → 游戏秒退）。
/// 去重已由 collect_libraries 完成（坐标 + 相对路径），此处按落盘路径再兜一层。
fn resolved_libraries(app: &AppHandle, merged: &Value) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut artifacts: Vec<PathBuf> = Vec::new();
    let mut natives: Vec<PathBuf> = Vec::new();
    for entry in installer::collect_libraries(merged) {
        let dest = installer::library_path(app, &entry.rel);
        let key = dest.to_string_lossy().to_lowercase();
        let seen = if entry.is_native { &mut natives } else { &mut artifacts };
        if seen.iter().any(|p: &PathBuf| p.to_string_lossy().to_lowercase() == key) {
            continue;
        }
        seen.push(dest);
    }
    (artifacts, natives)
}

/// 缺失依赖库补全（对应 launch.ts 启动前 repair 阶段）
fn repair_missing_libraries(app: &AppHandle, merged: &Value, mirror: &str, deadline: &Deadline) -> Result<(), String> {
    let all = installer::library_tasks(app, merged);
    let total = all.len();
    let items: Vec<installer::DownloadItem> = {
        let mut items = Vec::new();
        for (i, item) in all.into_iter().enumerate() {
            if !item.dest.exists() {
                items.push(item);
            }
            if i % 16 == 0 {
                emit_progress(
                    app,
                    json!({
                        "stage": "repair",
                        "progress": (i as f64 + 1.0) / total.max(1) as f64,
                        "text": format!("校验依赖库 {}/{}", i + 1, total)
                    }),
                );
            }
        }
        items
    };
    deadline.touch();
    if items.is_empty() {
        return Ok(());
    }
    emit_progress(app, json!({ "stage": "repair", "progress": 0, "text": format!("修复依赖库 0/{}", items.len()) }));
    let app2 = app.clone();
    let progress = move |d: usize, t: usize, _speed: f64, _detail: &installer::DownloadDetail| {
        emit_progress(
            &app2,
            json!({
                "stage": "repair",
                "progress": d as f64 / t.max(1) as f64,
                "text": format!("修复依赖库 {}/{}", d, t.max(1))
            }),
        );
    };
    installer::download_all(app, None, items, mirror, &progress)?;
    Ok(())
}

/// 修复客户端 jar（downloads.client 重下）
fn repair_client_jar(
    app: &AppHandle,
    folder: &str,
    base_id: &str,
    jar: &Path,
    mirror: &str,
    deadline: &Deadline,
) -> Result<(), String> {
    let client = installer::read_version_json(app, folder, base_id)
        .ok()
        .and_then(|j| j.get("downloads").and_then(|d| d.get("client")).cloned())
        .unwrap_or(Value::Null);
    let Some(url) = client.get("url").and_then(|v| v.as_str()).map(str::to_string) else {
        return Err(format!("无法自动补全游戏本体：版本 {} 缺少下载地址", base_id));
    };
    let sha1 = client.get("sha1").and_then(|v| v.as_str()).map(str::to_string);
    let size = client.get("size").and_then(|v| v.as_u64());
    let app2 = app.clone();
    let progress = move |done: u64, total: Option<u64>| {
        emit_progress(
            &app2,
            json!({
                "stage": "repair",
                "progress": total.map(|t| done as f64 / t as f64).unwrap_or(0.0),
                "text": "修复游戏本体"
            }),
        );
    };
    if let Some(dir) = jar.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let jar2 = jar.to_path_buf();
    let mirror2 = mirror.to_string();
    installer::run_blocking(move || {
        installer::download_file(app, None, None, &url, &jar2, sha1.as_deref(), size, &mirror2, Some(&progress), &[], &[])
    })?;
    deadline.touch();
    Ok(())
}

fn extract_natives(natives: &[PathBuf], natives_path: &Path) -> Result<(), String> {
    for jar in natives {
        if !jar.exists() {
            continue;
        }
        let file = std::fs::File::open(jar).map_err(|e| e.to_string())?;
        let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
            if entry.is_dir() {
                continue;
            }
            let name = entry.name().to_string();
            if name.starts_with("META-INF/") {
                continue;
            }
            let out_path = natives_path.join(name.replace('/', "\\"));
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut out = std::fs::File::create(&out_path).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

// ---------------- 参数展开 / 替换 ----------------

/// 按空格拆分用户 JVM 参数，支持简单双引号（launch.ts splitArgs）
fn split_args(s: &str) -> Vec<String> {
    if s.trim().is_empty() {
        return Vec::new();
    }
    let re = regex::Regex::new(r#""[^"]*"|[^\s"]+"#).unwrap();
    re.find_iter(s).map(|m| m.as_str().trim_matches('"').to_string()).collect()
}

/// yggdrasilProvider.ts serializeYggdrasilUserProperties
fn serialize_user_properties(properties: Option<&Value>) -> String {
    let mut result = serde_json::Map::new();
    if let Some(list) = properties.and_then(|p| p.as_array()) {
        for p in list {
            let name = p.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let value = p.get("value").and_then(|v| v.as_str()).unwrap_or("");
            if name.is_empty() {
                continue;
            }
            let entry = result
                .entry(name.to_string())
                .or_insert_with(|| Value::Array(Vec::new()));
            if let Some(arr) = entry.as_array_mut() {
                arr.push(json!(value));
            }
        }
    }
    Value::Object(result).to_string()
}

static LAUNCH_VARS: Mutex<Option<Value>> = Mutex::new(None);
static LAUNCH_ACCOUNT: Mutex<Option<Value>> = Mutex::new(None);

fn set_launch_account(account: Value) {
    *LAUNCH_ACCOUNT.lock().unwrap() = Some(account);
}

fn account_value() -> Value {
    LAUNCH_ACCOUNT.lock().unwrap().clone().unwrap_or(Value::Null)
}

/// ${...} 替换（launch.ts sub）：账号变量 + 全局变量表
fn substitute(input: &str) -> String {
    let account = account_value();
    let empty = serde_json::Map::new();
    let vars_guard = LAUNCH_VARS.lock().unwrap();
    let vars = vars_guard.as_ref().and_then(|v| v.as_object()).unwrap_or(&empty);
    let re = regex::Regex::new(r"\$\{([^}]+)\}").unwrap();
    let out = re
        .replace_all(input, |caps: &regex::Captures| {
            let k = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            match k {
                "auth_player_name" => account["username"].as_str().unwrap_or("").to_string(),
                "auth_uuid" => {
                    let uuid = account["uuid"].as_str().unwrap_or("");
                    if account["type"].as_str() == Some("yggdrasil") {
                        uuid.replace('-', "")
                    } else {
                        uuid.to_string()
                    }
                }
                "auth_access_token" | "auth_session" => account["accessToken"].as_str().unwrap_or("").to_string(),
                "user_type" => {
                    if account["type"].as_str() == Some("microsoft") {
                        "msa".to_string()
                    } else {
                        "mojang".to_string()
                    }
                }
                "user_properties" => serialize_user_properties(account.get("userProperties")),
                "version_name" => vars.get("version_name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                _ => vars
                    .get(k)
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
                    .unwrap_or_else(|| caps.get(0).map(|m| m.as_str().to_string()).unwrap_or_default()),
            }
        })
        .to_string();
    drop(vars_guard);
    out
}

/// launch.ts expandEntries：展开新版 arguments 数组（字符串或带 rules 的对象）
fn expand_entries(entries: Option<&Vec<Value>>) -> Vec<String> {
    let mut out = Vec::new();
    let Some(entries) = entries else { return out };
    for e in entries {
        if let Some(s) = e.as_str() {
            out.push(substitute(s));
        } else if e.is_object() && installer::rules_allow(e.get("rules")) {
            let value = e.get("value").cloned().unwrap_or(Value::Null);
            match value {
                Value::Array(list) => {
                    for v in list {
                        if let Some(s) = v.as_str() {
                            out.push(substitute(s));
                        }
                    }
                }
                Value::String(s) => out.push(substitute(&s)),
                _ => {}
            }
        }
    }
    out
}

// ---------------- 窗口尺寸（gameWindow.ts） ----------------

struct WindowArgs {
    args: Vec<String>,
    mode: String,
    width: Option<i64>,
    height: Option<i64>,
}

/// gameWindow.ts resolveGameResolution：实例覆盖全局 + 归一化 + 校验
fn resolve_game_resolution(global: Option<&Value>, instance: Option<&Value>) -> Result<Value, String> {
    let fallback = json!({ "width": GAME_WIDTH_MIN, "height": GAME_HEIGHT_MIN, "mode": "windowed", "fullscreen": false });
    let global_norm = normalize_resolution(global, &fallback);
    let resolved = match instance {
        Some(v) => normalize_resolution(Some(v), &global_norm),
        None => global_norm,
    };
    resolution_validation_error(&resolved)?;
    Ok(resolved)
}

fn normalize_resolution(value: Option<&Value>, fallback: &Value) -> Value {
    let legacy_fullscreen = value
        .map(|v| v.get("fullscreen").and_then(|f| f.as_bool()).unwrap_or(false))
        .unwrap_or(false);
    let raw_mode = value.and_then(|v| v.get("mode")).and_then(|m| m.as_str()).unwrap_or("");
    let mode = if ["windowed", "maximized", "fullscreen"].contains(&raw_mode) {
        raw_mode.to_string()
    } else if legacy_fullscreen {
        "fullscreen".to_string()
    } else {
        fallback["mode"].as_str().unwrap_or("windowed").to_string()
    };
    let width = value
        .and_then(|v| v.get("width"))
        .and_then(|w| w.as_f64())
        .map(|w| w.round() as i64)
        .unwrap_or(fallback["width"].as_i64().unwrap_or(GAME_WIDTH_MIN));
    let height = value
        .and_then(|v| v.get("height"))
        .and_then(|h| h.as_f64())
        .map(|h| h.round() as i64)
        .unwrap_or(fallback["height"].as_i64().unwrap_or(GAME_HEIGHT_MIN));
    json!({ "width": width, "height": height, "mode": mode, "fullscreen": mode == "fullscreen" })
}

fn resolution_validation_error(res: &Value) -> Result<(), String> {
    let mode = res["mode"].as_str().unwrap_or("");
    if !["windowed", "maximized", "fullscreen"].contains(&mode) {
        return Err("请选择有效的窗口模式".into());
    }
    let width = res["width"].as_i64().unwrap_or(0);
    let height = res["height"].as_i64().unwrap_or(0);
    if width < GAME_WIDTH_MIN || width > GAME_WIDTH_MAX {
        return Err(format!("窗口宽度必须是 {}–{} 之间的整数", GAME_WIDTH_MIN, GAME_WIDTH_MAX));
    }
    if height < GAME_HEIGHT_MIN || height > GAME_HEIGHT_MAX {
        return Err(format!("窗口高度必须是 {}–{} 之间的整数", GAME_HEIGHT_MIN, GAME_HEIGHT_MAX));
    }
    Ok(())
}

/// Electron screen.getDisplayNearestPoint().workAreaSize 的 windows-sys 近似：
/// 主显示器工作区 SystemParametersInfoW(SPI_GETWORKAREA)
fn work_area_size() -> Option<(i64, i64)> {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETWORKAREA};
    let mut rect: RECT = unsafe { std::mem::zeroed() };
    unsafe {
        if SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut rect as *mut RECT as *mut core::ffi::c_void, 0) != 0 {
            Some(((rect.right - rect.left) as i64, (rect.bottom - rect.top) as i64))
        } else {
            None
        }
    }
}

/// gameWindow.ts buildGameWindowArguments：先清掉版本元数据自带窗口参数，再写权威值
fn build_game_window_arguments(base: Vec<String>, resolution: &Value, work_area: Option<&(i64, i64)>) -> WindowArgs {
    let mut args = Vec::new();
    let mut i = 0;
    while i < base.len() {
        let value = &base[i];
        if value == "--width" || value == "--height" {
            i += 2;
            continue;
        }
        if value == "--fullscreen" {
            i += 1;
            continue;
        }
        args.push(value.clone());
        i += 1;
    }
    if resolution["mode"].as_str() == Some("fullscreen") {
        args.push("--fullscreen".into());
        return WindowArgs { args, mode: "fullscreen".into(), width: None, height: None };
    }
    let use_work_area = resolution["mode"].as_str() == Some("maximized")
        && work_area.map(|(w, h)| *w > 0 && *h > 0).unwrap_or(false);
    let width = if use_work_area {
        work_area.unwrap().0
    } else {
        resolution["width"].as_i64().unwrap_or(GAME_WIDTH_MIN)
    };
    let height = if use_work_area {
        work_area.unwrap().1
    } else {
        resolution["height"].as_i64().unwrap_or(GAME_HEIGHT_MIN)
    };
    args.push("--width".into());
    args.push(width.to_string());
    args.push("--height".into());
    args.push(height.to_string());
    WindowArgs {
        args,
        mode: resolution["mode"].as_str().unwrap_or("windowed").to_string(),
        width: Some(width),
        height: Some(height),
    }
}

/// 保存窗口化尺寸（rememberGameWindowSize；实例覆盖优先于全局设置）
fn commit_window_size(app: &AppHandle, folder: &str, version_id: &str, width: i64, height: i64) -> Result<(), String> {
    let normalized = json!({ "width": width, "height": height, "mode": "windowed", "fullscreen": false });
    match installer::read_version_json(app, folder, version_id) {
        Ok(mut j) if j.get("_resolution").is_some() => {
            j["_resolution"] = normalized;
            installer::write_version_json(&installer::version_json_path_in(folder, version_id), &j)
        }
        _ => settings::save(app, json!({ "resolution": normalized })).map(|_| ()),
    }
}

/// gameWindowSize.ts readOwnedGameWindow：user32 只读观察（可见/非最小化/非最大化/
/// GLFW|LWJGL 类名/带 WS_THICKFRAME 的窗口客户区）；命中不唯一时放弃
fn read_owned_game_window(pid: u32) -> Option<(i64, i64)> {
    use windows_sys::Win32::Foundation::{HWND, LPARAM, RECT};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetClientRect, GetWindowLongW, GetWindowThreadProcessId,
        IsIconic, IsWindowVisible, IsZoomed, GWL_STYLE,
    };
    const WS_THICKFRAME: i32 = 0x0004_0000;

    struct Ctx {
        pid: u32,
        matches: Vec<(i64, i64)>,
    }
    unsafe extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> i32 {
        let ctx = &mut *(lparam as *mut Ctx);
        let mut out_pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, &mut out_pid);
        if out_pid != ctx.pid || IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 || IsZoomed(hwnd) != 0 {
            return 1;
        }
        let mut name = [0u16; 256];
        let n = GetClassNameW(hwnd, name.as_mut_ptr(), 256);
        let class_name = String::from_utf16_lossy(&name[..n.max(0) as usize]);
        let upper = class_name.to_uppercase();
        if !upper.contains("GLFW") && !upper.contains("LWJGL") {
            return 1;
        }
        if GetWindowLongW(hwnd, GWL_STYLE) & WS_THICKFRAME == 0 {
            return 1;
        }
        let mut rect: RECT = std::mem::zeroed();
        if GetClientRect(hwnd, &mut rect) == 0 {
            return 1;
        }
        let mut out_pid2: u32 = 0;
        GetWindowThreadProcessId(hwnd, &mut out_pid2);
        if out_pid2 == ctx.pid {
            ctx.matches.push(((rect.right - rect.left) as i64, (rect.bottom - rect.top) as i64));
        }
        1
    }

    let mut ctx = Ctx { pid, matches: Vec::new() };
    unsafe {
        EnumWindows(Some(callback), &mut ctx as *mut Ctx as LPARAM);
    }
    if ctx.matches.len() == 1 {
        ctx.matches.pop()
    } else {
        None
    }
}

// ---------------- 命令世界（commandWorld.ts） ----------------

/// 最小 NBT 写入器（level.dat 用；TAG_Byte/Int/Long/String/List/Compound）
mod nbt {
    pub enum Tag {
        Byte(i8),
        Int(i32),
        Long(i64),
        Str(String),
        List(u8, Vec<Tag>),
        Compound(Vec<(String, Tag)>),
    }

    fn put_str(out: &mut Vec<u8>, s: &str) {
        let bytes = s.encode_utf16().collect::<Vec<_>>();
        out.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
        for u in bytes {
            out.extend_from_slice(&u.to_be_bytes());
        }
    }

    fn tag_type(tag: &Tag) -> u8 {
        match tag {
            Tag::Byte(_) => 1,
            Tag::Int(_) => 3,
            Tag::Long(_) => 4,
            Tag::Str(_) => 8,
            Tag::List(..) => 9,
            Tag::Compound(_) => 10,
        }
    }

    fn put_payload(out: &mut Vec<u8>, tag: &Tag) {
        match tag {
            Tag::Byte(b) => out.push(*b as u8),
            Tag::Int(v) => out.extend_from_slice(&v.to_be_bytes()),
            Tag::Long(v) => out.extend_from_slice(&v.to_be_bytes()),
            Tag::Str(s) => put_str(out, s),
            Tag::List(inner_type, items) => {
                out.push(*inner_type);
                out.extend_from_slice(&(items.len() as i32).to_be_bytes());
                for item in items {
                    put_payload(out, item);
                }
            }
            Tag::Compound(fields) => {
                for (name, child) in fields {
                    out.push(tag_type(child));
                    put_str(out, name);
                    put_payload(out, child);
                }
                out.push(0);
            }
        }
    }

    /// 序列化为未压缩 NBT（网络序，根 Compound 名为空），调用方自行 gzip
    pub fn encode(root: &Tag) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(10);
        put_str(&mut out, "");
        put_payload(&mut out, root);
        out
    }
}

fn world_generation(seed: i64, split_format: bool) -> nbt::Tag {
    let noise = |dimension: &str, biome: &str| {
        nbt::Tag::Compound(vec![
            ("type".into(), nbt::Tag::Str(format!("minecraft:{}", dimension))),
            (
                "generator".into(),
                nbt::Tag::Compound(vec![
                    ("type".into(), nbt::Tag::Str("minecraft:noise".into())),
                    ("settings".into(), nbt::Tag::Str(format!("minecraft:{}", biome))),
                    (
                        "biome_source".into(),
                        if biome == "end" {
                            nbt::Tag::Compound(vec![("type".into(), nbt::Tag::Str("minecraft:the_end".into()))])
                        } else {
                            nbt::Tag::Compound(vec![
                                ("type".into(), nbt::Tag::Str("minecraft:multi_noise".into())),
                                ("preset".into(), nbt::Tag::Str(format!("minecraft:{}", biome))),
                            ])
                        },
                    ),
                ]),
            ),
        ])
    };
    nbt::Tag::Compound(vec![
        ("seed".into(), nbt::Tag::Long(seed)),
        (
            if split_format { "generate_structures".into() } else { "generate_features".into() },
            nbt::Tag::Byte(1),
        ),
        ("bonus_chest".into(), nbt::Tag::Byte(0)),
        (
            "dimensions".into(),
            nbt::Tag::Compound(vec![
                ("minecraft:overworld".into(), noise("overworld", "overworld")),
                ("minecraft:the_nether".into(), noise("the_nether", "nether")),
                ("minecraft:the_end".into(), noise("the_end", "end")),
            ]),
        ),
    ])
}

fn rand_bytes(n: usize) -> Vec<u8> {
    use rand::RngCore;
    let mut buf = vec![0u8; n];
    rand::rng().fill_bytes(&mut buf);
    buf
}

/// commandWorld.ts createCommandWorld：只新建世界（游戏自己生成地形并应用实例模组）
fn create_command_world(game_directory: &str, client_jar: &Path) -> Result<(String, String), String> {
    let file = std::fs::File::open(client_jar).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let version: Value = {
        let mut entry = zip
            .by_name("version.json")
            .map_err(|_| "客户端缺少 version.json，未创建存档".to_string())?;
        let mut text = String::new();
        entry.read_to_string(&mut text).map_err(|e| e.to_string())?;
        serde_json::from_str(&text).map_err(|_| "客户端 version.json 不是有效 JSON".to_string())?
    };
    let version_id = version.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if !supports_quick_play_multiplayer(&version_id) {
        return Err("此 Minecraft 版本不支持官方单人 Quick Play（需要 1.20+）；请在游戏内新建世界并开启命令".into());
    }
    let world_version = match version.get("world_version").and_then(|v| v.as_i64()) {
        Some(v) if v >= 0 => v,
        _ => return Err("客户端缺少真实世界数据版本，未创建存档".into()),
    };
    let stable = version.get("stable").and_then(|v| v.as_bool()).unwrap_or(true);
    let series = version.get("series_id").and_then(|v| v.as_str()).unwrap_or("main").to_string();
    let id = format!(
        "WUHUICL-Test-{}-{}",
        chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true).replace([':', '.'], "-"),
        hex::encode(rand_bytes(3))
    );
    let destination = Path::new(game_directory).join("saves").join(&id);
    std::fs::create_dir_all(destination.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::create_dir(&destination).map_err(|e| format!("世界目录已存在：{}", e))?; // exclusive

    // 26.1 snapshot 6+ 把世界生成挪进 namespaced SavedData：按客户端字节码探测
    let world_gen_class = zip
        .by_name("net/minecraft/world/level/levelgen/WorldGenSettings.class")
        .ok()
        .map(|mut e| {
            let mut buf = Vec::new();
            let _ = e.read_to_end(&mut buf);
            buf
        });
    let split_format = world_gen_class
        .map(|bytes| bytes.windows(13).any(|w| w == b"SavedDataType"))
        .unwrap_or(false);
    let seed = i64::from_be_bytes(rand_bytes(8).try_into().unwrap());
    let level_name = format!("命令测试 {}", chrono::Local::now().format("%Y/%m/%d %H:%M:%S"));
    let now_ms = chrono::Utc::now().timestamp_millis();
    let mut data_fields = vec![
        ("DataVersion".to_string(), nbt::Tag::Int(world_version as i32)),
        (
            "Version".to_string(),
            nbt::Tag::Compound(vec![
                ("Name".into(), nbt::Tag::Str(version_id.clone())),
                ("Id".into(), nbt::Tag::Int(world_version as i32)),
                ("Snapshot".into(), nbt::Tag::Int(if stable { 0 } else { 1 })),
                ("Series".into(), nbt::Tag::Str(series)),
            ]),
        ),
        ("version".into(), nbt::Tag::Int(19133)),
        ("LevelName".into(), nbt::Tag::Str(level_name)),
        ("GameType".into(), nbt::Tag::Int(1)),
        ("allowCommands".into(), nbt::Tag::Byte(1)),
        ("hardcore".into(), nbt::Tag::Byte(0)),
        (
            if split_format { "difficulty_settings".into() } else { "Difficulty".into() },
            if split_format {
                nbt::Tag::Compound(vec![
                    ("difficulty".into(), nbt::Tag::Str("easy".into())),
                    ("locked".into(), nbt::Tag::Byte(0)),
                ])
            } else {
                nbt::Tag::Byte(1)
            },
        ),
        ("initialized".into(), nbt::Tag::Byte(0)),
        ("LastPlayed".into(), nbt::Tag::Long(now_ms)),
        ("Time".into(), nbt::Tag::Long(0)),
        ("DayTime".into(), nbt::Tag::Long(0)),
        (
            "DataPacks".into(),
            nbt::Tag::Compound(vec![
                ("Enabled".into(), nbt::Tag::List(8, vec![nbt::Tag::Str("vanilla".into())])),
                ("Disabled".into(), nbt::Tag::List(8, Vec::new())),
            ]),
        ),
    ];
    if split_format {
        data_fields.push(("WorldGenSettings".into(), world_generation(seed, true)));
    } else {
        data_fields.push(("WorldGenSettings".into(), world_generation(seed, false)));
        data_fields.push((
            "DragonFight".into(),
            nbt::Tag::Compound(vec![
                ("NeedsStateScanning".into(), nbt::Tag::Byte(1)),
                ("DragonKilled".into(), nbt::Tag::Byte(0)),
                ("PreviouslyKilled".into(), nbt::Tag::Byte(0)),
            ]),
        ));
    }
    let root = nbt::Tag::Compound(vec![("Data".into(), nbt::Tag::Compound(data_fields))]);
    std::fs::write(destination.join("level.dat"), gzip_bytes(&nbt::encode(&root))).map_err(|e| e.to_string())?;
    if split_format {
        let data_dir = destination.join("data").join("minecraft");
        std::fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;
        let saved = nbt::Tag::Compound(vec![
            ("DataVersion".into(), nbt::Tag::Int(world_version as i32)),
            ("data".into(), world_generation(seed, true)),
        ]);
        std::fs::write(data_dir.join("world_gen_settings.dat"), gzip_bytes(&nbt::encode(&saved)))
            .map_err(|e| e.to_string())?;
    }
    Ok((id, destination.to_string_lossy().to_string()))
}

fn gzip_bytes(data: &[u8]) -> Vec<u8> {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    let _ = encoder.write_all(data);
    encoder.finish().unwrap_or_default()
}

/// serverUtils.ts supportsQuickPlayMultiplayer（1.20+ / 23w14a+）
fn supports_quick_play_multiplayer(version: &str) -> bool {
    let v = version.trim();
    if let Some(caps) = regex::Regex::new(r"^(\d+)\.(\d+)(?:\.(\d+))?(?:$|[-+])").unwrap().captures(v) {
        let major: i64 = caps.get(1).unwrap().as_str().parse().unwrap_or(0);
        let minor: i64 = caps.get(2).unwrap().as_str().parse().unwrap_or(0);
        return major > 1 || (major == 1 && minor >= 20);
    }
    if let Some(caps) = regex::Regex::new(r"^(\d{2})w(\d{2})([a-z])$").unwrap().captures(&v.to_lowercase()) {
        let year: i64 = caps.get(1).unwrap().as_str().parse().unwrap_or(0);
        let week: i64 = caps.get(2).unwrap().as_str().parse().unwrap_or(0);
        let revision = caps.get(3).unwrap().as_str();
        return year > 23 || (year == 23 && (week > 14 || (week == 14 && revision >= "a")));
    }
    false
}

/// instanceMetadata.ts isMinecraftVersionId
fn is_minecraft_version_id(value: &str) -> bool {
    if value.trim().is_empty() || value.trim() != value || value == "0.0.0" {
        return false;
    }
    regex::Regex::new(
        r"^(?:[1-9]\d*\.\d+(?:\.\d+)?(?:-(?:pre|rc)-?\d+|-snapshot-\d+| Pre-Release \d+)?|\d{2}w\d{2}[a-z](?:[\w.-]*)|[ab]\d+(?:\.\d+)+[a-z]?(?:_\d+|-pre\d+)?|c\d+(?:\.\d+)+[a-z]?(?:_[\w-]+)?|rd-\d+|inf-\d+|1\.RV-Pre1|3D Shareware v1\.34)$",
    )
    .unwrap()
    .is_match(value)
}

fn host_arch() -> String {
    if cfg!(target_arch = "aarch64") {
        "arm64".into()
    } else {
        "x64".into()
    }
}

fn total_mem_mb() -> u64 {
    crate::sysinfo::system_info()["totalMemMB"].as_u64().unwrap_or(4096)
}

/// shared/memory.ts autoMemoryMB：物理内存 25% 取 0.5GB 整，2GB~8GB 钳制
fn auto_memory_mb(total_mem_mb: u64) -> u64 {
    let quarter = ((total_mem_mb as f64 * 0.25) as u64 / 512) * 512;
    quarter.clamp(2048, 8192)
}

// ---------------- 工具 ----------------

fn spawn_reader<R: std::io::Read + Send + 'static>(stream: Option<R>, sink: impl Fn(String) + Send + 'static) {
    let Some(stream) = stream else { return };
    std::thread::spawn(move || {
        let reader = BufReader::new(stream);
        for line in reader.split(b'\n') {
            match line {
                Ok(bytes) => {
                    let mut text = String::from_utf8_lossy(&bytes).to_string();
                    if text.ends_with('\r') {
                        text.pop();
                    }
                    if !text.is_empty() {
                        sink(text);
                    }
                }
                Err(_) => break,
            }
        }
    });
}

/// instances.ts instanceDirectoryState 的 path 取值
fn effective_game_dir(app: &AppHandle, folder: &str, version_id: &str) -> Result<String, String> {
    let j = installer::read_version_json(app, folder, version_id)?;
    let state = installer::instance_directory_state(app, folder, version_id, &j);
    Ok(state["path"].as_str().unwrap_or(folder).to_string())
}

fn persist_running_game(app: &AppHandle, record: &Value) {
    let file = settings::user_data_dir(app).join("running-game.json");
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string_pretty(record) {
        let _ = std::fs::write(file, text);
    }
}

/// gracefulClose.ts windowsQuote：Windows 命令行引号规则（命令摘要用）
fn windows_quote(arg: &str) -> String {
    if !arg.is_empty() && !arg.chars().any(|c| c.is_whitespace() || c == '"') {
        return arg.to_string();
    }
    let mut out = String::from("\"");
    let mut backslashes = 0usize;
    for ch in arg.chars() {
        if ch == '\\' {
            backslashes += 1;
            continue;
        }
        if ch == '"' {
            out.push_str(&"\\".repeat(backslashes * 2 + 1));
            out.push('"');
        } else {
            out.push_str(&"\\".repeat(backslashes));
            out.push(ch);
        }
        backslashes = 0;
    }
    out.push_str(&"\\".repeat(backslashes * 2));
    out.push('"');
    out
}

// ---------------- kill / restart（gameSession.ts 语义） ----------------

/// gracefulClose.ts requestGameWindowClose：PowerShell 给主窗口发 WM_CLOSE（绝不强杀）
fn request_game_window_close(pid: u32) {
    let script = format!(
        "$ErrorActionPreference='Stop'; $gameProcess=[System.Diagnostics.Process]::GetProcessById({}); if (-not $gameProcess.CloseMainWindow()) {{ throw 'Minecraft has no responsive main window; exit from inside the game.' }}",
        pid
    );
    let mut cmd = std::process::Command::new("powershell.exe");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let _ = cmd.spawn();
}

fn child_exited(proc: &Arc<ChildProc>) -> bool {
    if proc.exit_code.lock().unwrap().is_some() {
        return true;
    }
    proc.child
        .lock()
        .unwrap()
        .try_wait()
        .map(|s| s.is_some())
        .unwrap_or(false)
}

fn hard_stop(proc: &Arc<ChildProc>, timeout_ms: u64) -> Result<(), String> {
    {
        let mut guard = proc.child.lock().unwrap();
        if guard.try_wait().map(|s| s.is_some()).unwrap_or(false) {
            return Ok(());
        }
        // Windows kill = TerminateProcess(1)，与 gracefulClose.ts DetachedGameProcess.kill 一致
        if guard.kill().is_err() {
            return Err("系统未接受结束请求，游戏状态保持运行".into());
        }
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    while std::time::Instant::now() < deadline {
        if child_exited(proc) {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(120));
    }
    Err("游戏进程尚未确认退出；请在游戏内退出或检查进程状态".into())
}

/// game:kill（gameSession.requestStop）：无 token 先正常关窗等 30s；超时返回 forceToken
fn kill_game(args: &[Value]) -> Result<Value, String> {
    let force_token = arg_str(args, 0);
    if with_globals(|gg| gg.restart_pending.lock().unwrap().is_some()) {
        return Err("正在处理重启，请先完成或取消重启请求".into());
    }
    let entry = with_globals(|gg| {
        gg.sessions
            .lock()
            .unwrap()
            .iter()
            .filter(|s| !s.exited.load(Ordering::SeqCst))
            .rev()
            .find(|s| s.child.lock().unwrap().is_some())
            .cloned()
    })
    .ok_or("没有正在运行的游戏，或游戏仍在准备启动")?;
    if entry.stopping.load(Ordering::SeqCst) {
        return Err("正在等待游戏退出，请稍候".into());
    }
    if let Some(token) = &force_token {
        let approved = entry.stop_approval.lock().unwrap().clone();
        if approved.as_deref() != Some(token.as_str()) {
            return Err("强制结束确认已失效，未结束任何进程".into());
        }
        entry.stop_approval.lock().unwrap().take();
        entry.intentional_stop.store(true, Ordering::SeqCst);
        let proc = entry.child.lock().unwrap().clone();
        if let Some(proc) = proc {
            hard_stop(&proc, FORCE_STOP_MS)?;
        }
        return Ok(json!({ "requiresForce": false }));
    }
    // 正常停止：发送关窗请求 + 等待正常退出（stopGracefully；关窗失败不影响等待）
    entry.stopping.store(true, Ordering::SeqCst);
    let proc = entry.child.lock().unwrap().clone();
    let result = (|| -> Result<Value, String> {
        let Some(proc) = proc else { return Err("游戏仍在准备启动，请稍后重试".into()) };
        request_game_window_close(proc.pid);
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(GRACEFUL_STOP_MS);
        while std::time::Instant::now() < deadline {
            if child_exited(&proc) {
                return Ok(json!({ "requiresForce": false }));
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        let token = uuid_v4();
        *entry.stop_approval.lock().unwrap() = Some(token.clone());
        entry.stopping.store(false, Ordering::SeqCst);
        Ok(json!({ "requiresForce": true, "forceToken": token }))
    })();
    if result.is_err() {
        entry.stopping.store(false, Ordering::SeqCst);
    }
    result
}

/// game:restart（launch.ts restartGame）：先正常退出（可带 forceToken 强杀确认），再复用调用上下文重启
fn restart_game(app: &AppHandle, args: &[Value]) -> Result<Value, String> {
    let version_id = arg_str(args, 0).ok_or("缺少参数")?;
    let folder = arg_str(args, 1).ok_or("缺少参数")?;
    let force_token = arg_str(args, 2);

    let target_token = token_of(&version_id);
    let invocation = LAST_INVOCATION.lock().unwrap().clone();
    if let Some(token) = &force_token {
        let ok = with_globals(|gg| {
            let mut pending = gg.restart_pending.lock().unwrap();
            match pending.as_mut() {
                Some(p)
                    if !p.waiting
                        && p.force_token.as_deref() == Some(token.as_str())
                        && p.invocation.version_id == version_id
                        && installer::same_path(&p.invocation.folder, &folder) =>
                {
                    p.waiting = true;
                    true
                }
                _ => false,
            }
        });
        if !ok {
            return Err("重启确认已失效，请重新请求".into());
        }
        if let Some(entry) = target_token.as_deref().and_then(find_session) {
            entry.intentional_stop.store(true, Ordering::SeqCst);
            let proc = entry.child.lock().unwrap().clone();
            if let Some(proc) = proc {
                if hard_stop(&proc, FORCE_STOP_MS).is_err() {
                    with_globals(|gg| *gg.restart_pending.lock().unwrap() = None);
                    return Err(format!("重启确认后强制停止游戏失败：{}", version_id));
                }
            }
        }
    } else {
        if with_globals(|gg| gg.restart_pending.lock().unwrap().is_some()) {
            return Err("已有重启请求正在处理".into());
        }
        let matches = match (&invocation, &target_token) {
            (Some(inv), Some(_)) => inv.version_id == version_id && installer::same_path(&inv.folder, &folder),
            _ => false,
        };
        if !matches {
            return Err("此实例当前未运行；请选择启动实例".into());
        }
        // 正常退出等待（30s）；超时给出强杀确认
        if let Some(entry) = target_token.clone().and_then(|t| find_session(&t)) {
            let proc = entry.child.lock().unwrap().clone();
            if let Some(proc) = proc {
                request_game_window_close(proc.pid);
                let deadline = std::time::Instant::now() + std::time::Duration::from_millis(GRACEFUL_STOP_MS);
                let mut exited = false;
                while std::time::Instant::now() < deadline {
                    if child_exited(&proc) {
                        exited = true;
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
                if !exited {
                    let token = uuid_v4();
                    with_globals(|gg| {
                        *gg.restart_pending.lock().unwrap() = Some(RestartPending {
                            invocation: invocation.clone().unwrap(),
                            force_token: Some(token.clone()),
                            waiting: false,
                        });
                    });
                    return Ok(json!({ "requiresForce": true, "forceToken": token }));
                }
            }
        }
    }

    let Some(inv) = invocation else {
        with_globals(|gg| *gg.restart_pending.lock().unwrap() = None);
        return Err("此实例当前未运行；请选择启动实例".into());
    };
    // 重新启动同一实例（launch.ts restartGame 的 launchOwned 复用；await 语义）
    send_state(
        app,
        json!({
            "status": "launching",
            "text": "游戏已确认退出，正在重新启动同一实例…",
            "versionId": inv.version_id,
            "folder": inv.folder,
            "launchId": inv.launch_id
        }),
    );
    *RESTARTING.lock().unwrap() = Some(inv.launch_id.clone());
    // launch_pipeline 的 restart_pending 守卫只针对并发启动；重启自身先清场
    with_globals(|gg| *gg.restart_pending.lock().unwrap() = None);
    let app2 = app.clone();
    let inv2 = inv.clone();
    let relaunch_result = launch_pipeline(&app2, &inv2);
    if let Err(e) = &relaunch_result {
        send_state(
            app,
            json!({
                "status": "error",
                "text": e,
                "versionId": inv.version_id,
                "folder": inv.folder,
                "launchId": inv.launch_id
            }),
        );
    }
    with_globals(|gg| *gg.restart_pending.lock().unwrap() = None);
    relaunch_result.map(|_| json!({ "requiresForce": false }))
}

/// game:restartCancel（launch.ts cancelRestart）
fn cancel_restart() -> Result<Value, String> {
    with_globals(|gg| {
        let mut pending = gg.restart_pending.lock().unwrap();
        if pending.as_ref().map(|p| p.waiting).unwrap_or(false) {
            return Err("仍在等待游戏正常退出，请稍后取消".into());
        }
        *pending = None;
        Ok(Value::Null)
    })
}
