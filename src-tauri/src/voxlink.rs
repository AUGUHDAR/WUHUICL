//! voxlink 联机状态层（对应原版 src/main/core/voxlink/ 的 index.ts / api.ts / settings.ts
//! 以及 engine.ts 的状态部分）：空闲快照、设置读写、房间列表 HTTP 查询、空闲态门禁分支。
//! 引擎本体（会话/WebSocket、RUDP、打洞、TURN、modsync、工单）未移植，涉及真联网与 P2P
//! 的通道只保留原版「网络之前」的校验，校验通过后返回未移植错误，不伪造成功。
//! 通道：voxlink:status / voxlink:settings / voxlink:lobby / voxlink:stop /
//! voxlink:tryDirect / voxlink:usePlayerRelay / voxlink:useTurnRelay / voxlink:start；
//! 事件：voxlink:event {type,data}（与原版 index.ts push() 同形状）。

use serde_json::{json, Value};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

// 对应原版 api.ts DEFAULT_SERVER_URL / HTTP_TIMEOUT_MS / MAX_RESPONSE_LEN；
// 两个 static 对应 index.ts 的 requestPending 与 VoxlinkApp.settings
const DEFAULT_SERVER_URL: &str = "https://p2p.wuhui.icu";
const HTTP_TIMEOUT_MS: u64 = 10_000;
const MAX_RESPONSE_LEN: usize = 4 << 20;

static REQUEST_PENDING: AtomicBool = AtomicBool::new(false);
static SETTINGS: Mutex<Option<Value>> = Mutex::new(None);

fn emit(app: &AppHandle, kind: &str, data: Value) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.emit_to("main", "voxlink:event", json!({ "type": kind, "data": data }));
    }
}

fn settings_path(app: &AppHandle) -> PathBuf {
    crate::settings::user_data_dir(app).join("voxlink-settings.json")
}

fn default_settings() -> Value {
    json!({ "allowRelay": true, "theme": "light", "uploadDiagnostics": false })
}

fn load_settings(file: &Path) -> Value {
    let read = (|| -> Result<Value, String> {
        let text = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
        serde_json::from_str::<Value>(&text).map_err(|e| e.to_string())
    })();
    let value = match read {
        Ok(v) => v,
        Err(_) => return default_settings(),
    };
    let theme = match value.get("theme").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => "light".to_string(),
    };
    json!({
        "uploadDiagnostics": value.get("uploadDiagnostics").and_then(|v| v.as_bool()) == Some(true),
        "allowRelay": match value.get("allowRelay").and_then(|v| v.as_bool()) {
            Some(b) => b,
            None => true,
        },
        "theme": theme,
    })
}

// 对应原版 settings.ts loadSettings()/saveSettings()：读取/解析失败回落默认值，
// 写入 tmp 名 `<file>.<uuid>.tmp` → rename → finally 删 tmp（内容仅三字段）
fn save_settings(value: &Value, file: &Path) -> Result<(), String> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let temporary = format!("{}.{}.tmp", file.display(), crate::accounts::uuid_v4());
    let body = json!({
        "allowRelay": value.get("allowRelay").cloned().unwrap_or(Value::Null),
        "theme": value.get("theme").cloned().unwrap_or(Value::Null),
        "uploadDiagnostics": value.get("uploadDiagnostics").and_then(|v| v.as_bool()) == Some(true),
    });
    let bytes = serde_json::to_vec(&body).map_err(|e| e.to_string())?;
    let written = std::fs::write(&temporary, bytes).map_err(|e| e.to_string());
    let renamed = written.and_then(|_| std::fs::rename(&temporary, file).map_err(|e| e.to_string()));
    let _ = std::fs::remove_file(&temporary);
    renamed
}

fn settings_of(app: &AppHandle) -> Value {
    let mut guard = SETTINGS.lock().unwrap();
    if guard.is_none() {
        *guard = Some(load_settings(&settings_path(app)));
    }
    guard.clone().unwrap_or_else(default_settings)
}

// 对应原版 index.ts:120-126 voxlink:settings 的三条 if 判定与写入顺序（partial 缺失按空对象，不照抄原版崩溃）
fn apply_settings(app: &AppHandle, partial: &Value) -> Value {
    let file = settings_path(app);
    let mut guard = SETTINGS.lock().unwrap();
    let mut current = guard.take().unwrap_or_else(|| load_settings(&file));
    if let Some(flag) = partial.get("uploadDiagnostics").and_then(|v| v.as_bool()) {
        current["uploadDiagnostics"] = json!(flag);
        let _ = save_settings(&current, &file);
    }
    if let Some(flag) = partial.get("allowRelay").and_then(|v| v.as_bool()) {
        current["allowRelay"] = json!(flag);
        let _ = save_settings(&current, &file);
    }
    let theme = match partial.get("theme") {
        Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
        _ => None,
    };
    if let Some(theme) = theme {
        current["theme"] = json!(theme);
        let _ = save_settings(&current, &file);
    }
    *guard = Some(current.clone());
    current
}

