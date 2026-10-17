//! 桥接 MOD 实时配置面板（对应原版 core/modBridge.ts 全文 + ipc.ts:962-971 的 bridge:* 六通道）。
//! 仅连接 127.0.0.1；token 来自游戏目录发现文件（本机可读即授权）。
//! 任何失败都以错误/断开状态返回，绝不影响游戏进程。

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::Manager;

const BUNDLED_BRIDGE_JAR: &[u8] = include_bytes!("../resources/kamucl-bridge.jar");

struct Discovery {
    protocol: u64,
    port: u64,
    token: String,
    mod_version: String,
    pid: f64,
}

fn arg_str(args: &[Value], i: usize) -> String {
    js_string(args.get(i))
}

fn js_string(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => n.to_string(),
        Some(other) => other.to_string(),
    }
}

fn js_number_or_zero(v: Option<&Value>) -> f64 {
    match v {
        None | Some(Value::Null) => 0.0,
        Some(Value::Bool(b)) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(s)) => {
            let t = s.trim();
            if t.is_empty() {
                0.0
            } else {
                t.parse::<f64>().unwrap_or(f64::NAN)
            }
        }
        Some(other) => other.to_string().parse::<f64>().unwrap_or(f64::NAN),
    }
}

fn js_truthy_id(v: Option<&Value>) -> Option<String> {
    match v {
        None | Some(Value::Null) | Some(Value::Bool(false)) => None,
        Some(Value::Bool(true)) => Some("true".into()),
        Some(Value::Number(n)) => {
            if n.as_f64() == Some(0.0) {
                None
            } else {
                Some(n.to_string())
            }
        }
        Some(Value::String(s)) => {
            if s.is_empty() {
                None
            } else {
                Some(s.clone())
            }
        }
        Some(Value::Array(a)) => {
            if a.is_empty() {
                None
            } else {
                Some(js_string(v))
            }
        }
        Some(other) => Some(other.to_string()),
    }
}

/// 对应原版 gameDirOf()：instanceDirectoryState(versionId, readVersionJson(versionId)).path
fn game_dir_of(app: &tauri::AppHandle, version_id: &str) -> Result<PathBuf, String> {
    let folder = crate::installer::folder_of_version(app, version_id);
    let json = crate::installer::read_version_json(app, &folder, version_id)?;
    let state = crate::installer::instance_directory_state(app, &folder, version_id, &json);
    let path = state["path"].as_str().unwrap_or("").to_string();
    if path.is_empty() {
        return Err("实例目录不存在".into());
    }
    Ok(PathBuf::from(path))
}

/// 对应原版 readDiscovery()：发现文件不存在/损坏一律返回 null（gameDirOf 的抛错也被吞掉）
fn read_discovery(app: &tauri::AppHandle, version_id: &str) -> Option<Discovery> {
    let file = game_dir_of(app, version_id)
        .ok()?
        .join(".kamucl-bridge.json");
    let raw = std::fs::read_to_string(&file).ok()?;
    let parsed: Value = serde_json::from_str(&raw).ok()?;
    if parsed["protocol"].as_f64() != Some(1.0) {
        return None;
    }
    let port = js_number_or_zero(parsed.get("port"));
    if !port.is_finite() || port.fract() != 0.0 || port <= 0.0 || port > 65535.0 {
        return None;
    }
    let token = parsed.get("token").and_then(|t| t.as_str())?;
    if token.chars().count() < 16 {
        return None;
    }
    Some(Discovery {
        protocol: 1,
        port: port as u64,
        token: token.to_string(),
        mod_version: js_string(parsed.get("modVersion")),
        pid: js_number_or_zero(parsed.get("pid")),
    })
}

/// 对应原版 process.kill(pid, 0)：进程不存在即抛错 → 视为已退出
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
        ok && code == STILL_ACTIVE
    }
}

/// 对应原版 bridgeStatus()
fn bridge_status(app: &tauri::AppHandle, version_id: &str) -> Result<Value, String> {
    let Some(discovery) = read_discovery(app, version_id) else {
        return Ok(json!({
            "connected": false,
            "reason": "未发现桥接服务：请使用内置 KAMUCL Bridge 的实例启动游戏"
        }));
    };
    if discovery.pid as u32 == 0 || !process_alive(discovery.pid as u32) {
        return Ok(json!({
            "connected": false,
            "reason": "检测到上次的桥接记录，但游戏已退出"
        }));
    }
    match call(&discovery, "ping", None, 1500) {
        Ok(pong) if pong["ok"] == json!(true) => Ok(json!({
            "connected": true,
            "modVersion": discovery.mod_version,
            "protocol": discovery.protocol
        })),
        Ok(_) => Ok(json!({ "connected": false, "reason": "桥接服务响应异常" })),
        Err(_) => Ok(json!({
            "connected": false,
            "reason": "桥接服务无响应（游戏可能正在加载或已退出）"
        })),
    }
}

/// 对应原版 bridgeInstalled()：复用 mods.rs parse_mod_file（原版 parseModFile）按 fabric mod id 识别
fn bridge_installed(app: &tauri::AppHandle, version_id: &str) -> Result<bool, String> {
    let mods_dir = game_dir_of(app, version_id)?.join("mods");
    let Ok(entries) = std::fs::read_dir(&mods_dir) else {
        return Ok(false);
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.to_lowercase().ends_with(".jar") {
            continue;
        }
        let info = crate::mods::parse_mod_file(&entry.path().to_string_lossy());
        if info["id"].as_str() == Some("kamucl-bridge") {
            return Ok(true);
        }
    }
    Ok(false)
}

