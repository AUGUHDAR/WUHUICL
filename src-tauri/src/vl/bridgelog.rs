//! 本地 TCP 转发 + 联机诊断日志（对应原版 voxlink/bridge.ts 51 行 + voxlink/connectionLog.ts 58 行）
//!
//! bridge.ts:2 的说明照抄：背压与生命周期由 socket 自己拥有，这里不加额外缓冲。
//! 渲染层契约：guest 侧的 `127.0.0.1:<port>` 字符串原样进 voxlink:event 的 conn:state.address；
//! ConnectionLog 没有任何 IPC 输出，只按 settings.uploadDiagnostics 决定是否上传。

use crate::vl::rudp::{now_ms, RudpConn};
use crate::vl::session::HTTP_TIMEOUT_MS;
use flate2::{Compression, GzBuilder};
use sha2::{Digest, Sha256};
use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// bridge.ts:5 —— 缓冲与四个超时窗口，数值一字不改
pub const BRIDGE_BUF_SIZE: usize = 32768;
pub const BRIDGE_IDLE_TIMEOUT_MS: u64 = 30000;
pub const BRIDGE_DIAL_WINDOW_MS: u64 = 10000;
pub const BRIDGE_DIAL_RETRY_MS: u64 = 1000;
pub const BRIDGE_DIAL_TIMEOUT_MS: u64 = 2000;

/// bridge.ts:6 `BridgeDownCb = (() => void) | null`
pub type BridgeDownCb = Option<Arc<dyn Fn() + Send + Sync>>;
/// bridge.ts:7 `LogFn`，level 取 'info' | 'warn' | 'error'
pub type LogFn = Option<Arc<dyn Fn(&str, &str) + Send + Sync>>;

/// bridge.ts:13 Date.now()，取整毫秒
fn now_millis() -> u64 {
    now_ms() as u64
}

struct BridgeInner {
    rc: Arc<RudpConn>,
    down: BridgeDownCb,
    listener: Mutex<Option<TcpListener>>,
    socket: Mutex<Option<TcpStream>>,
    stopped: AtomicBool,
    activity: AtomicU64,
}

/// bridge.ts:9 TcpBridge —— 一条 RUDP 通路配一条本地游戏连接
pub struct TcpBridge {
    inner: Arc<BridgeInner>,
}

/// bridge.ts:24 startGuest 的返回形状 `{ addr, bridge }`
pub struct GuestBridge {
    pub addr: String,
    pub bridge: Arc<TcpBridge>,
}

/// bridge.ts:18 stop —— 幂等；关监听、拆连接、收 RUDP、最后回调 down
fn stop_inner(inner: &Arc<BridgeInner>) {
    if inner.stopped.swap(true, Ordering::SeqCst) {
        return;
    }
    if let Some(listener) = inner.listener.lock().unwrap().take() {
        drop(listener);
    }
    if let Some(socket) = inner.socket.lock().unwrap().take() {
        let _ = socket.shutdown(Shutdown::Both);
    }
    inner.rc.close();
    if let Some(down) = inner.down.as_ref() {
        down();
    }
}

/// bridge.ts:19 setConn —— 槽位存句柄副本（Rust 句柄无法按引用共享），旧连接直接拆掉；
/// 原版的 `_old?: unknown` 形参从未被读取，故这里不保留
fn set_conn_inner(inner: &Arc<BridgeInner>, socket: &TcpStream) -> std::io::Result<()> {
    let handle = socket.try_clone()?;
    // Windows 上 accept 出来的连接继承监听套接字的非阻塞模式，
    // 不纠正回来会把 WouldBlock 当成对端关闭（bridge.ts 的 socket 从不非阻塞）
    handle.set_nonblocking(false)?;
    if let Some(old) = inner.socket.lock().unwrap().replace(handle) {
        let _ = old.shutdown(Shutdown::Both);
    }
    inner.activity.store(now_millis(), Ordering::SeqCst);
    Ok(())
}

/// bridge.ts:8 send —— Node 的 write 回调只在整块写失败时报错，等价 write_all
fn send(socket: &TcpStream, data: &[u8]) -> std::io::Result<()> {
    let mut handle = socket.try_clone()?;
    handle.write_all(data)
}

/// bridge.ts:26 pump —— 两个方向各起一个线程，任一先结束就走 stop()
fn pump_inner(inner: &Arc<BridgeInner>, socket: &TcpStream) {
    let _ = socket.set_nodelay(true);
    let (reader, writer) = match (socket.try_clone(), socket.try_clone()) {
        (Ok(reader), Ok(writer)) => (reader, writer),
        _ => {
            stop_inner(inner);
            return;
        }
    };
    let (tx, rx) = channel::<()>();
    let upload = {
        let inner = inner.clone();
        let tx = tx.clone();
        std::thread::spawn(move || {
            let mut reader = reader;
            let mut buf = vec![0u8; BRIDGE_BUF_SIZE];
            loop {
                let size = match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(size) => size,
                    Err(ref err) if err.kind() == ErrorKind::Interrupted => continue,
                    Err(_) => break,
                };
                if inner.stopped.load(Ordering::SeqCst) {
                    break;
                }
                inner.activity.store(now_millis(), Ordering::SeqCst);
                if inner.rc.write(&buf[..size]).is_err() {
                    break;
                }
            }
            let _ = tx.send(());
        })
    };
    let download = {
        let inner = inner.clone();
        std::thread::spawn(move || {
            let mut writer = writer;
            let mut buf = vec![0u8; BRIDGE_BUF_SIZE];
            loop {
                if inner.stopped.load(Ordering::SeqCst) {
                    break;
                }
                let size = match inner.rc.read(&mut buf) {
                    Ok(size) => size,
                    Err(_) => 0,
                };
                if size == 0 {
                    break;
                }
                inner.activity.store(now_millis(), Ordering::SeqCst);
                if writer.write_all(&buf[..size]).is_err() {
                    break;
                }
            }
            let _ = tx.send(());
        })
    };
    let _ = rx.recv();
    // 任一方向先结束即停（bridge.ts:30）
    stop_inner(inner);
    let _ = upload.join();
    let _ = download.join();
}