fn api_error(code: &str, message: &str) -> String {
    format!("{code}: {message}")
}

fn api_client() -> &'static reqwest::blocking::Client {
    static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .no_proxy()
            .build()
            .expect("voxlink HTTP 客户端初始化失败")
    })
}

fn validate_server_url(value: &str) -> bool {
    match reqwest::Url::parse(value) {
        Ok(u) => {
            matches!(u.scheme(), "https" | "http")
                && !u.host_str().unwrap_or("").is_empty()
                && u.username().is_empty()
                && u.password().is_none()
        }
        Err(_) => false,
    }
}

fn js_num(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

fn js_scalar_str(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => "[object Object]".to_string(),
    }
}

fn enc(value: &str) -> String {
    let mut out = String::new();
    for b in value.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'*' | b'/' => out.push(*b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02x}")),
        }
    }
    out
}

// 对应原版 api.ts ApiClient.do()：三个请求头、信封 {success,data,error,message}、4MB 上限、
// 10s 超时，错误串取 APIError.toString() 的 `"<code>: <message>"`；
// 编码按 URLSearchParams（空格转 +），但 `/` 保持字面、hex 用小写——服务端对 %2F 回 UNKNOWN_ENDPOINT
fn api_get(app: &AppHandle, base: &str, route: &str, params: &[(String, String)]) -> Result<Value, String> {
    if !validate_server_url(base) {
        return Err(api_error("NETWORK", "服务器地址无效"));
    }
    let version = app.package_info().version.to_string();
    let mut pairs: Vec<(String, String)> = Vec::with_capacity(params.len() + 1);
    pairs.push(("route".to_string(), route.to_string()));
    pairs.extend_from_slice(params);
    let query = pairs
        .iter()
        .map(|(name, value)| format!("{}={}", enc(name), enc(value)))
        .collect::<Vec<_>>()
        .join("&");
    let mut url = reqwest::Url::parse(base).map_err(|_| api_error("NETWORK", "服务器地址无效"))?;
    url.set_query(Some(&query));
    let mut resp = api_client()
        .get(url.as_str())
        .timeout(Duration::from_millis(HTTP_TIMEOUT_MS))
        .header("User-Agent", format!("KAMUCL-App/{version}"))
        .header("X-VoxLink-Version", version.as_str())
        .header("Content-Type", "application/json")
        .send()
        .map_err(|e| api_error("NETWORK", &e.to_string()))?;
    let status = resp.status().as_u16();
    let mut buf: Vec<u8> = Vec::new();
    let mut block = [0u8; 64 * 1024];
    loop {
        match resp.read(&mut block) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&block[..n]);
                if buf.len() > MAX_RESPONSE_LEN {
                    return Err(api_error("RESPONSE_LIMIT", "服务器响应过大"));
                }
            }
            Err(e) => return Err(api_error("NETWORK", &e.to_string())),
        }
    }
    let envelope: Value = serde_json::from_slice(&buf)
        .map_err(|_| api_error(&format!("HTTP_{status}"), "服务器未返回有效 JSON"))?;
    if !(200..=299).contains(&status) || envelope.get("success").and_then(|v| v.as_bool()) != Some(true) {
        let code = match envelope.get("error").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None if status == 429 => "RATE_LIMITED".to_string(),
            None => format!("HTTP_{status}"),
        };
        let message = envelope.get("message").and_then(|v| v.as_str()).unwrap_or("请求失败");
        return Err(api_error(&code, message));
    }
    Ok(envelope.get("data").cloned().unwrap_or(Value::Null))
}

/// 对应原版 engine.ts:518 listRooms() 的 page/size 夹取（index.ts voxlink:lobby）
fn list_rooms(app: &AppHandle, query: &Value) -> Result<Value, String> {
    let page = query.get("page").and_then(|v| v.as_f64()).unwrap_or(1.0).max(1.0);
    let size = query.get("size").and_then(|v| v.as_f64()).unwrap_or(20.0).max(1.0).min(100.0);
    let mut pairs: Vec<(String, String)> = Vec::new();
    let mut saw_page = false;
    let mut saw_size = false;
    if let Some(obj) = query.as_object() {
        for (name, value) in obj {
            match name.as_str() {
                "page" => {
                    saw_page = true;
                    pairs.push((name.clone(), js_num(page)));
                }
                "size" => {
                    saw_size = true;
                    pairs.push((name.clone(), js_num(size)));
                }
                _ => match value {
                    Value::Null => {}
                    Value::Array(list) => {
                        for item in list.iter().filter(|item| !item.is_null()) {
                            pairs.push((name.clone(), js_scalar_str(item)));
                        }
                    }
                    other => pairs.push((name.clone(), js_scalar_str(other))),
                },
            }
        }
    }
    if !saw_page {
        pairs.push(("page".to_string(), js_num(page)));
    }
    if !saw_size {
        pairs.push(("size".to_string(), js_num(size)));
    }
    api_get(app, DEFAULT_SERVER_URL, "/room/list", &pairs)
}