/// 对应原版 call()：GET 无鉴权头，POST 带 Content-Type 与 X-Kamucl-Token
fn call(
    discovery: &Discovery,
    pathname: &str,
    body: Option<Value>,
    timeout_ms: u64,
) -> Result<Value, String> {
    let url = format!("http://127.0.0.1:{}/kamucl/v1/{}", discovery.port, pathname);
    let timeout = Duration::from_millis(timeout_ms);
    let send = match body {
        None => crate::net::client().get(url).timeout(timeout).send(),
        Some(payload) => crate::net::client()
            .post(url)
            .timeout(timeout)
            .header("Content-Type", "application/json")
            .header("X-Kamucl-Token", discovery.token.as_str())
            .body(payload.to_string())
            .send(),
    }
    .map_err(|e| e.to_string())?;
    let status = send.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(format!("HTTP {}", status));
    }
    send.json::<Value>().map_err(|e| e.to_string())
}

fn bridge_manifest(app: &tauri::AppHandle, version_id: &str) -> Result<Value, String> {
    let Some(discovery) = read_discovery(app, version_id) else {
        return Err("未发现桥接服务".into());
    };
    let manifest = call(&discovery, "manifest", None, 3000)?;
    if manifest["protocol"].as_f64() != Some(1.0) || !manifest["params"].is_array() {
        return Err("桥接服务协议不兼容".into());
    }
    Ok(json!({ "protocol": 1, "params": manifest["params"] }))
}

fn bridge_set(
    app: &tauri::AppHandle,
    version_id: &str,
    id: &str,
    value: Option<&Value>,
) -> Result<Value, String> {
    let Some(discovery) = read_discovery(app, version_id) else {
        return Ok(json!({ "ok": false, "error": "未发现桥接服务：游戏未运行或未安装桥接 MOD" }));
    };
    let mut body = serde_json::Map::new();
    body.insert("id".into(), json!(id));
    if let Some(v) = value {
        body.insert("value".into(), v.clone());
    }
    Ok(call(&discovery, "set", Some(Value::Object(body)), 3000)
        .unwrap_or_else(|e| json!({ "ok": false, "error": e })))
}

fn bridge_reset(
    app: &tauri::AppHandle,
    version_id: &str,
    id: Option<String>,
) -> Result<Value, String> {
    let Some(discovery) = read_discovery(app, version_id) else {
        return Ok(json!({ "ok": false, "error": "未发现桥接服务" }));
    };
    let body = match &id {
        Some(v) => json!({ "id": v }),
        None => json!({}),
    };
    Ok(call(&discovery, "reset", Some(body), 3000)
        .unwrap_or_else(|e| json!({ "ok": false, "error": e })))
}

/// 对应原版 bundledBridgeJar() + installBridge()：幂等装入 mods/kamucl-bridge-1.0.1.jar
fn install_bridge(app: &tauri::AppHandle, version_id: &str) -> Value {
    match bridge_installed(app, version_id) {
        Ok(true) => return json!({ "ok": true, "already": true }),
        Err(e) => return json!({ "ok": false, "error": e }),
        Ok(false) => {}
    }
    let Some(bytes) = bundled_bridge_bytes(app) else {
        return json!({ "ok": false, "error": "内置桥接 MOD 文件缺失，请重新安装启动器" });
    };
    let mods_dir = match game_dir_of(app, version_id) {
        Ok(dir) => dir.join("mods"),
        Err(e) => return json!({ "ok": false, "error": e }),
    };
    if let Err(e) = std::fs::create_dir_all(&mods_dir) {
        return json!({ "ok": false, "error": e.to_string() });
    }
    match std::fs::write(mods_dir.join("kamucl-bridge-1.0.1.jar"), &bytes) {
        Ok(_) => json!({ "ok": true }),
        Err(e) => json!({ "ok": false, "error": e.to_string() }),
    }
}

fn bundled_bridge_bytes(app: &tauri::AppHandle) -> Option<Vec<u8>> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(dir) = app.path().resource_dir() {
        candidates.push(dir.join("resources").join("kamucl-bridge.jar"));
        candidates.push(dir.join("kamucl-bridge.jar"));
    }
    if let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
    {
        candidates.push(exe_dir.join("resources").join("kamucl-bridge.jar"));
        candidates.push(exe_dir.join("kamucl-bridge.jar"));
    }
    candidates.push(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("kamucl-bridge.jar"),
    );
    for p in &candidates {
        if p.is_file() {
            let bytes = std::fs::read(p).ok()?;
            return if bytes.is_empty() { None } else { Some(bytes) };
        }
    }
    if BUNDLED_BRIDGE_JAR.is_empty() {
        None
    } else {
        Some(BUNDLED_BRIDGE_JAR.to_vec())
    }
}

pub fn invoke(
    app: &tauri::AppHandle,
    channel: &str,
    args: &[Value],
) -> Option<Result<Value, String>> {
    match channel {
        "bridge:status" => Some(bridge_status(app, &arg_str(args, 0))),
        "bridge:manifest" => Some(bridge_manifest(app, &arg_str(args, 0))),
        "bridge:set" => {
            let version_id = arg_str(args, 0);
            let id = arg_str(args, 1);
            Some(bridge_set(app, &version_id, &id, args.get(2)))
        }
        "bridge:reset" => {
            let version_id = arg_str(args, 0);
            let id = js_truthy_id(args.get(2));
            Some(bridge_reset(app, &version_id, id))
        }
        "bridge:installed" => Some(bridge_installed(app, &arg_str(args, 0)).map(Value::from)),
        "bridge:install" => Some(Ok(install_bridge(app, &arg_str(args, 0)))),
        _ => None,
    }
}