impl TcpBridge {
    /// bridge.ts:14 构造函数会注册 onClosed，所以直接给出 Arc
    pub fn new(rc: Arc<RudpConn>, down: BridgeDownCb) -> Arc<TcpBridge> {
        let inner = Arc::new(BridgeInner {
            rc,
            down,
            listener: Mutex::new(None),
            socket: Mutex::new(None),
            stopped: AtomicBool::new(false),
            activity: AtomicU64::new(now_millis()),
        });
        let late = inner.clone();
        inner.rc.set_on_closed(move |_reason| stop_inner(&late));
        Arc::new(TcpBridge { inner })
    }

    /// bridge.ts:15 touch
    pub fn touch(&self) {
        self.inner.activity.store(now_millis(), Ordering::SeqCst);
    }

    /// bridge.ts:16 idleMs
    pub fn idle_ms(&self) -> u64 {
        now_millis().saturating_sub(self.inner.activity.load(Ordering::SeqCst))
    }

    /// bridge.ts:17 isStopped
    pub fn is_stopped(&self) -> bool {
        self.inner.stopped.load(Ordering::SeqCst)
    }

    /// bridge.ts:18 stop
    pub fn stop(&self) {
        stop_inner(&self.inner);
    }

    /// bridge.ts:19 setConn
    pub fn set_conn(&self, socket: &TcpStream) -> std::io::Result<()> {
        set_conn_inner(&self.inner, socket)
    }

    /// bridge.ts:26 pump —— 阻塞到任一方向结束，调用方按需自己起线程
    pub fn pump(&self, socket: &TcpStream) {
        pump_inner(&self.inner, socket);
    }

    /// bridge.ts:32 watchdogOnce
    pub fn watchdog_once(&self) -> bool {
        self.is_stopped() || self.idle_ms() > BRIDGE_IDLE_TIMEOUT_MS
    }

    /// bridge.ts:20 startGuest —— 入会侧在 127.0.0.1 临时端口上等游戏连进来
    pub fn start_guest(rc: Arc<RudpConn>, down: BridgeDownCb) -> std::io::Result<GuestBridge> {
        let bridge = TcpBridge::new(rc, down);
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let port = listener.local_addr()?.port();
        listener.set_nonblocking(true)?;
        *bridge.inner.listener.lock().unwrap() = Some(listener);
        let inner = bridge.inner.clone();
        std::thread::spawn(move || loop {
            if inner.stopped.load(Ordering::SeqCst) {
                break;
            }
            let accepted = {
                let guard = inner.listener.lock().unwrap();
                match guard.as_ref() {
                    Some(listener) => listener.accept(),
                    None => break,
                }
            };
            match accepted {
                Ok((socket, _)) => {
                    let busy = inner.socket.lock().unwrap().is_some();
                    if busy || inner.stopped.load(Ordering::SeqCst) {
                        drop(socket);
                        continue;
                    }
                    if set_conn_inner(&inner, &socket).is_err() {
                        drop(socket);
                        continue;
                    }
                    let pump = inner.clone();
                    std::thread::spawn(move || pump_inner(&pump, &socket));
                }
                Err(ref err) if err.kind() == ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(10)),
                Err(_) => {
                    stop_inner(&inner);
                    break;
                }
            }
        });
        Ok(GuestBridge { addr: format!("127.0.0.1:{port}"), bridge })
    }
}

/// bridge.ts:34 startHostLazyBridge —— 房主侧收到第一帧才回拨 127.0.0.1:hostPort；
/// 原版用 `void` 起协程，本函数阻塞，调用方自己起线程
pub fn start_host_lazy_bridge(rc: Arc<RudpConn>, port: u16, log: LogFn) {
    let mut first = vec![0u8; BRIDGE_BUF_SIZE];
    let length = match rc.read(&mut first) {
        Ok(length) => length,
        Err(_) => 0,
    };
    if length == 0 {
        return;
    }
    let deadline = now_millis() + BRIDGE_DIAL_WINDOW_MS;
    let target = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    while rc.is_connected() && now_millis() < deadline {
        let attempt = (|| -> std::io::Result<TcpStream> {
            let socket = TcpStream::connect_timeout(&target, Duration::from_millis(BRIDGE_DIAL_TIMEOUT_MS))?;
            let bridge = TcpBridge::new(rc.clone(), None);
            bridge.set_conn(&socket)?;
            send(&socket, &first[..length])?;
            bridge.pump(&socket);
            Ok(socket)
        })();
        match attempt {
            Ok(_socket) => return,
            Err(error) => {
                // 超时还原成原版中文串（bridge.ts:40）
                let message = if error.kind() == ErrorKind::TimedOut {
                    "本地游戏连接超时".to_string()
                } else {
                    error.to_string()
                };
                if let Some(log) = log.as_ref() {
                    log("warn", &message);
                }
                std::thread::sleep(Duration::from_millis(BRIDGE_DIAL_RETRY_MS));
            }
        }
    }
    rc.close();
}

