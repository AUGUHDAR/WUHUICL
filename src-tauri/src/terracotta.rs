//! 陶瓦联机（Terracotta 官方工具集成），对应原版 src/main/core/terracotta.ts。
//! 通道全部为 ipcMain.handle（原版无 ipcMain.on 版）：tc:install / tc:cancel-install /
//! tc:start / tc:stop / tc:status；事件 tc:event {type:'log'|'ready'|'error'|'stopped'|'status', data}；
//! 内部函数按 ensureBinary/download/extractTarGz/verifySha256/startProcess/tcGet/
//! pollUntilReady/killTree/tcStart/tcStop 同名一一对应。

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

const TC_VERSION: &str = "0.4.2";
const START_TOTAL_TIMEOUT_MS: u64 = 120_000;
const PORT_POLL_MS: u64 = 500;
const STATE_POLL_MS: u64 = 500;
const HTTP_TIMEOUT_MS: u64 = 8_000;
const CONNECT_TIMEOUT_MS: u64 = 20_000;
const MAX_REDIRECTS: u32 = 5;
const MAX_UNGZIP_BYTES: u64 = 256 * 1024 * 1024;
const PROGRESS_THROTTLE_MS: u64 = 200;
const HOST_TIMEOUT_SEC: u64 = 30;
const JOIN_TIMEOUT_SEC: u64 = 60;
const SESSION_TIMEOUT_SEC: u64 = 5;
const TASKKILL_WAIT_MS: u64 = 800;
const PROC_POLL_MS: u64 = 50;

struct Asset {
    pkg: String,
    package_sha256: &'static str,
    sha256: &'static str,
    exe: String,
}

fn asset_key() -> String {
    let platform = match std::env::consts::OS {
        "windows" => "win32",
        "macos" => "darwin",
        other => other,
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => other,
    };
    format!("{platform}-{arch}")
}

// 对应原版 terracotta.ts ASSETS 表（平台资产与 SHA-256 逐字照抄）
fn asset_for(key: &str) -> Option<Asset> {
    let (pkg_tag, package_sha256, sha256, exe_tag) = match key {
        "linux-x64" => (
            "linux-x86_64",
            "675c4fd6c74d49ed8165151ba2be5b6582e0af20fb6d912074543c2484b1e10a",
            "dc8eed0338a1888743ab38468d88b9dd8a60d60c29df072adba7c8d2edaf7937",
            "linux-x86_64",
        ),
        "linux-arm64" => (
            "linux-arm64",
            "845285ff264ac5fbc16db1a1605ad190e7fa64196516068cc309de5a1d2bf66d",
            "1cc03ed2ccaab8a7b64e8eb375ccfb8c1d4cd28f4c1a242fe3b492522f9f4aad",
            "linux-arm64",
        ),
        "win32-x64" => (
            "windows-x86_64",
            "07ebe139e3ca5f74576e58b1a96efe59abdfbe148d3f1a49bfdca8b6f70745f0",
            "74c10568a7fea9c1d38cf8d2d4ca90baf1517f8e5a26c63d3349db70bc449796",
            "windows-x86_64.exe",
        ),
        "win32-arm64" => (
            "windows-arm64",
            "acfab0a87a02dedc6dab7c05303186c8907f56f815548b693fb3324358da7d14",
            "782c2fa911488d487447694acca6b17fa68304c87023fb6814b83a167fc2845f",
            "windows-arm64.exe",
        ),
        "darwin-arm64" => (
            "macos-arm64",
            "13de7f9ce8733971b23493fabbe7e16d480f1e0d16a6265b4861f5a01bbecb60",
            "14a6cfa98e841c33b552f2291b0637461f37813c0bb3d29c6b56a59cb5e6714a",
            "macos-arm64",
        ),
        "darwin-x64" => (
            "macos-x86_64",
            "16306157d89423ce79fa901cdb75a6386ec1a9b1bd43a5d47c2c47cf01a16b86",
            "07899429515f7646fd6c271acb39a2d3a34d330547b1d2682c2e3311db07aa0a",
            "macos-x86_64",
        ),
        _ => return None,
    };
    Some(Asset {
        pkg: format!("terracotta-{TC_VERSION}-{pkg_tag}-pkg.tar.gz"),
        package_sha256,
        sha256,
        exe: format!("terracotta-{TC_VERSION}-{exe_tag}"),
    })
}