// 对应原版 index.ts:50-59 snapshot()（engine 未移植：恒为空闲态）
fn snapshot(app: &AppHandle) -> Value {
    json!({
        "state": "idle",
        "room": Value::Null,
        "session": { "state": "idle", "room": Value::Null, "code": "", "token": "", "isHost": false },
        "settings": settings_of(app),
        "pending": REQUEST_PENDING.load(Ordering::SeqCst),
        "joinedAt": 0,
        "connection": Value::Null,
        "stages": {},
    })
}

// 引擎未移植：这三个通道恒命中原版 engine.ts 的「无会话」首行守卫，返回值照抄空闲真值，不伪造成功
fn try_direct() -> Value {
    json!({ "ok": false, "err": "当前房间没有有效直连地址" })
}

fn use_player_relay() -> Value {
    json!({ "ok": false, "err": "请先加入房间" })
}

fn use_turn_relay() -> Result<Value, String> {
    Err("请先加入房间".to_string())
}

// 引擎未移植：无会话时原版 leaveRoom 恒为 {left:false}，stop 仍按原版 push 一份快照
fn stop(app: &AppHandle) -> Value {
    REQUEST_PENDING.store(false, Ordering::SeqCst);
    let snap = snapshot(app);
    emit(app, "state", snap.clone());
    snap
}

fn room_code_re() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"^[A-HJ-NP-Z2-9]{6}$").unwrap())
}

fn proc_re() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"^(\d+)\s+(.*)$").unwrap())
}

fn num_from_str(text: &str) -> f64 {
    if text.is_empty() {
        0.0
    } else {
        text.parse::<f64>().unwrap_or(f64::NAN)
    }
}

fn js_number(value: Option<&Value>) -> f64 {
    match value {
        None | Some(Value::Null) => 0.0,
        Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(Value::Bool(b)) => if *b { 1.0 } else { 0.0 },
        Some(Value::String(s)) => num_from_str(s.trim()),
        Some(_) => f64::NAN,
    }
}

fn js_truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Number(n)) => match n.as_f64() {
            Some(f) => f != 0.0 && !f.is_nan(),
            None => true,
        },
        Some(_) => true,
    }
}

// 对应原版 mc_ports.ts detectMcPorts()：本地进程/端口探测，不外发网络包
fn capture(program: &str, arguments: &[&str]) -> Result<String, String> {
    let mut cmd = std::process::Command::new(program);
    cmd.args(arguments).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd.stdout(std::process::Stdio::piped()).spawn().map_err(|e| e.to_string())?;
    let pipe = child.stdout.take();
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut stream) = pipe {
            let _ = stream.read_to_end(&mut buf);
        }
        buf
    });
    let deadline = std::time::Instant::now() + Duration::from_millis(8_000);
    loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) => {
                let out = reader.join().map_err(|_| "读取失败".to_string())?;
                if !status.success() {
                    return Err("命令执行失败".to_string());
                }
                if out.len() > MAX_RESPONSE_LEN {
                    return Err("输出过大".to_string());
                }
                return String::from_utf8(out).map_err(|e| e.to_string());
            }
            None => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("探测超时".to_string());
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