/// bridge.ts:46 pumpRelay —— 返回的 stop 等价原版的 `() => void`
pub fn pump_relay(a: Arc<RudpConn>, b: Arc<RudpConn>) -> Box<dyn Fn() + Send + Sync> {
    let stopped = Arc::new(AtomicBool::new(false));
    relay_copy(a.clone(), b.clone(), stopped.clone());
    relay_copy(b.clone(), a.clone(), stopped.clone());
    let (sa, sb) = (a.clone(), b.clone());
    Box::new(move || relay_stop(&stopped, &sa, &sb))
}

fn relay_stop(stopped: &Arc<AtomicBool>, a: &Arc<RudpConn>, b: &Arc<RudpConn>) {
    if !stopped.swap(true, Ordering::SeqCst) {
        a.close();
        b.close();
    }
}

fn relay_copy(source: Arc<RudpConn>, target: Arc<RudpConn>, stopped: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let mut buf = vec![0u8; BRIDGE_BUF_SIZE];
        while !stopped.load(Ordering::SeqCst) {
            let size = match source.read(&mut buf) {
                Ok(size) => size,
                Err(_) => 0,
            };
            if size == 0 {
                break;
            }
            if target.write(&buf[..size]).is_err() {
                break;
            }
        }
        relay_stop(&stopped, &source, &target);
    });
}

/// connectionLog.ts:14 —— 单行上限
pub const LOG_LINE_UNITS: usize = 2000;
/// connectionLog.ts:15 —— 越界丢最旧一行
pub const LOG_LINE_KEEP: usize = 1000;
/// connectionLog.ts:27/35 —— 120 秒内算“刚连上就断”
pub const LOG_EARLY_MS: u64 = 120000;
/// connectionLog.ts:22 —— 入会侧兜底窗口
pub const LOG_JOIN_SCHEDULE_MS: u64 = 90000;
/// connectionLog.ts:28 —— 早断 1 秒 / 普通失败 5 秒
pub const LOG_FAILED_EARLY_MS: u64 = 1000;
pub const LOG_FAILED_MS: u64 = 5000;
/// connectionLog.ts:35 —— 已连通时推迟
pub const LOG_CONNECTED_SCHEDULE_MS: u64 = 15000;
/// connectionLog.ts:32 —— 默认上传超时
pub const LOG_UPLOAD_TIMEOUT_MS: u64 = 10000;
/// connectionLog.ts:55 —— 退出 6 秒、平时 3 秒
pub const LOG_QUIT_TIMEOUT_MS: u64 = 6000;
pub const LOG_STOP_TIMEOUT_MS: u64 = 3000;
/// connectionLog.ts:55 —— 退出且从未连通、会话满 30 秒才补传
pub const LOG_QUIT_AGE_MS: u64 = 30000;
/// connectionLog.ts:20 与 :37 的两条 route
pub const LOG_STATUS_ROUTE: &str = "/log/status";
pub const LOG_UPLOAD_ROUTE: &str = "/log/upload";
/// connectionLog.ts:19
pub const LOG_SESSION_TEXT: &str = "联机会话开始";
/// connectionLog.ts:41/43 —— UA 前缀与 'X-Log-Name' 按原版保留 KAMUCL 字样
pub const LOG_CLIENT_NAME: &str = "KAMUCL";
/// connectionLog.ts:37 —— zlib.gzipSync 的默认级别；Windows 上 zlib 1.3 写 OS 字节 10
const LOG_GZIP_LEVEL: u32 = 6;
const LOG_GZIP_OS_WINDOWS: u8 = 10;

#[derive(Default)]
struct LogState {
    lines: Vec<String>,
    code: String,
    role: String,
    started: u64,
    connected: u64,
    failed: bool,
    uploaded: bool,
    generation: u64,
}

struct LogInner {
    base: Arc<dyn Fn() -> String + Send + Sync>,
    enabled: Arc<dyn Fn() -> bool + Send + Sync>,
    secrets: Arc<dyn Fn() -> Vec<String> + Send + Sync>,
    version: String,
    http: reqwest::blocking::Client,
    state: Mutex<LogState>,
    timer_seq: AtomicU64,
    pending: Mutex<bool>,
    pending_done: Condvar,
}

/// connectionLog.ts:7 ConnectionLog
#[derive(Clone)]
pub struct ConnectionLog {
    inner: Arc<LogInner>,
}

/// diagnostics.ts:4 TOKEN_KEYS
const TOKEN_KEYS: &str = "(?:access[_-]?token|refresh[_-]?token|client[_-]?token|session[_-]?(?:id|key)|password|authorization)";

fn replace_re(text: &str, pattern: &str, with: &str) -> String {
    regex::Regex::new(pattern).expect("脱敏规则可用").replace_all(text, with).to_string()
}

/// diagnostics.ts:25 —— os.homedir()，win32 走 USERPROFILE
fn home_dir() -> String {
    std::env::var("USERPROFILE")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| std::env::var("HOME").ok())
        .unwrap_or_default()
}