fn download_bases() -> [String; 4] {
    [
        format!("https://github.com/burningtnt/Terracotta/releases/download/v{TC_VERSION}"),
        format!("https://gitee.com/burningtnt/Terracotta/releases/download/v{TC_VERSION}"),
        format!("https://cnb.cool/HMCL-Terracotta/Terracotta/-/releases/download/v{TC_VERSION}"),
        format!("https://mirror.ghproxy.com/https://github.com/burningtnt/Terracotta/releases/download/v{TC_VERSION}"),
    ]
}

fn unsupported(key: &str) -> String {
    format!("陶瓦联机暂不支持此平台：{key}")
}

fn tc_dir(app: &AppHandle) -> PathBuf {
    crate::settings::user_data_dir(app).join("terracotta")
}

fn binary_path(app: &AppHandle) -> Result<String, String> {
    let key = asset_key();
    let asset = asset_for(&key).ok_or_else(|| unsupported(&key))?;
    tc_dir(app)
        .join(asset.exe)
        .to_str()
        .map(|s| s.to_string())
        .ok_or_else(|| unsupported(&key))
}

static STATE: Mutex<Option<Value>> = Mutex::new(None);
static PROC: Mutex<Option<Arc<Proc>>> = Mutex::new(None);
static HTTP_PORT: AtomicU64 = AtomicU64::new(0);
static PORT_FILE: Mutex<String> = Mutex::new(String::new());
static DISPOSED_BY_USER: AtomicBool = AtomicBool::new(false);
static STARTING: AtomicBool = AtomicBool::new(false);
static STOPPING: AtomicBool = AtomicBool::new(false);
static OPERATION: AtomicU64 = AtomicU64::new(0);
static INSTALL_TASK: Mutex<Option<String>> = Mutex::new(None);

struct Proc {
    child: Arc<Mutex<std::process::Child>>,
    pid: u32,
    exited: Arc<AtomicBool>,
}

fn new_state() -> Value {
    // 对应原版 terracotta.ts:91 `let state: TerracottaState = { phase: 'idle' }`：
    // 空闲态只有这一个键，其余键由后续 set_state 按原版时机加入
    json!({ "phase": "idle" })
}

fn state() -> Value {
    STATE.lock().unwrap().clone().unwrap_or_else(new_state)
}

fn emit(app: &AppHandle, kind: &str, data: Value) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.emit_to("main", "tc:event", json!({ "type": kind, "data": data }));
    }
}

fn set_state(app: &AppHandle, patch: Value) {
    let merged = {
        let mut guard = STATE.lock().unwrap();
        let mut base = guard.take().unwrap_or_else(new_state);
        if let (Some(obj), Some(p)) = (base.as_object_mut(), patch.as_object()) {
            for (k, v) in p {
                obj.insert(k.clone(), v.clone());
            }
        }
        *guard = Some(base.clone());
        base
    };
    emit(app, "status", merged);
}

fn replace_state(next: Value) {
    *STATE.lock().unwrap() = Some(next);
}

fn abort_install() {
    let id = INSTALL_TASK.lock().unwrap().clone();
    if let Some(id) = id {
        crate::tasks::cancel_task(&id);
    }
}

fn installing() -> bool {
    INSTALL_TASK.lock().unwrap().is_some()
}

fn proc_alive() -> bool {
    PROC.lock().unwrap().as_ref().map(|p| !p.exited.load(Ordering::SeqCst)).unwrap_or(false)
}

fn verify_sha256(file: &Path, expected: &str) -> bool {
    let mut hasher = Sha256::new();
    let mut f = match std::fs::File::open(file) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        match f.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => hasher.update(&buf[..n]),
            Err(_) => return false,
        }
    }
    hex::encode(hasher.finalize()) == expected
}

fn terracotta_client() -> &'static reqwest::blocking::Client {
    static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .no_proxy()
            .connect_timeout(Duration::from_millis(CONNECT_TIMEOUT_MS))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("陶瓦 HTTP 客户端初始化失败")
    })
}

fn enc_component(s: &str) -> String {
    const KEEP: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_.!~*'()";
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
        if KEEP.contains(b) {
            out.push(*b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn port_re() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r#""port"\s*:\s*(\d+)"#).unwrap())
}

fn room_code_re() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"^U/[A-Z0-9]{4}(-[A-Z0-9]{4}){3}$").unwrap())
}

fn tc_get(path_name: &str) -> Result<String, String> {
    let port = HTTP_PORT.load(Ordering::SeqCst);
    let url = format!("http://127.0.0.1:{port}{path_name}");
    let resp = terracotta_client()
        .get(&url)
        .timeout(Duration::from_millis(HTTP_TIMEOUT_MS))
        .send()
        .map_err(|e| if e.is_timeout() { "请求超时".to_string() } else { e.to_string() })?;
    let status = resp.status().as_u16();
    let body = resp.text().map_err(|e| e.to_string())?;
    if status == 200 {
        return Ok(body);
    }
    let cut: String = body.chars().take(200).collect();
    Err(format!("HTTP {status}: {cut}"))
}