fn detect_mc_ports() -> Vec<u64> {
    let mut found: Vec<(f64, f64)> = Vec::new();
    if std::env::consts::OS == "windows" {
        const QUERY: &str = r#"$p = @(Get-CimInstance Win32_Process -Filter "Name = 'java.exe' OR Name = 'javaw.exe'" | Where-Object { $_.CommandLine -match 'minecraft|fabric|forge|quilt|net\.minecraft' } | ForEach-Object { $_.ProcessId }); @(Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue | Where-Object { $p -contains $_.OwningProcess } | Select-Object LocalPort,OwningProcess) | ConvertTo-Json -Compress"#;
        let raw = match capture("powershell.exe", &["-NoProfile", "-NonInteractive", "-Command", QUERY]) {
            Ok(v) => v,
            Err(_) => return Vec::new(),
        };
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Vec::new();
        }
        let values: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => return Vec::new(),
        };
        let rows: Vec<&Value> = match &values {
            Value::Array(list) => list.iter().collect(),
            other => vec![other],
        };
        for row in rows {
            let port = js_number(row.get("LocalPort"));
            let pid = js_number(row.get("OwningProcess"));
            found.push((port, pid));
        }
    } else {
        let listing = match capture("ps", &["-axo", "pid=,command="]) {
            Ok(v) => v,
            Err(_) => return Vec::new(),
        };
        let mut pids: Vec<u64> = Vec::new();
        for line in listing.lines() {
            if let Some(cap) = proc_re().captures(line.trim()) {
                let command = cap.get(2).map(|m| m.as_str()).unwrap_or("");
                let pid: u64 = cap.get(1).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
                if command.to_lowercase().contains("java")
                    && ["minecraft", "fabric", "forge", "quilt"].iter().any(|k| command.to_lowercase().contains(k))
                {
                    pids.push(pid);
                }
            }
        }
        if pids.is_empty() {
            return Vec::new();
        }
        if let Ok(open) = capture("lsof", &["-nP", "-iTCP", "-sTCP:LISTEN", "-Fpn"]) {
            let mut pid = 0.0_f64;
            for line in open.lines() {
                if let Some(rest) = line.strip_prefix('p') {
                    pid = num_from_str(rest);
                } else if let Some(rest) = line.strip_prefix('n') {
                    if pids.iter().any(|own| *own as f64 == pid) {
                        let port = match rest.rfind(':') {
                            Some(idx) => num_from_str(&rest[idx + 1..]),
                            None => f64::NAN,
                        };
                        found.push((port, pid));
                    }
                }
            }
        }
    }
    let mut rows: Vec<(f64, f64)> = Vec::new();
    for row in found {
        if row.0.fract() == 0.0 && row.0 > 0.0 && row.0 <= 65535.0 && row.1 > 0.0 {
            let key = format!("{}:{}", row.1 as i64, row.0 as i64);
            if let Some(idx) = rows.iter().position(|r| format!("{}:{}", r.1 as i64, r.0 as i64) == key) {
                rows[idx] = row;
            } else {
                rows.push(row);
            }
        }
    }
    rows.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    rows.into_iter().map(|r| r.0 as u64).collect()
}

// 对应原版 index.ts:64-104 voxlink:start 的网络前校验（含 probeHostPort 的端口范围校验，
// 其 127.0.0.1 回环探测属引擎未执行）；再往后要真联网（/room/create、/room/join）与 P2P，
// 引擎未移植，返回明确错误，不伪造成功。
fn voxlink_start(payload: &Value) -> Result<Value, String> {
    if REQUEST_PENDING.load(Ordering::SeqCst) {
        return Err("正在处理联机请求，请先取消".to_string());
    }
    REQUEST_PENDING.store(true, Ordering::SeqCst);
    let result = (|| -> Result<Value, String> {
        if payload.get("mode").and_then(|v| v.as_str()) == Some("join") {
            let raw = match payload.get("code") {
                None | Some(Value::Null) => String::new(),
                Some(v) => js_scalar_str(v),
            };
            let code = raw.trim().to_uppercase();
            if !room_code_re().is_match(&code) {
                return Err(api_error("INVALID_PARAMS", "请输入有效的六位房间码"));
            }
            return Err("联机子系统未移植".to_string());
        }
        if !js_truthy(payload.get("target")) {
            return Err("请先选择房主正在使用的游戏实例".to_string());
        }
        let mut host_port = js_number(payload.get("hostPort"));
        if host_port == 0.0 || host_port.is_nan() {
            host_port = detect_mc_ports().first().copied().map(|p| p as f64).unwrap_or(0.0);
        }
        if host_port == 0.0 || host_port.is_nan() {
            return Err("请先启动游戏并对局域网开放世界".to_string());
        }
        if host_port.fract() != 0.0 || host_port < 1.0 || host_port > 65535.0 {
            return Err("请填写游戏中显示的有效端口（1–65535）".to_string());
        }
        Err("联机子系统未移植".to_string())
    })();
    REQUEST_PENDING.store(false, Ordering::SeqCst);
    result
}

pub fn invoke(app: &AppHandle, channel: &str, args: &[Value]) -> Option<Result<Value, String>> {
    match channel {
        "voxlink:status" => Some(Ok(snapshot(app))),
        "voxlink:settings" => Some(Ok(apply_settings(app, &args.first().cloned().unwrap_or(json!({}))))),
        "voxlink:lobby" => Some(list_rooms(app, &args.first().cloned().unwrap_or(json!({})))),
        "voxlink:stop" => Some(Ok(stop(app))),
        "voxlink:tryDirect" => Some(Ok(try_direct())),
        "voxlink:usePlayerRelay" => Some(Ok(use_player_relay())),
        "voxlink:useTurnRelay" => Some(use_turn_relay()),
        "voxlink:start" => Some(voxlink_start(&args.first().cloned().unwrap_or(json!({})))),
        _ => None,
    }
}