/// diagnostics.ts:11 —— 与 exportlogs.rs 的 redact_text 同规则（主目录替换在 Windows 上大小写不敏感）；
/// 替换串里写 `$$` 才能输出字面量 `$HOME`（diagnostics.ts:29）
pub fn redact_diagnostic_text(input: &str, known_secrets: &[String]) -> String {
    let mut out = input.to_string();
    for secret in known_secrets {
        if secret.is_empty() || secret.len() < 4 {
            continue;
        }
        out = out.replace(secret.as_str(), "<redacted>");
    }
    out = replace_re(&out, r"(?i)(Authorization\s*:\s*Bearer\s+)[^\s,;]+", "${1}<redacted>");
    out = replace_re(&out, &format!(r#"(?i)("?{}"?\s*[:=]\s*")([^"\r\n]+)(")"#, TOKEN_KEYS), "${1}<redacted>${3}");
    out = replace_re(&out, &format!(r"(?i)({}\s*[:=]\s*)([^\s,;]+)", TOKEN_KEYS), "${1}<redacted>");
    out = replace_re(&out, r#"(?i)(--accessToken|--clientId|--xuid|--userProperties)\s+("[^"]*"|\S+)"#, "$1 <redacted>");
    out = replace_re(&out, r#"(?i)(--username|--uuid)\s+("[^"]*"|\S+)"#, "$1 <redacted-user>");
    out = replace_re(&out, r"(?i)([?&](?:access_token|refresh_token|token|key)=)[^&#\s]+", "${1}<redacted>");
    let home = home_dir();
    if !home.is_empty() {
        out = replace_re(&out, &format!("(?i){}", regex::escape(&home)), "%USERPROFILE%");
    }
    out = replace_re(&out, r#"[A-Za-z]:\\Users\\[^\\\s"']+"#, "%USERPROFILE%");
    out = replace_re(&out, r#"/(?:Users|home)/[^/\s"']+"#, "$$HOME");
    out
}

/// connectionLog.ts:14 —— String.prototype.slice 数的是 UTF-16 码元
fn utf16_slice(text: &str, limit: usize) -> String {
    let mut units = 0usize;
    let mut out = String::new();
    for ch in text.chars() {
        let width = if (ch as u32) >= 0x1_0000 { 2 } else { 1 };
        if units + width > limit {
            break;
        }
        out.push(ch);
        units += width;
    }
    out
}

/// connectionLog.ts:14 的 new Date().toISOString()
fn iso_now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// connectionLog.ts:37 —— 等价 zlib.gzipSync(Buffer.from(text))
fn gzip_payload(text: &str) -> Vec<u8> {
    let mut encoder = GzBuilder::new()
        .mtime(0)
        .operating_system(LOG_GZIP_OS_WINDOWS)
        .write(Vec::new(), Compression::new(LOG_GZIP_LEVEL));
    encoder.write_all(text.as_bytes()).expect("写内存缓冲不会失败");
    encoder.finish().expect("gzip 收尾不会失败")
}

/// connectionLog.ts:42 的 createHash('sha256').digest('hex')
fn sha256_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// connectionLog.ts:37 —— new URL(base) 后 searchParams.set('route', …)：
/// 已有 route 就地替换（保持原位置）并删掉重复项，没有才追加到末尾
fn route_url(base: &str, route: &str) -> Option<String> {
    let mut parsed = url::Url::parse(base).ok()?;
    let pairs: Vec<(String, String)> = parsed.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
    let replaced = pairs.iter().any(|(k, _)| k.as_str() == "route");
    let mut serializer = parsed.query_pairs_mut();
    serializer.clear();
    let mut written = false;
    for (k, v) in &pairs {
        if k.as_str() == "route" {
            if written {
                continue;
            }
            serializer.append_pair("route", route);
            written = true;
        } else {
            serializer.append_pair(k, v);
        }
    }
    if !replaced {
        serializer.append_pair("route", route);
    }
    drop(serializer);
    Some(parsed.to_string())
}

/// connectionLog.ts:20 —— ApiClient.post(base, '/log/status', {code, role, enabled:true})
fn log_post_status(inner: &Arc<LogInner>, code: &str, role: &str) {
    let Some(url) = route_url(&(inner.base)(), LOG_STATUS_ROUTE) else {
        return;
    };
    let body = serde_json::json!({ "code": code, "role": role, "enabled": true });
    let _ = inner
        .http
        .post(url)
        .timeout(Duration::from_millis(HTTP_TIMEOUT_MS))
        .header("User-Agent", format!("KAMUCL-App/{}", inner.version))
        .header("X-VoxLink-Version", inner.version.as_str())
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .send();
}

fn log_clear_timer(inner: &Arc<LogInner>) {
    inner.timer_seq.fetch_add(1, Ordering::SeqCst);
}

/// connectionLog.ts:31 schedule —— 同一时刻只留最后一个定时器
fn log_schedule(inner: &Arc<LogInner>, ms: u64) {
    log_clear_timer(inner);
    let seq = inner.timer_seq.load(Ordering::SeqCst);
    let late = inner.clone();
    std::thread::spawn(move || {
        // 定时器 unref：不拖住退出（connectionLog.ts:31）
        let began = Instant::now();
        while began.elapsed() < Duration::from_millis(ms) {
            if late.timer_seq.load(Ordering::SeqCst) != seq {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if late.timer_seq.load(Ordering::SeqCst) != seq {
            return;
        }
        log_upload(&late, LOG_UPLOAD_TIMEOUT_MS);
    });
}

/// connectionLog.ts:32 upload —— pending 复用：已在途就等它结束（原版的 return this.pending）
fn log_upload(inner: &Arc<LogInner>, timeout: u64) {
    let mut pending = inner.pending.lock().unwrap();
    if *pending {
        while *pending {
            pending = inner.pending_done.wait(pending).unwrap();
        }
        return;
    }
    *pending = true;
    drop(pending);
    log_upload_now(inner, timeout);
    *inner.pending.lock().unwrap() = false;
    inner.pending_done.notify_all();
}

fn log_upload_now(inner: &Arc<LogInner>, timeout: u64) {
    let enabled = (inner.enabled)();
    let base = (inner.base)();
    let now = now_millis();
    let (epoch, code, role, elapsed, connected, uploaded, text) = {
        let st = inner.state.lock().unwrap();
        (
            st.generation,
            st.code.clone(),
            st.role.clone(),
            now.saturating_sub(st.started),
            st.connected,
            st.uploaded,
            st.lines.join("\n"),
        )
    };
    if code.is_empty() || uploaded || !enabled {
        return;
    }
    if connected != 0 {
        if now.saturating_sub(connected) < LOG_EARLY_MS {
            log_schedule(inner, LOG_CONNECTED_SCHEDULE_MS);
        }
        return;
    }
    let payload = gzip_payload(&text);
    let digest = sha256_hex(&payload);
    let Some(url) = route_url(&base, LOG_UPLOAD_ROUTE) else {
        return;
    };
    let sent = inner
        .http
        .post(url)
        .timeout(Duration::from_millis(timeout))
        .header("Content-Type", "application/octet-stream")
        .header("Accept", "application/json")
        .header("User-Agent", format!("KAMUCL-App/{}", inner.version))
        .header("X-VoxLink-Version", inner.version.as_str())
        .header("X-Log-Sha256", digest)
        .header("X-Log-Role", role.as_str())
        .header("X-Log-Code", code.as_str())
        .header("X-Log-Name", LOG_CLIENT_NAME)
        .header("X-Log-Version", inner.version.as_str())
        .header("X-Log-Duration-Ms", elapsed.to_string())
        .body(payload)
        .send();
    let accepted = match sent {
        Ok(response) => {
            let ok = response.status().is_success();
            match response.json::<serde_json::Value>() {
                Ok(data) => ok && data.get("success").and_then(|v| v.as_bool()) == Some(true),
                Err(_) => false,
            }
        }
        Err(_) => false,
    };
    let mut st = inner.state.lock().unwrap();
    if epoch == st.generation && accepted {
        st.uploaded = true;
    }
}

impl ConnectionLog {
    /// connectionLog.ts:12 构造 —— version 是 Rust 侧补的第 4 个参数：原版从 package.json 读
    /// APP_VERSION，本模块不能依赖 tauri::AppHandle，由调用方传同一个值
    pub fn new<F, G, H, V>(base: F, enabled: G, secrets: H, version: V) -> ConnectionLog
    where
        F: Fn() -> String + Send + Sync + 'static,
        G: Fn() -> bool + Send + Sync + 'static,
        H: Fn() -> Vec<String> + Send + Sync + 'static,
        V: Into<String>,
    {
        ConnectionLog {
            inner: Arc::new(LogInner {
                base: Arc::new(base),
                enabled: Arc::new(enabled),
                secrets: Arc::new(secrets),
                version: version.into(),
                http: reqwest::blocking::Client::builder().no_proxy().build().expect("HTTP 客户端初始化失败"),
                state: Mutex::new(LogState::default()),
                timer_seq: AtomicU64::new(0),
                pending: Mutex::new(false),
                pending_done: Condvar::new(),
            }),
        }
    }

    /// connectionLog.ts:13 record
    pub fn record(&self, level: &str, text: &str) {
        let secrets = (self.inner.secrets)();
        let line = utf16_slice(&format!("{} [{}] {}", iso_now(), level, redact_diagnostic_text(text, &secrets)), LOG_LINE_UNITS);
        let mut st = self.inner.state.lock().unwrap();
        st.lines.push(line);
        if st.lines.len() > LOG_LINE_KEEP {
            st.lines.remove(0);
        }
    }

    /// connectionLog.ts:17 start
    pub fn start(&self, code: &str, host: bool) {
        log_clear_timer(&self.inner);
        let (stored, role) = {
            let mut st = self.inner.state.lock().unwrap();
            st.generation += 1;
            st.lines.clear();
            st.code = code.to_string();
            st.role = (if host { "host" } else { "joiner" }).to_string();
            st.started = now_millis();
            st.connected = 0;
            st.failed = false;
            st.uploaded = false;
            (st.code.clone(), st.role.clone())
        };
        self.record("info", LOG_SESSION_TEXT);
        if (self.inner.enabled)() {
            let inner = self.inner.clone();
            std::thread::spawn(move || log_post_status(&inner, &stored, &role));
        }
        if !host {
            log_schedule(&self.inner, LOG_JOIN_SCHEDULE_MS);
        }
    }

    /// connectionLog.ts:23 state —— level 直接用 status 字面量
    pub fn state(&self, status: &str, detail: &str) {
        self.record(status, detail);
        let mut delay = None;
        {
            let mut st = self.inner.state.lock().unwrap();
            if status == "success" {
                st.connected = now_millis();
                st.failed = false;
            }
            if status == "failed" {
                let early = st.connected != 0 && now_millis().saturating_sub(st.connected) < LOG_EARLY_MS;
                st.connected = 0;
                st.failed = true;
                delay = Some(if early { LOG_FAILED_EARLY_MS } else { LOG_FAILED_MS });
            }
        }
        if let Some(ms) = delay {
            log_schedule(&self.inner, ms);
        }
    }

    /// connectionLog.ts:32 upload —— 原版私有，仅测试直接驱动
    #[cfg(test)]
    fn upload(&self, timeout: u64) {
        log_upload(&self.inner, timeout);
    }

    /// connectionLog.ts:52 stop(quit = false) —— 原版 await 上传，故本函数阻塞到尝试结束
    pub fn stop(&self, quit: bool) {
        let inner = &self.inner;
        let epoch = inner.state.lock().unwrap().generation;
        log_clear_timer(inner);
        let (failed, connected, started) = {
            let st = inner.state.lock().unwrap();
            (st.failed, st.connected, st.started)
        };
        if failed || (quit && connected == 0 && now_millis().saturating_sub(started) >= LOG_QUIT_AGE_MS) {
            log_upload(inner, if quit { LOG_QUIT_TIMEOUT_MS } else { LOG_STOP_TIMEOUT_MS });
        }
        let mut st = inner.state.lock().unwrap();
        if epoch == st.generation {
            st.generation += 1;
            st.code.clear();
            drop(st);
            log_clear_timer(inner);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vl::rudp::RudpTarget;
    use flate2::read::GzDecoder;
    use std::net::UdpSocket;

    fn conn_pair() -> (Arc<RudpConn>, Arc<RudpConn>) {
        let a = Arc::new(UdpSocket::bind("127.0.0.1:0").unwrap());
        let b = Arc::new(UdpSocket::bind("127.0.0.1:0").unwrap());
        let ap = a.local_addr().unwrap();
        let bp = b.local_addr().unwrap();
        let ca = Arc::new(RudpConn::new(
            a.clone(),
            Some(RudpTarget { address: ap.ip().to_string(), port: bp.port() }),
            None,
            None,
            false,
            false,
        ));
        let cb = Arc::new(RudpConn::new(
            b.clone(),
            Some(RudpTarget { address: bp.ip().to_string(), port: ap.port() }),
            None,
            None,
            false,
            false,
        ));
        cb.start();
        ca.start();
        (ca, cb)
    }

    fn read_some(conn: &Arc<RudpConn>, want: usize, secs: u64) -> Vec<u8> {
        let mut got = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(secs);
        while got.len() < want && Instant::now() < deadline {
            let mut chunk = [0u8; 1024];
            match conn.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => got.extend_from_slice(&chunk[..n]),
                Err(_) => break,
            }
        }
        got
    }

    #[test]
    fn guest_reports_loopback_addr_and_copies_both_ways() {
        let (rc, peer) = conn_pair();
        let guest = TcpBridge::start_guest(rc.clone(), None).unwrap();
        let port = guest
            .addr
            .strip_prefix("127.0.0.1:")
            .unwrap_or_default()
            .parse::<u16>()
            .expect("conn:state.address 必须是字面 127.0.0.1:<port>");
        assert!(port > 0, "临时端口不能是 0");
        let mut game = TcpStream::connect(("127.0.0.1", port)).unwrap();
        game.write_all(b"mc-handshake").unwrap();
        assert_eq!(read_some(&peer, 12, 6), b"mc-handshake", "上行 TCP → RUDP 必须原样搬运");
        peer.write(b"mc-login").unwrap();
        let mut down = [0u8; 8];
        game.read_exact(&mut down).unwrap();
        assert_eq!(&down, b"mc-login", "下行 RUDP → TCP 必须原样搬运");
        let mut second = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let mut probe = [0u8; 1];
        assert_eq!(second.read(&mut probe).unwrap(), 0, "bridge.ts:22 只允许一条游戏连接");
        guest.bridge.stop();
        assert!(guest.bridge.is_stopped());
        assert!(!rc.is_connected(), "stop 必须关掉 RUDP（bridge.ts:18）");
    }

    #[test]
    fn host_dials_back_and_replays_the_first_frame() {
        let (rc, peer) = conn_pair();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        peer.write(b"kick").unwrap();
        let handle = {
            let rc = rc.clone();
            std::thread::spawn(move || start_host_lazy_bridge(rc, port, None))
        };
        let (mut game, _) = listener.accept().unwrap();
        let mut first = [0u8; 4];
        game.read_exact(&mut first).unwrap();
        assert_eq!(&first, b"kick", "bridge.ts:41 的首包必须先交给本地游戏");
        game.write_all(b"game-in").unwrap();
        assert_eq!(read_some(&peer, 7, 6), b"game-in", "上行未丢");
        peer.write(b"game-out").unwrap();
        let mut out = [0u8; 8];
        game.read_exact(&mut out).unwrap();
        assert_eq!(&out, b"game-out", "下行未丢");
        drop(game);
        handle.join().unwrap();
        assert!(!rc.is_connected(), "pump 的 finally 会关掉通路");
    }

    #[test]
    fn host_retries_with_warn_log_inside_the_dial_window() {
        let (rc, peer) = conn_pair();
        let dead = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = dead.local_addr().unwrap().port();
        drop(dead);
        peer.write(b"first").unwrap();
        let seen: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let log: LogFn = {
            let seen = seen.clone();
            Some(Arc::new(move |level: &str, message: &str| {
                seen.lock().unwrap().push((level.to_string(), message.to_string()));
            }) as Arc<dyn Fn(&str, &str) + Send + Sync>)
        };
        let handle = {
            let rc = rc.clone();
            std::thread::spawn(move || start_host_lazy_bridge(rc, port, log))
        };
        let mut got: Vec<(String, String)> = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        while got.is_empty() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
            got = seen.lock().unwrap().clone();
        }
        assert!(!got.is_empty(), "本地端口连不上时必须走 netLog('warn', …)");
        assert_eq!(got[0].0, "warn", "bridge.ts:42 固定 warn 级别");
        assert!(!got[0].1.is_empty(), "日志要带上错误原因");
        rc.close();
        handle.join().unwrap();
    }

    #[test]
    fn watchdog_reports_stopped_before_the_idle_window() {
        let (rc, _peer) = conn_pair();
        let bridge = TcpBridge::new(rc.clone(), None);
        assert!(bridge.idle_ms() < BRIDGE_IDLE_TIMEOUT_MS);
        assert!(!bridge.watchdog_once());
        bridge.stop();
        assert!(bridge.watchdog_once(), "bridge.ts:32 已停即刻判定超时");
    }

    #[test]
    fn pump_relay_copies_both_directions_and_stop_closes_both() {
        let (a, a_peer) = conn_pair();
        let (b, b_peer) = conn_pair();
        let stop = pump_relay(a.clone(), b.clone());
        a_peer.write(b"left").unwrap();
        assert_eq!(read_some(&b_peer, 4, 6), b"left");
        b_peer.write(b"right").unwrap();
        assert_eq!(read_some(&a_peer, 5, 6), b"right");
        stop();
        assert!(!a.is_connected() && !b.is_connected(), "bridge.ts:48 stop 要关两端");
        stop();
    }

    #[derive(Clone)]
    struct Captured {
        line: String,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    }

    fn header_value(item: &Captured, name: &str) -> String {
        item.headers.iter().find(|(k, _)| k.as_str() == name).map(|(_, v)| v.clone()).unwrap_or_default()
    }

    fn fake_server() -> (u16, Arc<Mutex<Vec<Captured>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let captured: Arc<Mutex<Vec<Captured>>> = Arc::new(Mutex::new(Vec::new()));
        let late = captured.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let _ = stream.set_read_timeout(Some(Duration::from_millis(800)));
                let mut buffer: Vec<u8> = Vec::new();
                let mut chunk = [0u8; 4096];
                let mut head_end: Option<usize> = None;
                let mut broken = false;
                while !broken && head_end.is_none() {
                    match stream.read(&mut chunk) {
                        Ok(0) => broken = true,
                        Ok(n) => {
                            buffer.extend_from_slice(&chunk[..n]);
                            if let Some(pos) = buffer.windows(4).position(|w| w == b"\r\n\r\n".as_slice()) {
                                head_end = Some(pos + 4);
                            }
                        }
                        Err(_) => broken = true,
                    }
                }
                let Some(head_end) = head_end else { continue };
                let head = String::from_utf8_lossy(&buffer[..head_end]).to_string();
                let want = head
                    .lines()
                    .filter_map(|l| l.split_once(':'))
                    .find(|(k, _)| k.trim().eq_ignore_ascii_case("content-length"))
                    .and_then(|(_, v)| v.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                let mut body = buffer[head_end..].to_vec();
                while body.len() < want && !broken {
                    match stream.read(&mut chunk) {
                        Ok(0) => broken = true,
                        Ok(n) => body.extend_from_slice(&chunk[..n]),
                        Err(_) => broken = true,
                    }
                }
                body.truncate(want);
                let mut lines = head.lines();
                let line = lines.next().unwrap_or_default().to_string();
                let headers = lines
                    .filter_map(|item| item.split_once(':').map(|(k, v)| (k.trim().to_lowercase(), v.trim().to_string())))
                    .collect();
                late.lock().unwrap().push(Captured { line, headers, body });
                let reply = "{\"success\": true}";
                let _ = stream.write_all(
                    format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", reply.len(), reply).as_bytes(),
                );
                let _ = stream.flush();
            }
        });
        (port, captured)
    }

    fn take_all(captured: &Arc<Mutex<Vec<Captured>>>) -> Vec<Captured> {
        std::mem::take(&mut *captured.lock().unwrap())
    }

    fn drain(captured: &Arc<Mutex<Vec<Captured>>>, want: usize, secs: u64) -> Vec<Captured> {
        let mut items: Vec<Captured> = take_all(captured);
        let deadline = Instant::now() + Duration::from_secs(secs);
        while items.len() < want && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
            items.extend(take_all(captured));
        }
        items
    }

    fn upload_requests(items: &[Captured]) -> Vec<Captured> {
        items.iter().filter(|i| i.line.contains("route=%2Flog%2Fupload")).cloned().collect()
    }

    #[test]
    fn recorded_line_keeps_iso_prefix_level_and_redaction() {
        let log = ConnectionLog::new(|| "http://127.0.0.1:1/".to_string(), || false, || vec!["TOPSECRET".to_string()], "1.1.21");
        log.record("warn", "加入 TOPSECRET https://h/p?access_token=abc12345&t=1 --username Steve");
        let st = log.inner.state.lock().unwrap();
        assert_eq!(st.lines.len(), 1);
        let line = &st.lines[0];
        let shape = regex::Regex::new(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z \[warn\] ").unwrap();
        assert!(shape.is_match(line), "行首必须是 ISO8601 毫秒 + [级别]：{line}");
        assert!(!line.contains("TOPSECRET"), "已知密钥必须先整体替换");
        assert!(line.contains("access_token=<redacted>"));
        assert!(line.contains("--username <redacted-user>"));
    }

    #[test]
    fn line_cap_and_utf16_truncation_match_js_slice() {
        let log = ConnectionLog::new(|| "http://127.0.0.1:1/".to_string(), || false, || Vec::new(), "1.1.21");
        for i in 0..1002 {
            log.record("info", &format!("#{i}"));
        }
        let mut st = log.inner.state.lock().unwrap();
        assert_eq!(st.lines.len(), LOG_LINE_KEEP, "connectionLog.ts:15 的 shift 上限");
        assert!(st.lines[0].contains("#2"), "最旧两行应被丢掉");
        assert!(st.lines[LOG_LINE_KEEP - 1].contains("#1001"));
        st.lines.clear();
        drop(st);
        let text = format!("{}😀😀", "x".repeat(1966));
        log.record("info", &text);
        let kept = log.inner.state.lock().unwrap().lines[0].clone();
        assert_eq!(kept.chars().count(), 1999, "按 UTF-16 码元截到 2000");
        assert_eq!(kept.chars().filter(|c| *c == '😀').count(), 1, "半个代理对不保留");
    }

    #[test]
    fn upload_posts_zlib_compatible_gzip_with_every_header() {
        let (port, captured) = fake_server();
        let log = ConnectionLog::new(move || format!("http://127.0.0.1:{port}/log"), || true, || Vec::new(), "1.1.21");
        log.start("ABCDEF", true);
        log.state("failed", "通路断开");
        log.stop(false);
        let items = drain(&captured, 2, 6);
        let status = items.iter().find(|i| i.line.contains("route=%2Flog%2Fstatus")).expect("/log/status 走 route 查询串");
        assert_eq!(header_value(status, "content-type"), "application/json");
        assert_eq!(status.body, br#"{"code":"ABCDEF","role":"host","enabled":true}"#.to_vec());
        let uploads = upload_requests(&items);
        assert_eq!(uploads.len(), 1, "失败后只补传一次");
        let upload = &uploads[0];
        assert!(upload.line.starts_with("POST "), "必须是 POST");
        assert_eq!(header_value(upload, "content-type"), "application/octet-stream");
        assert_eq!(header_value(upload, "accept"), "application/json");
        assert_eq!(header_value(upload, "user-agent"), "KAMUCL-App/1.1.21");
        assert_eq!(header_value(upload, "x-voxlink-version"), "1.1.21");
        assert_eq!(header_value(upload, "x-log-name"), "KAMUCL");
        assert_eq!(header_value(upload, "x-log-version"), "1.1.21");
        assert_eq!(header_value(upload, "x-log-role"), "host");
        assert_eq!(header_value(upload, "x-log-code"), "ABCDEF");
        assert!(regex::Regex::new(r"^\d+$").unwrap().is_match(&header_value(upload, "x-log-duration-ms")));
        assert_eq!(header_value(upload, "x-log-sha256"), sha256_hex(&upload.body), "摘要算的是 gzip 后的字节");
        assert_eq!(
            &upload.body[..10],
            [0x1f, 0x8b, 0x08, 0x00, 0, 0, 0, 0, 0x00, LOG_GZIP_OS_WINDOWS].as_slice(),
            "gzip 头要逐字节等于 zlib.gzipSync 在 Windows 上的默认输出"
        );
        let mut decoder = GzDecoder::new(upload.body.as_slice());
        let mut plain = String::new();
        decoder.read_to_string(&mut plain).unwrap();
        let joined = log.inner.state.lock().unwrap().lines.join("\n");
        assert_eq!(plain, joined, "正文必须是无尾换行的 join('\\n')");
        let lines: Vec<&str> = plain.split('\n').collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].ends_with("[info] 联机会话开始"));
        assert!(lines[1].ends_with("[failed] 通路断开"));
        assert!(log.inner.state.lock().unwrap().uploaded);
        log.stop(false);
        std::thread::sleep(Duration::from_millis(200));
        assert!(upload_requests(&take_all(&captured)).is_empty(), "uploaded 之后不再重复上传");
    }

    #[test]
    fn disabled_diagnostics_never_reach_the_socket() {
        let (port, captured) = fake_server();
        let log = ConnectionLog::new(move || format!("http://127.0.0.1:{port}/"), || false, || Vec::new(), "1.1.21");
        log.start("ABCDEF", false);
        log.state("failed", "通路断开");
        log.stop(false);
        log.upload(LOG_STOP_TIMEOUT_MS);
        log.stop(true);
        std::thread::sleep(Duration::from_millis(300));
        assert!(captured.lock().unwrap().is_empty(), "uploadDiagnostics 关闭时一条请求都不能发出去");
        assert_ne!(log.inner.timer_seq.load(Ordering::SeqCst), 0, "start/state 都应动过定时器");
    }

    #[test]
    fn connected_session_reschedules_instead_of_uploading() {
        let (port, captured) = fake_server();
        let log = ConnectionLog::new(move || format!("http://127.0.0.1:{port}/"), || true, || Vec::new(), "1.1.21");
        log.start("ABCDEF", true);
        log.state("success", "连接成功，请在游戏中输入下方地址");
        let seq_before = log.inner.timer_seq.load(Ordering::SeqCst);
        log.upload(500);
        assert!(!log.inner.state.lock().unwrap().uploaded, "刚连通不该补传");
        assert_ne!(log.inner.timer_seq.load(Ordering::SeqCst), seq_before, "已连通要改排 15 秒（:35）");
        let items = drain(&captured, 1, 6);
        assert!(upload_requests(&items).is_empty(), "这条路径不能产生上传请求");
        log.stop(false);
        assert!(upload_requests(&take_all(&captured)).is_empty());
    }
}