// 对应原版 terracotta.ts download()：手动跟随重定向（≤5）、20s 超时、200ms 进度节流、可取消
fn download(app: &AppHandle, url: &str, file: &Path, cancel: &AtomicBool, redirects: u32) -> Result<(), String> {
    if cancel.load(Ordering::SeqCst) {
        return Err("下载已取消".to_string());
    }
    if redirects > MAX_REDIRECTS {
        return Err("下载重定向过多".to_string());
    }
    let resp = terracotta_client()
        .get(url)
        .send()
        .map_err(|e| if e.is_timeout() { "下载连接超时，请重试".to_string() } else { e.to_string() })?;
    let status = resp.status().as_u16();
    let total = resp.content_length().unwrap_or(0);
    if (300..400).contains(&status) {
        // 对应原版 terracotta.ts:173：只有带 location 的 3xx 才递归跟随，否则落到下面的状态码错误
        let location = resp
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        if let Some(loc) = location {
            let joined = reqwest::Url::parse(url)
                .map_err(|e| e.to_string())?
                .join(&loc)
                .map_err(|e| e.to_string())?
                .to_string();
            return download(app, &joined, file, cancel, redirects + 1);
        }
    }
    if status != 200 {
        return Err(format!("HTTP {status}"));
    }
    let (tx, rx) = std::sync::mpsc::channel::<Result<Vec<u8>, String>>();
    std::thread::spawn(move || {
        let mut resp = resp;
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            match resp.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if tx.send(Ok(buf[..n].to_vec())).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(if e.kind() == std::io::ErrorKind::TimedOut { "下载连接超时，请重试".to_string() } else { e.to_string() }));
                    break;
                }
            }
        }
    });
    let mut out = std::fs::File::create(file).map_err(|e| e.to_string())?;
    let mut downloaded: u64 = 0;
    let mut last_update = Instant::now();
    let mut idle_at = Instant::now();
    loop {
        if cancel.load(Ordering::SeqCst) {
            drop(out);
            let _ = std::fs::remove_file(file);
            return Err("下载已取消".to_string());
        }
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(Ok(chunk)) => {
                idle_at = Instant::now();
                if out.write_all(&chunk).is_err() {
                    let _ = std::fs::remove_file(file);
                    return Err("下载写入失败".to_string());
                }
                downloaded += chunk.len() as u64;
                if last_update.elapsed() >= Duration::from_millis(PROGRESS_THROTTLE_MS) {
                    last_update = Instant::now();
                    set_state(app, json!({ "downloaded": downloaded, "total": total }));
                }
            }
            Ok(Err(e)) => {
                drop(out);
                let _ = std::fs::remove_file(file);
                return Err(e);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if idle_at.elapsed() >= Duration::from_millis(CONNECT_TIMEOUT_MS) {
                    drop(out);
                    let _ = std::fs::remove_file(file);
                    return Err("下载连接超时，请重试".to_string());
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    drop(rx);
    out.flush().map_err(|e| e.to_string())?;
    set_state(app, json!({ "downloaded": downloaded, "total": total }));
    Ok(())
}

fn tar_basename(name: &str) -> String {
    let normalized = name.replace('\\', "/");
    let trimmed = normalized.trim_end_matches('/');
    trimmed.rsplit('/').next().unwrap_or("").to_string()
}

// 对应原版 terracotta.ts extractTarGz()：gunzip（上限 256MB）+ ustar 头解析，只落地普通文件，按 basename 写入目标目录
fn extract_tar_gz(tar_gz: &Path, dest_dir: &Path) -> Result<(), String> {
    let f = std::fs::File::open(tar_gz).map_err(|e| e.to_string())?;
    let mut dec = flate2::read::GzDecoder::new(f);
    let mut raw: Vec<u8> = Vec::new();
    let read = std::io::Read::by_ref(&mut dec)
        .take(MAX_UNGZIP_BYTES + 1)
        .read_to_end(&mut raw)
        .map_err(|e| e.to_string())?;
    if read as u64 > MAX_UNGZIP_BYTES {
        return Err(format!("陶瓦压缩包解压超出 {MAX_UNGZIP_BYTES} 字节上限"));
    }
    let cut = |bytes: &[u8]| -> String {
        let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
        String::from_utf8_lossy(&bytes[..end]).to_string()
    };
    let mut off: usize = 0;
    while off + 512 <= raw.len() {
        let header = &raw[off..off + 512];
        if header.iter().all(|b| *b == 0) {
            break;
        }
        let name = cut(&header[0..100]);
        let size_text = cut(&header[124..136]).trim().to_string();
        let size: usize = if size_text.is_empty() {
            0
        } else {
            usize::from_str_radix(&size_text, 8).map_err(|_| "陶瓦压缩包结构不完整".to_string())?
        };
        let type_flag = header.get(156).copied().unwrap_or(b'0');
        off += 512;
        if off + size > raw.len() {
            return Err("陶瓦压缩包结构不完整".to_string());
        }
        if name.is_empty() {
            break;
        }
        let data = raw[off..off + size].to_vec();
        off += size.div_ceil(512) * 512;
        if type_flag == b'0' || type_flag == 0 {
            let base = tar_basename(&name);
            if !base.is_empty() && base != ".." && base != "." {
                std::fs::write(dest_dir.join(&base), data.as_slice()).map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> Result<(), String> {
    Ok(())
}

// 对应原版 terracotta.ts ensureBinary()：镜像顺序 × 每源 2 次尝试 → 包 SHA-256 → 解压 → EXE SHA-256
fn ensure_binary(app: &AppHandle, cancel: &Arc<AtomicBool>) -> Result<String, String> {
    let key = asset_key();
    let asset = asset_for(&key).ok_or_else(|| unsupported(&key))?;
    let dir = tc_dir(app);
    let exe = dir.join(&asset.exe);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    if exe.exists() {
        if verify_sha256(&exe, asset.sha256) {
            if !cfg!(windows) {
                set_executable(&exe)?;
            }
            return Ok(exe.to_string_lossy().to_string());
        }
        emit(app, "log", json!({ "level": "warn", "msg": "本地陶瓦二进制校验失败，重新下载" }));
        let _ = std::fs::remove_file(&exe);
    }
    let pkg_path = dir.join(&asset.pkg);
    let mut last_err: Option<String> = None;
    for base in download_bases() {
        for _attempt in 0..2 {
            let outcome: Result<(), String> = (|| {
                set_state(app, json!({ "phase": "downloading" }));
                let url = format!("{base}/{}", asset.pkg);
                let host = reqwest::Url::parse(&url).ok().and_then(|u| u.host_str().map(|h| h.to_string())).unwrap_or_default();
                emit(app, "log", json!({ "level": "info", "msg": format!("下载陶瓦官方二进制：{host}") }));
                download(app, &url, &pkg_path, &**cancel, 0)?;
                if !verify_sha256(&pkg_path, asset.package_sha256) {
                    return Err("SHA-256 校验失败（包已损坏或被篡改）".to_string());
                }
                if cancel.load(Ordering::SeqCst) {
                    return Err("下载已取消".to_string());
                }
                extract_tar_gz(&pkg_path, &dir)?;
                if !exe.exists() {
                    return Err(format!("压缩包内未找到 {}", asset.exe));
                }
                if !verify_sha256(&exe, asset.sha256) {
                    let _ = std::fs::remove_file(&exe);
                    return Err("陶瓦 EXE 校验失败".to_string());
                }
                let _ = std::fs::remove_file(&pkg_path);
                if !cfg!(windows) {
                    set_executable(&exe)?;
                }
                emit(app, "log", json!({ "level": "info", "msg": "陶瓦官方二进制就绪（已通过 SHA-256 校验）" }));
                Ok(())
            })();
            match outcome {
                Ok(()) => return Ok(exe.to_string_lossy().to_string()),
                Err(e) => {
                    last_err = Some(e.clone());
                    let _ = std::fs::remove_file(&pkg_path);
                    if cancel.load(Ordering::SeqCst) {
                        return Err("下载已取消".to_string());
                    }
                    emit(app, "log", json!({ "level": "warn", "msg": format!("下载源失败：{e}，尝试下一个") }));
                }
            }
        }
    }
    Err(format!(
        "陶瓦二进制下载失败：{}。可到 https://github.com/burningtnt/Terracotta/releases 手动下载后放到 {}",
        last_err.unwrap_or_else(|| "全部镜像不可用".to_string()),
        dir.to_string_lossy()
    ))
}

fn tc_install(app: &AppHandle) -> Result<Value, String> {
    if installing() || STARTING.load(Ordering::SeqCst) || STOPPING.load(Ordering::SeqCst) || proc_alive() {
        return Err("陶瓦正在运行或下载，请稍后重试".to_string());
    }
    let rec = crate::tasks::register_task("下载陶瓦工具", "download");
    let task_id = rec.id.clone();
    let cancel = rec.cancelled.clone();
    *INSTALL_TASK.lock().unwrap() = Some(task_id.clone());
    set_state(app, json!({ "phase": "downloading", "error": Value::Null, "downloaded": 0, "total": 0 }));
    let (tx, rx) = std::sync::mpsc::channel::<Result<(), String>>();
    let app2 = app.clone();
    let thread_cancel = cancel.clone();
    let done_task_id = task_id.clone();
    std::thread::spawn(move || {
        let res = ensure_binary(&app2, &thread_cancel).map(|_| ());
        let cancelled = thread_cancel.load(Ordering::SeqCst);
        match &res {
            Ok(()) => crate::tasks::task_done(&app2, &done_task_id, true, None, false, Some("terracotta".to_string())),
            Err(e) => crate::tasks::task_done(&app2, &done_task_id, false, Some(e.clone()), cancelled, Some("terracotta".to_string())),
        }
        let _ = tx.send(res);
    });
    let result = rx.recv().map_err(|_| "陶瓦下载任务已中断".to_string())?;
    {
        let mut guard = INSTALL_TASK.lock().unwrap();
        if guard.as_deref() == Some(task_id.as_str()) {
            *guard = None;
        }
    }
    match result {
        Ok(()) => {
            set_state(app, json!({ "phase": "idle" }));
            emit(app, "ready", Value::Null);
            Ok(Value::Null)
        }
        Err(e) => {
            let msg = if cancel.load(Ordering::SeqCst) { "下载已取消".to_string() } else { e };
            set_state(app, json!({ "phase": "idle", "error": msg.clone() }));
            Err(msg)
        }
    }
}

fn tc_status(app: &AppHandle) -> Result<Value, String> {
    let key = asset_key();
    let binary_ready = match asset_for(&key) {
        Some(asset) => verify_sha256(&tc_dir(app).join(&asset.exe), asset.sha256),
        None => false,
    };
    let mut obj = state();
    {
        let map = obj.as_object_mut().ok_or("陶瓦状态损坏")?;
        map.insert("binaryReady".to_string(), json!(binary_ready));
        map.insert("running".to_string(), json!(proc_alive()));
        map.insert("toolVersion".to_string(), json!(TC_VERSION));
    }
    let path = binary_path(app)?;
    obj["binaryPath"] = json!(path);
    Ok(obj)
}

fn random_bytes(len: usize) -> Vec<u8> {
    use rand::RngCore;
    let mut buf = vec![0u8; len];
    rand::rng().fill_bytes(&mut buf);
    buf
}

fn read_mac_port(lock: &Path) -> Result<Option<u64>, String> {
    let meta = match std::fs::symlink_metadata(lock) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
        Ok(m) => m,
    };
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err("陶瓦端口文件不是普通文件".to_string());
    }
    if meta.len() < 2 {
        return Ok(None);
    }
    if meta.len() != 2 {
        return Err("陶瓦端口文件格式错误".to_string());
    }
    let raw = std::fs::read(lock).map_err(|e| e.to_string())?;
    if raw.len() < 2 {
        return Ok(None);
    }
    if raw.len() != 2 {
        return Err("陶瓦端口文件格式错误".to_string());
    }
    let port = u16::from_be_bytes([raw[0], raw[1]]) as u64;
    if port == 0 {
        return Err("陶瓦端口无效".to_string());
    }
    Ok(Some(port))
}

fn prepare_mac_identity(dir_path: &Path, session: &Path) -> Result<(), String> {
    let persistent = dir_path.join("terracotta");
    std::fs::create_dir_all(&persistent).map_err(|e| e.to_string())?;
    let meta = std::fs::symlink_metadata(&persistent).map_err(|e| e.to_string())?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err("陶瓦身份目录不是独立目录".to_string());
    }
    let identity = persistent.join("machine-id");
    match std::fs::OpenOptions::new().write(true).create_new(true).open(&identity) {
        Ok(mut f) => f.write_all(&random_bytes(16)).map_err(|e| e.to_string())?,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.to_string()),
    }
    let stat = std::fs::symlink_metadata(&identity).map_err(|e| e.to_string())?;
    if !stat.is_file() || stat.file_type().is_symlink() || stat.len() != 16 {
        return Err("陶瓦身份文件损坏，已保留原文件；请备份后修复 machine-id".to_string());
    }
    let service = session.join("terracotta");
    std::fs::create_dir(&service).map_err(|e| e.to_string())?;
    let bytes = std::fs::read(&identity).map_err(|e| e.to_string())?;
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(service.join("machine-id")).map_err(|e| e.to_string())?;
    f.write_all(&bytes).map_err(|e| e.to_string())
}

fn spawn_log_reader<R: Read + Send + 'static>(app: &AppHandle, pipe: Option<R>, level: &'static str) {
    let Some(mut pipe) = pipe else { return };
    let app = app.clone();
    std::thread::spawn(move || {
        let mut buf = vec![0u8; 8 * 1024];
        loop {
            match pipe.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let msg = String::from_utf8_lossy(&buf[..n]).trim().to_string();
                    emit(&app, "log", json!({ "level": level, "msg": msg }));
                }
                Err(_) => break,
            }
        }
    });
}

fn proc_matches(pid: u32) -> bool {
    PROC.lock().unwrap().as_ref().map(|p| p.pid == pid).unwrap_or(false)
}

// 对应原版 startProcess()：--hmcl2 <portFile>（Windows）+ 轮询端口文件，120s 上限
fn start_process(app: &AppHandle) -> Result<u64, String> {
    let exe = binary_path(app)?;
    let linux = cfg!(target_os = "linux");
    let mac = cfg!(target_os = "macos");
    let dir_path = tc_dir(app);
    if linux || mac {
        std::fs::create_dir_all(&dir_path).map_err(|e| e.to_string())?;
        let meta = std::fs::symlink_metadata(&dir_path).map_err(|e| e.to_string())?;
        if !meta.is_dir() || meta.file_type().is_symlink() {
            return Err("陶瓦工具目录不是独立目录".to_string());
        }
    }
    let prefix = if linux { "linux-session-" } else if mac { "mac-session-" } else { "kamucl-tc-" };
    let base = if linux || mac { dir_path.clone() } else { std::env::temp_dir() };
    let session = base.join(format!("{prefix}{}", crate::accounts::uuid_v4()));
    std::fs::create_dir_all(&session).map_err(|e| e.to_string())?;
    let port_file = session.join("http");
    let _ = std::fs::remove_file(&port_file);
    DISPOSED_BY_USER.store(false, Ordering::SeqCst);
    if mac {
        prepare_mac_identity(&dir_path, &session)?;
    }
    let mut cmd = std::process::Command::new(&exe);
    let flags = if mac { "--daemon".to_string() } else { if cfg!(windows) { "--hmcl2" } else { "--hmcl" }.to_string() };
    cmd.arg(&flags);
    if !mac {
        cmd.arg(port_file.to_string_lossy().to_string());
    }
    if mac {
        cmd.env("HOME", &session);
    } else if linux {
        cmd.env("TMPDIR", &session);
    }
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd.spawn().map_err(|e| format!("进程启动失败：{e}"))?;
    let pid = child.id();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let child = Arc::new(Mutex::new(child));
    let exited = Arc::new(AtomicBool::new(false));
    *PROC.lock().unwrap() = Some(Arc::new(Proc { child: child.clone(), pid, exited: exited.clone() }));
    *PORT_FILE.lock().unwrap() = port_file.to_string_lossy().to_string();

    spawn_log_reader(app, stdout, "info");
    spawn_log_reader(app, stderr, "warn");

    // 对应原版 proc.on('exit')：退出日志 + 非用户停止时复位为 idle 并推 stopped
    {
        let watch_child = child.clone();
        let watch_exit = exited.clone();
        let app = app.clone();
        std::thread::spawn(move || {
            let code: Option<i32> = loop {
                if let Ok(mut c) = watch_child.try_lock() {
                    if let Ok(Some(status)) = c.try_wait() {
                        break status.code();
                    }
                }
                std::thread::sleep(Duration::from_millis(PROC_POLL_MS));
            };
            watch_exit.store(true, Ordering::SeqCst);
            let text = code.map(|c| c.to_string()).unwrap_or_else(|| "信号".to_string());
            emit(&app, "log", json!({ "level": "info", "msg": format!("陶瓦进程退出（{text}）") }));
            if !proc_matches(pid) {
                return;
            }
            *PROC.lock().unwrap() = None;
            if !DISPOSED_BY_USER.load(Ordering::SeqCst) {
                set_state(&app, json!({ "phase": "idle", "room": Value::Null, "url": Value::Null }));
                emit(&app, "stopped", Value::Null);
            }
        });
    }

    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_millis(START_TOTAL_TIMEOUT_MS) {
        if !proc_matches(pid) || exited.load(Ordering::SeqCst) {
            return Err("陶瓦进程在启动期间退出，请查看日志".to_string());
        }
        if mac {
            let port = read_mac_port(&session.join("terracotta").join("terracotta.lock"))?;
            if let Some(port) = port {
                HTTP_PORT.store(port, Ordering::SeqCst);
                if let Ok(text) = tc_get("/state") {
                    let observed: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
                    if observed.get("state").and_then(|v| v.as_str()).is_some() && proc_matches(pid) && !exited.load(Ordering::SeqCst) {
                        emit(app, "log", json!({ "level": "info", "msg": format!("陶瓦 HTTP 端口 {port}") }));
                        return Ok(port);
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(PORT_POLL_MS));
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(&port_file) {
            if let Some(cap) = port_re().captures(content.trim()) {
                if let Some(m) = cap.get(1) {
                    if let Ok(port) = m.as_str().parse::<u64>() {
                        HTTP_PORT.store(port, Ordering::SeqCst);
                        emit(app, "log", json!({ "level": "info", "msg": format!("陶瓦 HTTP 端口 {port}") }));
                        return Ok(port);
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_millis(PORT_POLL_MS));
    }
    Err("陶瓦启动超时（120s）未写出端口文件".to_string())
}

fn kill_tree(app: &AppHandle) {
    let taken = PROC.lock().unwrap().take();
    HTTP_PORT.store(0, Ordering::SeqCst);
    let Some(p) = taken else { return };
    if p.exited.load(Ordering::SeqCst) {
        return;
    }
    if cfg!(windows) {
        let pid = p.pid.to_string();
        let mut cmd = std::process::Command::new("taskkill");
        cmd.args(["/T", "/F", "/PID", pid.as_str()]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        let _ = cmd.spawn();
        std::thread::sleep(Duration::from_millis(TASKKILL_WAIT_MS));
    } else {
        let _ = p.child.lock().unwrap().kill();
        let deadline = Instant::now() + Duration::from_secs(SESSION_TIMEOUT_SEC);
        while !p.exited.load(Ordering::SeqCst) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(PROC_POLL_MS));
        }
    }
    DISPOSED_BY_USER.store(true, Ordering::SeqCst);
}

fn poll_until_ready(app: &AppHandle, kind: &str, timeout_sec: u64) -> Result<Value, String> {
    let deadline = Instant::now() + Duration::from_secs(timeout_sec);
    while Instant::now() < deadline {
        if !proc_alive() {
            return Err("陶瓦进程已退出".to_string());
        }
        if let Ok(text) = tc_get("/state") {
            if let Ok(json) = serde_json::from_str::<Value>(&text) {
                let st = json.get("state").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                let prev = state().get("stateRaw").and_then(|v| v.as_str()).map(|s| s.to_string());
                if prev.as_deref() != Some(st.as_str()) {
                    set_state(app, json!({ "stateRaw": st.clone() }));
                    emit(app, "log", json!({ "level": "info", "msg": format!("陶瓦状态：{st}") }));
                }
                if st == "exception" || st == "fatal" || st == "failed" {
                    let _ = tc_get("/state/ide");
                    return Err(format!("陶瓦进入异常状态（{st}），已复位，可重试"));
                }
                if kind == "host" && st == "host-ok" && json.get("room").and_then(|v| v.as_str()).is_some() {
                    return Ok(json);
                }
                if kind == "join" && st == "guest-ok" && json.get("url").and_then(|v| v.as_str()).is_some() {
                    return Ok(json);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(STATE_POLL_MS));
    }
    Err(if kind == "host" { "等待房间号超时（30s）".to_string() } else { "等待连接就绪超时".to_string() })
}

fn payload_str(payload: &Value, key: &str) -> String {
    match payload.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    }
}

// 对应原版 tcStart()：早退守卫 reject，正文错误按原样收敛进 state.error / tc:event error
fn tc_start(app: &AppHandle, payload: &Value) -> Result<Value, String> {
    if STARTING.load(Ordering::SeqCst) {
        return Err("陶瓦联机正在启动中，请稍候再试".to_string());
    }
    if STOPPING.load(Ordering::SeqCst) {
        return Err("正在关闭陶瓦房间，请稍候".to_string());
    }
    if installing() {
        return Err("请等待陶瓦工具下载完成".to_string());
    }
    if proc_alive() {
        return Err("陶瓦已运行，请先关闭当前房间".to_string());
    }
    let generation = OPERATION.fetch_add(1, Ordering::SeqCst) + 1;
    STARTING.store(true, Ordering::SeqCst);
    let mode = payload.get("mode").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let outcome: Result<(), String> = (|| {
        let ready = match asset_for(&asset_key()) {
            Some(asset) => verify_sha256(&tc_dir(app).join(&asset.exe), asset.sha256),
            None => false,
        };
        if !ready {
            return Err("请先点击「下载陶瓦工具」，下载并校验完成后再联机".to_string());
        }
        if generation != OPERATION.load(Ordering::SeqCst) {
            return Ok(());
        }
        set_state(app, json!({ "phase": "starting" }));
        start_process(app)?;
        if generation != OPERATION.load(Ordering::SeqCst) {
            return Ok(());
        }
        let name = payload_str(payload, "playerName");
        let me = if name.is_empty() { "KAMUCL".to_string() } else { name };
        if mode == "host" {
            set_state(app, json!({ "phase": "hosting", "room": Value::Null, "url": Value::Null, "error": Value::Null }));
            tc_get(&format!("/state/scanning?player={}", enc_component(&me)))?;
            emit(app, "log", json!({ "level": "info", "msg": "已请求创建陶瓦房间，等待房间号…" }));
            let final_state = poll_until_ready(app, "host", HOST_TIMEOUT_SEC)?;
            if generation != OPERATION.load(Ordering::SeqCst) {
                return Ok(());
            }
            let room = final_state.get("room").cloned().unwrap_or(Value::Null);
            set_state(app, json!({ "phase": "ready", "room": room, "url": Value::Null }));
            emit(app, "ready", json!({ "mode": "host", "room": room, "state": final_state.get("state").cloned().unwrap_or(Value::Null) }));
        } else {
            let code = payload_str(payload, "code").trim().to_uppercase();
            if !room_code_re().is_match(&code) {
                return Err("陶瓦房间码格式应为 U/XXXX-XXXX-XXXX-XXXX（U/ 开头共四段）".to_string());
            }
            set_state(app, json!({ "phase": "joining", "room": code.clone(), "url": Value::Null, "error": Value::Null }));
            tc_get(&format!("/state/guesting?room={}&player={}", enc_component(&code), enc_component(&me)))?;
            emit(app, "log", json!({ "level": "info", "msg": "已请求加入陶瓦房间，等待连接就绪…" }));
            let final_state = poll_until_ready(app, "join", JOIN_TIMEOUT_SEC)?;
            if generation != OPERATION.load(Ordering::SeqCst) {
                return Ok(());
            }
            let url = final_state.get("url").cloned().unwrap_or(Value::Null);
            set_state(app, json!({ "phase": "ready", "room": code, "url": url.clone() }));
            emit(app, "ready", json!({ "mode": "join", "url": url, "state": final_state.get("state").cloned().unwrap_or(Value::Null) }));
        }
        Ok(())
    })();
    if let Err(e) = outcome {
        if generation != OPERATION.load(Ordering::SeqCst) {
            return Ok(state());
        }
        kill_tree(app);
        set_state(app, json!({ "phase": "idle", "error": e.clone() }));
        emit(app, "error", json!(e));
    }
    if generation == OPERATION.load(Ordering::SeqCst) {
        STARTING.store(false, Ordering::SeqCst);
    }
    Ok(state())
}

// 对应原版 tcStop()：operation++、中止下载、/panic?peaceful=true、杀进程树、state 复位
fn tc_stop(app: &AppHandle, silent: bool) -> Value {
    if STOPPING.load(Ordering::SeqCst) {
        return state();
    }
    STOPPING.store(true, Ordering::SeqCst);
    OPERATION.fetch_add(1, Ordering::SeqCst);
    STARTING.store(false, Ordering::SeqCst);
    abort_install();
    DISPOSED_BY_USER.store(true, Ordering::SeqCst);
    if HTTP_PORT.load(Ordering::SeqCst) > 0 {
        let _ = tc_get("/panic?peaceful=true");
    }
    kill_tree(app);
    replace_state(new_state());
    if !silent {
        emit(app, "stopped", Value::Null);
    }
    STOPPING.store(false, Ordering::SeqCst);
    state()
}

/// 对应原版 stopTerracottaOnQuit()：退出时只中止下载 + killTree（不复位状态、不请求 /panic）。
pub fn stop_on_quit(app: &AppHandle) {
    abort_install();
    kill_tree(app);
}

pub fn invoke(app: &AppHandle, channel: &str, args: &[Value]) -> Option<Result<Value, String>> {
    match channel {
        "tc:status" => Some(tc_status(app)),
        "tc:install" => Some(tc_install(app)),
        "tc:cancel-install" => {
            abort_install();
            Some(Ok(Value::Null))
        }
        "tc:start" => Some(tc_start(app, &args.first().cloned().unwrap_or_else(|| json!({})))),
        "tc:stop" => Some(Ok(tc_stop(app, false))),
        _ => None,
    }
}
