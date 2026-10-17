//! VoxLink 信令会话（对应原版 voxlink/session.ts 113 行 + api.ts 的 HTTP 兜底）
//! 协议来源：SignalingWsTransport.java / RoomManager.java, VoxLink c475faa9。
//!
//! 线程模型：WS 收发只在 supervisor 的 tokio 任务里；调用方（心跳/轮询循环、引擎线程）
//! 用 std 通道阻塞等回包，超时语义与原版 setTimeout(6000) 一致。

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async_with_config, MaybeTlsStream, WebSocketStream};

pub const HANDSHAKE_TIMEOUT_MS: u64 = 3000;
pub const MAX_PAYLOAD: usize = 4 << 20;
pub const PING_INTERVAL_MS: u64 = 15000;
pub const WATCHDOG_INTERVAL_MS: u64 = 10000;
pub const STALE_FRAME_MS: u64 = 35000;
pub const RESPONSE_TIMEOUT_MS: u64 = 6000;
pub const BACKOFF_MS: [u64; 3] = [10000, 30000, 60000];
pub const SEEN_LIMIT: usize = 2048;
pub const HEARTBEAT_BASE_MS: u64 = 5000;
pub const POLL_INTERVAL_MS: u64 = 1000;
pub const DEGRADED_AFTER_FAILURES: u32 = 8;
pub const DEGRADED_DELAY_MS: u64 = 30000;
pub const HTTP_TIMEOUT_MS: u64 = 10_000;

pub const FATAL_CODES: [&str; 5] = [
    "ROOM_NOT_FOUND",
    "ROOM_EXPIRED",
    "ROOM_CLOSED",
    "ROOM_EVICTED",
    "INVALID_TOKEN",
];

pub fn now_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or_default()
}

/// 对应 session.ts:55：http→ws / https→wss，pathname 强制 /ws，search 清空
pub fn signaling_url(base: &str) -> Option<String> {
    let parsed = url::Url::parse(base).ok()?;
    let scheme = match parsed.scheme() {
        "https" => "wss",
        "http" => "ws",
        _ => return None,
    };
    if parsed.host_str().unwrap_or_default().is_empty() {
        return None;
    }
    let port = parsed.port().map(|p| format!(":{}", p)).unwrap_or_default();
    Some(format!("{}://{}{}/ws", scheme, parsed.host_str()?, port))
}

#[derive(Clone, Debug, PartialEq)]
pub struct SignalMsg {
    pub id: Option<String>,
    pub from: String,
    pub rtype: String,
    pub data: Value,
    pub timestamp: f64,
    pub to: Option<String>,
}

/// JS 的 Number→String（整数不带 .0）
pub fn fmt_number(v: f64) -> String {
    if !v.is_finite() {
        return "NaN".into();
    }
    if v == v.trunc() && v.abs() < 9.007199254740992e15 {
        format!("{}", v as i64)
    } else {
        format!("{}", v)
    }
}

/// 对应 session.ts:44 的去重键：有 id 用 id，否则 from|type|timestamp|JSON(data)
pub fn signal_key(signal: &SignalMsg) -> String {
    match &signal.id {
        Some(id) => id.clone(),
        None => format!(
            "{}|{}|{}|{}",
            signal.from,
            signal.rtype,
            fmt_number(signal.timestamp),
            signal.data.to_string()
        ),
    }
}

/// 去重集合：超过 2048 按插入序丢最旧（对应 session.ts:46）
#[derive(Default)]
pub struct SeenSet {
    items: HashSet<String>,
    order: VecDeque<String>,
}

impl SeenSet {
    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    pub fn contains(&self, key: &str) -> bool {
        self.items.contains(key)
    }

    /// true 表示首次见到（需要下发）
    pub fn insert(&mut self, key: String) -> bool {
        if !self.items.insert(key.clone()) {
            return false;
        }
        self.order.push_back(key);
        while self.order.len() > SEEN_LIMIT {
            if let Some(old) = self.order.pop_front() {
                self.items.remove(&old);
            }
        }
        true
    }
}

/// 心跳间隔：服务端 heartbeatInterval（秒）钳到 1..30 秒，缺省 5 秒（session.ts:110）
pub fn heartbeat_delay_ms(info: Option<&Value>) -> u64 {
    let raw = info
        .and_then(|v| v.get("heartbeatInterval"))
        .and_then(|v| v.as_f64());
    match raw {
        Some(sec) if sec.is_finite() && sec > 0.0 => ((sec * 1000.0) as u64).clamp(1000, 30000),
        _ => HEARTBEAT_BASE_MS,
    }
}

pub fn is_fatal(code: &str, status: u16) -> bool {
    FATAL_CODES.contains(&code) || status == 410
}

/// 断线重连退避：[10000,30000,60000] 之后停在末位（session.ts:66）
pub fn backoff_wait(attempt: usize) -> u64 {
    BACKOFF_MS[attempt.min(2)]
}

#[derive(Clone, Debug, PartialEq)]
pub struct ApiFailure {
    pub code: String,
    pub message: String,
    pub status: u16,
}

impl ApiFailure {
    pub fn plain(message: &str) -> Self {
        ApiFailure {
            code: String::new(),
            message: message.to_string(),
            status: 0,
        }
    }

    /// APIError 的 message 本身就是 `"<code>: <message>"`（api.ts:4），普通 Error 只有文案
    pub fn text(&self) -> String {
        if self.code.is_empty() {
            self.message.clone()
        } else {
            format!("{}: {}", self.code, self.message)
        }
    }
}

/// 对应 session.ts:39-50：过滤缺 type 或非对象 data 的信号
pub fn parse_signals(value: &Value) -> Vec<SignalMsg> {
    let mut out = Vec::new();
    let Some(list) = value.get("s").and_then(|v| v.as_array()) else {
        return out;
    };
    for item in list {
        let rtype = match item.get("type").and_then(|v| v.as_str()) {
            Some(t) => t.to_string(),
            None => continue,
        };
        if item.get("data").and_then(|v| v.as_object()).is_none() {
            continue;
        }
        out.push(SignalMsg {
            id: item.get("id").filter(|v| !v.is_null()).map(|v| match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            }),
            from: item
                .get("from")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            rtype,
            data: item.get("data").cloned().unwrap_or(Value::Null),
            timestamp: item
                .get("timestamp")
                .and_then(|v| v.as_f64())
                .unwrap_or_default(),
            to: item.get("to").and_then(|v| v.as_str()).map(str::to_string),
        });
    }
    out
}

pub fn signal_to_json(signal: &SignalMsg) -> Value {
    json!({
        "id": signal.id.clone().map(Value::String).unwrap_or(Value::Null),
        "from": signal.from,
        "type": signal.rtype,
        "data": signal.data,
        "timestamp": signal.timestamp,
        "to": signal.to.clone().map(Value::String).unwrap_or(Value::Null),
    })
}

enum Outbound {
    Call {
        id: u64,
        frame: String,
        reply: Sender<Result<Value, ApiFailure>>,
    },
    Shutdown,
}

#[derive(Default)]
struct Shared {
    finished: bool,
    degraded: bool,
    since: f64,
    backoff: usize,
    reconnect_at: f64,
    next_id: u64,
    sequence: u64,
    ws_open: bool,
    seen: SeenSet,
}

pub struct SessionSink {
    pub event: Sender<Value>,
    pub net_log: Sender<(String, String)>,
}

struct Supervisor {
    shared: Arc<Mutex<Shared>>,
    base_url: Arc<dyn Fn() -> String + Send + Sync>,
    sink: Arc<Mutex<Option<SessionSink>>>,
    payload_base: Value,
}

#[derive(Clone)]
pub struct VoxlinkSession {
    supervisor: Arc<Supervisor>,
    out: Arc<Mutex<Option<Sender<Outbound>>>>,
    http: reqwest::blocking::Client,
}

impl VoxlinkSession {
    pub fn new(
        code: String,
        token: String,
        is_host: bool,
        base_url: Arc<dyn Fn() -> String + Send + Sync>,
        version: String,
    ) -> Self {
        VoxlinkSession {
            supervisor: Arc::new(Supervisor {
                shared: Arc::new(Mutex::new(Shared {
                    since: now_ms(),
                    next_id: 1,
                    ..Default::default()
                })),
                base_url,
                sink: Arc::new(Mutex::new(None)),
                payload_base: json!({ "code": code, "token": token, "isHost": is_host }),
            }),
            out: Arc::new(Mutex::new(None)),
            http: reqwest::blocking::Client::builder()
                .timeout(Duration::from_millis(HTTP_TIMEOUT_MS))
                .user_agent(format!("WUHUICL-App/{}", version))
                .default_headers({
                    let mut h = reqwest::header::HeaderMap::new();
                    h.insert(
                        "X-VoxLink-Version",
                        reqwest::header::HeaderValue::from_str(&version).expect("版本号可用"),
                    );
                    h
                })
                .build()
                .expect("HTTP 客户端初始化失败"),
        }
    }

    pub fn attach(&self, sink: SessionSink) {
        *self.supervisor.sink.lock().unwrap() = Some(sink);
    }

    pub fn is_done(&self) -> bool {
        self.supervisor.shared.lock().unwrap().finished
    }

    pub fn since(&self) -> f64 {
        self.supervisor.shared.lock().unwrap().since
    }

    fn emit(&self, data: Value) {
        if let Some(s) = self.supervisor.sink.lock().unwrap().as_ref() {
            let _ = s.event.send(data);
        }
    }

    fn log(&self, level: &str, message: &str) {
        if let Some(s) = self.supervisor.sink.lock().unwrap().as_ref() {
            let _ = s.net_log.send((level.to_string(), message.to_string()));
        }
    }

    fn payload(&self, extra: &Value) -> Value {
        let mut base = self.supervisor.payload_base.clone();
        if let (Some(target), Some(map)) = (base.as_object_mut(), extra.as_object()) {
            for (k, v) in map {
                target.insert(k.clone(), v.clone());
            }
        }
        base
    }

    /// 对应 session.ts:97 request()：WS 已开走 WS，否则退回 HTTP 同一条 route/body
    pub fn request(&self, route: &str, extra: &Value) -> Result<Value, ApiFailure> {
        if self.is_done() {
            return Err(ApiFailure::plain("会话已取消"));
        }
        let body = self.payload(extra);
        let sender = self.out.lock().unwrap().clone();
        let open = self.supervisor.shared.lock().unwrap().ws_open;
        if let Some(tx) = sender.filter(|_| open) {
            let (reply_tx, reply_rx) = std::sync::mpsc::channel();
            let id = {
                let mut st = self.supervisor.shared.lock().unwrap();
                let id = st.next_id;
                st.next_id = st.next_id.wrapping_add(1);
                id
            };
            let frame =
                json!({ "id": id, "route": route, "method": "POST", "body": body }).to_string();
            if tx
                .send(Outbound::Call {
                    id,
                    frame,
                    reply: reply_tx,
                })
                .is_ok()
            {
                return match reply_rx.recv_timeout(Duration::from_millis(RESPONSE_TIMEOUT_MS)) {
                    Ok(result) => result,
                    Err(_) => Err(ApiFailure::plain("信令响应超时")),
                };
            }
        }
        self.http_post(route, &body)
    }

    /// 对应 api.ts ApiClient.do() 的 POST 分支（信封、4MB 上限、错误码口径一致）
    pub fn http_post(&self, route: &str, body: &Value) -> Result<Value, ApiFailure> {
        let base = (self.supervisor.base_url)();
        let mut url = match url::Url::parse(&base) {
            Ok(u)
                if matches!(u.scheme(), "http" | "https")
                    && !u.host_str().unwrap_or_default().is_empty() =>
            {
                u
            }
            _ => {
                return Err(ApiFailure {
                    code: "NETWORK".into(),
                    message: "服务器地址无效".into(),
                    status: 0,
                })
            }
        };
        url.query_pairs_mut().append_pair("route", route);
        let reply = self
            .http
            .post(url.as_str())
            .header("Content-Type", "application/json")
            .body(body.to_string())
            .send();
        let reply = match reply {
            Ok(r) => r,
            Err(e) => {
                return Err(ApiFailure {
                    code: "NETWORK".into(),
                    message: e.to_string(),
                    status: 0,
                })
            }
        };
        let status = reply.status().as_u16();
        let bytes = match reply.bytes() {
            Ok(b) => b,
            Err(e) => {
                return Err(ApiFailure {
                    code: "NETWORK".into(),
                    message: e.to_string(),
                    status,
                })
            }
        };
        if bytes.len() > MAX_PAYLOAD {
            return Err(ApiFailure {
                code: "RESPONSE_LIMIT".into(),
                message: "服务器响应过大".into(),
                status,
            });
        }
        let envelope: Value = match serde_json::from_slice(&bytes) {
            Ok(v) => v,
            Err(_) => {
                return Err(ApiFailure {
                    code: format!("HTTP_{}", status),
                    message: "服务器未返回有效 JSON".into(),
                    status,
                })
            }
        };
        if !(200..=299).contains(&status)
            || envelope.get("success").and_then(|v| v.as_bool()) != Some(true)
        {
            let code = envelope
                .get("error")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or(if status == 429 {
                    "RATE_LIMITED".to_string()
                } else {
                    format!("HTTP_{}", status)
                });
            let message = envelope
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("请求失败")
                .to_string();
            return Err(ApiFailure {
                code,
                message,
                status,
            });
        }
        Ok(envelope.get("data").cloned().unwrap_or(Value::Null))
    }

    pub fn stop(&self) {
        {
            let mut st = self.supervisor.shared.lock().unwrap();
            if st.finished {
                return;
            }
            st.finished = true;
            st.ws_open = false;
        }
        if let Some(tx) = self.out.lock().unwrap().as_ref() {
            let _ = tx.send(Outbound::Shutdown);
        }
    }

    pub fn fatal(&self, message: &str) {
        if self.is_done() {
            return;
        }
        self.stop();
        self.emit(json!({ "type": "session:state", "state": "closed", "message": message }));
        self.emit(json!({ "type": "closed", "message": message }));
    }

    /// 对应 session.ts:108 loop(heartbeat=true)
    pub fn run_heartbeat_loop(&self) {
        let mut delay = HEARTBEAT_BASE_MS;
        let mut failures = 0u32;
        loop {
            if self.is_done() {
                return;
            }
            let seq = {
                let mut st = self.supervisor.shared.lock().unwrap();
                st.sequence += 1;
                st.sequence
            };
            let extra = json!({ "seq": seq, "currentInterval": delay as f64 / 1000.0, "natType": "unknown", "load": 0, "peerLatency": {} });
            match self.request("/room/heartbeat", &extra) {
                Ok(result) => {
                    if self.is_done() {
                        return;
                    }
                    failures = 0;
                    let recovered = {
                        let mut st = self.supervisor.shared.lock().unwrap();
                        if st.degraded {
                            st.degraded = false;
                            true
                        } else {
                            false
                        }
                    };
                    if recovered {
                        self.emit(json!({ "type": "signaling:recovered" }));
                    }
                    let next = heartbeat_delay_ms(Some(&result));
                    delay = next;
                    self.emit(json!({ "type": "roomInfo", "info": result }));
                }
                Err(error) => {
                    if self.is_done() {
                        return;
                    }
                    self.log("warn", &error.text());
                    if is_fatal(&error.code, error.status) {
                        self.fatal(&error.message);
                        return;
                    }
                    failures += 1;
                    if failures >= DEGRADED_AFTER_FAILURES {
                        delay = DEGRADED_DELAY_MS;
                        let enter = {
                            let mut st = self.supervisor.shared.lock().unwrap();
                            if st.degraded {
                                false
                            } else {
                                st.degraded = true;
                                true
                            }
                        };
                        if enter {
                            self.emit(json!({ "type": "signaling:degraded", "message": "信令暂时不可用，已降低心跳频率；游戏通路保持运行" }));
                        }
                    }
                }
            }
            self.wait(delay);
        }
    }

    /// 对应 session.ts:110 loop(heartbeat=false)
    pub fn run_poll_loop(&self) {
        loop {
            if self.is_done() {
                return;
            }
            if self.supervisor.shared.lock().unwrap().degraded {
                self.wait(POLL_INTERVAL_MS);
                continue;
            }
            let extra = json!({ "since": self.since() });
            match self.request("/signal/poll", &extra) {
                Ok(result) => {
                    if self.is_done() {
                        return;
                    }
                    accept_signals(&self.supervisor, &result);
                }
                Err(error) => {
                    if self.is_done() {
                        return;
                    }
                    self.log("warn", &error.text());
                    if is_fatal(&error.code, error.status) {
                        self.fatal(&error.message);
                        return;
                    }
                }
            }
            self.wait(POLL_INTERVAL_MS);
        }
    }

    fn wait(&self, ms: u64) {
        let deadline = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < deadline {
            if self.is_done() {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// 对应 session.ts:112 run()：监督任务 + 心跳 + 轮询三线并发
    pub fn run(&self) {
        let (tx, rx) = std::sync::mpsc::channel::<Outbound>();
        *self.out.lock().unwrap() = Some(tx);
        let supervisor = self.supervisor.clone();
        std::thread::spawn(move || supervise(supervisor, rx));
        let poll = self.clone();
        let poll_thread = std::thread::spawn(move || poll.run_poll_loop());
        self.run_heartbeat_loop();
        let _ = poll_thread.join();
    }
}

fn accept_signals(ctx: &Arc<Supervisor>, value: &Value) {
    let mut fresh = Vec::new();
    {
        let mut st = ctx.shared.lock().unwrap();
        for signal in parse_signals(value) {
            let key = signal_key(&signal);
            if st.seen.insert(key) {
                fresh.push(signal_to_json(&signal));
            }
        }
        if let Some(ts) = value.get("ts").and_then(|v| v.as_f64()) {
            if ts.is_finite() {
                st.since = st.since.max(ts);
            }
        }
    }
    if let Some(s) = ctx.sink.lock().unwrap().as_ref() {
        for signal in fresh {
            let _ = s
                .event
                .send(json!({ "type": "engineSignal", "signal": signal }));
        }
    }
}

#[derive(PartialEq)]
enum Round {
    Reconnect,
    Stopped,
}

fn supervise(ctx: Arc<Supervisor>, rx: std::sync::mpsc::Receiver<Outbound>) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(_) => return,
    };
    let (bridge_tx, mut bridge_rx) = unbounded_channel::<Outbound>();
    std::thread::spawn(move || {
        while let Ok(item) = rx.recv() {
            let stop = matches!(item, Outbound::Shutdown);
            if bridge_tx.send(item).is_err() || stop {
                return;
            }
        }
    });
    loop {
        if ctx.shared.lock().unwrap().finished {
            return;
        }
        let reconnect_at = ctx.shared.lock().unwrap().reconnect_at;
        let now = now_ms();
        if reconnect_at > now {
            std::thread::sleep(Duration::from_millis((reconnect_at - now) as u64));
        }
        let base = (ctx.base_url)();
        let Some(url) = signaling_url(&base) else {
            return;
        };
        let config = WebSocketConfig {
            max_message_size: Some(MAX_PAYLOAD),
            max_frame_size: Some(MAX_PAYLOAD),
            ..Default::default()
        };
        let connected = runtime.block_on(tokio::time::timeout(
            Duration::from_millis(HANDSHAKE_TIMEOUT_MS),
            connect_async_with_config(url.as_str(), Some(config), false),
        ));
        let session = match connected {
            Ok(Ok((stream, _response))) => stream,
            _ => {
                schedule_reconnect(&ctx, true);
                continue;
            }
        };
        {
            let mut st = ctx.shared.lock().unwrap();
            st.ws_open = true;
            st.backoff = 0;
            st.reconnect_at = 0.0;
        }
        // 对应 session.ts:77：连上先补一次轮询，补回离线期间漏掉的信号
        let since = ctx.shared.lock().unwrap().since;
        let frame = request_frame(&ctx, "/signal/poll", &json!({ "since": since }));
        let outcome = runtime.block_on(session_round(
            ctx.clone(),
            session,
            &mut bridge_rx,
            Some(frame),
        ));
        {
            let mut st = ctx.shared.lock().unwrap();
            st.ws_open = false;
            if st.finished {
                return;
            }
        }
        if outcome == Round::Stopped {
            return;
        }
        schedule_reconnect(&ctx, true);
    }
}

fn request_frame(ctx: &Arc<Supervisor>, route: &str, extra: &Value) -> (u64, String) {
    let mut body = ctx.payload_base.clone();
    if let (Some(target), Some(map)) = (body.as_object_mut(), extra.as_object()) {
        for (k, v) in map {
            target.insert(k.clone(), v.clone());
        }
    }
    let id = {
        let mut st = ctx.shared.lock().unwrap();
        let id = st.next_id;
        st.next_id = st.next_id.wrapping_add(1);
        id
    };
    (
        id,
        json!({ "id": id, "route": route, "method": "POST", "body": body }).to_string(),
    )
}

fn schedule_reconnect(ctx: &Arc<Supervisor>, unavailable: bool) {
    let mut st = ctx.shared.lock().unwrap();
    if st.finished {
        return;
    }
    let wait = if unavailable {
        backoff_wait(st.backoff)
    } else {
        0
    };
    if unavailable {
        st.backoff = (st.backoff + 1).min(2);
    }
    st.reconnect_at = now_ms() + wait as f64;
}

async fn session_round(
    ctx: Arc<Supervisor>,
    session: WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    calls: &mut UnboundedReceiver<Outbound>,
    initial: Option<(u64, String)>,
) -> Round {
    let (mut write, mut read) = session.split();
    let mut ping = tokio::time::interval(Duration::from_millis(PING_INTERVAL_MS));
    let mut watchdog = tokio::time::interval(Duration::from_millis(WATCHDOG_INTERVAL_MS));
    let mut sweep = tokio::time::interval(Duration::from_millis(500));
    let mut last_frame = Instant::now();
    let mut pending: HashMap<u64, Sender<Result<Value, ApiFailure>>> = HashMap::new();
    let mut deadlines: Vec<(Instant, u64)> = Vec::new();
    // 连上补的那次轮询没有等待方，回包要当信号处理而不是丢掉
    let mut signal_only: std::collections::HashSet<u64> = std::collections::HashSet::new();
    if let Some((id, frame)) = initial {
        signal_only.insert(id);
        deadlines.push((
            Instant::now() + Duration::from_millis(RESPONSE_TIMEOUT_MS),
            id,
        ));
        if write.send(Message::Text(frame)).await.is_err() {
            return Round::Reconnect;
        }
    }
    loop {
        tokio::select! {
            frame = read.next() => {
                match frame {
                    Some(Ok(Message::Text(text))) => { last_frame = Instant::now(); handle_text(&ctx, text.as_str(), &mut pending, &mut signal_only); }
                    Some(Ok(Message::Binary(bytes))) => { last_frame = Instant::now(); handle_text(&ctx, &String::from_utf8_lossy(&bytes), &mut pending, &mut signal_only); }
                    Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) => { last_frame = Instant::now(); }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return Round::Reconnect,
                    Some(Ok(_)) => {}
                }
            }
            call = calls.recv() => {
                match call {
                    Some(Outbound::Shutdown) => { let _ = write.send(Message::Close(None)).await; return Round::Stopped; }
                    Some(Outbound::Call { id, frame, reply }) => {
                        signal_only.remove(&id);
                        pending.insert(id, reply);
                        deadlines.push((Instant::now() + Duration::from_millis(RESPONSE_TIMEOUT_MS), id));
                        if write.send(Message::Text(frame)).await.is_err() {
                            return Round::Reconnect;
                        }
                    }
                    None => return Round::Stopped,
                }
            }
            _ = ping.tick() => {
                if write.send(Message::Ping(Vec::new())).await.is_err() {
                    return Round::Reconnect;
                }
            }
            _ = watchdog.tick() => {
                if last_frame.elapsed() > Duration::from_millis(STALE_FRAME_MS) {
                    return Round::Reconnect;
                }
            }
            _ = sweep.tick() => {
                let now = Instant::now();
                let mut expired = Vec::new();
                deadlines.retain(|(at, id)| if *at <= now { expired.push(*id); false } else { true });
                for id in expired {
                    if let Some(tx) = pending.remove(&id) {
                        let _ = tx.send(Err(ApiFailure::plain("信令响应超时")));
                    }
                }
            }
        }
    }
}

/// 对应 session.ts:83-93：id===0 是服务端推送，否则结算挂起请求
fn handle_text(
    ctx: &Arc<Supervisor>,
    text: &str,
    pending: &mut HashMap<u64, Sender<Result<Value, ApiFailure>>>,
    signal_only: &mut std::collections::HashSet<u64>,
) {
    if text.as_bytes().len() > MAX_PAYLOAD {
        return;
    }
    let Ok(frame) = serde_json::from_str::<Value>(text) else {
        return;
    };
    match frame.get("id").and_then(|v| v.as_u64()) {
        Some(0) => {
            if let Some(data) = frame.get("data") {
                accept_signals(ctx, data);
            }
            return;
        }
        Some(id) => {
            if signal_only.remove(&id) {
                if let Some(data) = frame.get("data") {
                    if frame.get("success").and_then(|v| v.as_bool()) == Some(true) {
                        accept_signals(ctx, data);
                    }
                }
                return;
            }
            let Some(reply) = pending.remove(&id) else {
                return;
            };
            if frame.get("success").and_then(|v| v.as_bool()) == Some(true) {
                let _ = reply.send(Ok(frame.get("data").cloned().unwrap_or(Value::Null)));
            } else {
                let code = frame
                    .get("error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("SIGNAL")
                    .to_string();
                let message = frame
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("请求失败")
                    .to_string();
                let _ = reply.send(Err(ApiFailure {
                    code,
                    message,
                    status: 0,
                }));
            }
        }
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ws_url_follows_original_derivation() {
        // 对应 session.ts:55：协议换壳、pathname 强制 /ws、search 丢弃
        assert_eq!(
            signaling_url("https://p2p.wuhui.icu/api?x=1").unwrap(),
            "wss://p2p.wuhui.icu/ws"
        );
        assert_eq!(
            signaling_url("http://127.0.0.1:8080/").unwrap(),
            "ws://127.0.0.1:8080/ws"
        );
        assert_eq!(
            signaling_url("https://h.example:9443/deep/path").unwrap(),
            "wss://h.example:9443/ws"
        );
        assert!(signaling_url("ftp://h.example").is_none());
        assert!(signaling_url("不是地址").is_none());
    }

    #[test]
    fn signal_key_prefers_id_and_falls_back_to_tuple() {
        let with_id = SignalMsg {
            id: Some("7".into()),
            from: "a".into(),
            rtype: "t".into(),
            data: json!({"k":1}),
            timestamp: 5.0,
            to: None,
        };
        assert_eq!(signal_key(&with_id), "7");
        let no_id = SignalMsg {
            id: None,
            from: "a".into(),
            rtype: "t".into(),
            data: json!({"k":1}),
            timestamp: 5.0,
            to: None,
        };
        assert_eq!(signal_key(&no_id), "a|t|5|{\"k\":1}");
        // 整数时间戳不能写成 5.0（那是 Rust 默认格式化，与 JS 不同）
        let frac = SignalMsg {
            id: None,
            from: "a".into(),
            rtype: "t".into(),
            data: json!(null),
            timestamp: 5.5,
            to: None,
        };
        assert_eq!(signal_key(&frac), "a|t|5.5|null");
    }

    #[test]
    fn parse_signals_filters_like_original() {
        let value = json!({ "s": [
            { "type": "offer", "data": { "sdp": "x" }, "from": "a", "timestamp": 1 },
            { "data": { "sdp": "y" }, "from": "a", "timestamp": 2 },
            { "type": "ice", "data": "不是对象", "from": "a", "timestamp": 3 },
            { "type": "ok", "data": {}, "from": "a", "timestamp": 4 }
        ]});
        let parsed = parse_signals(&value);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].rtype, "offer");
        assert_eq!(parsed[1].rtype, "ok");
        assert!(parse_signals(&json!({ "s": "不是数组" })).is_empty());
        assert!(parse_signals(&json!({})).is_empty());
    }

    #[test]
    fn seen_set_dedups_and_evicts_oldest_at_2048() {
        let mut seen = SeenSet::default();
        assert!(seen.insert("a".into()));
        assert!(!seen.insert("a".into()));
        for i in 0..3000 {
            seen.insert(format!("k{}", i));
        }
        assert_eq!(seen.len(), SEEN_LIMIT);
        assert!(!seen.contains("k0"), "最旧的应当被淘汰");
        assert!(seen.contains(&format!("k{}", 2999)));
    }

    #[test]
    fn heartbeat_interval_clamps() {
        assert_eq!(heartbeat_delay_ms(None), 5000);
        assert_eq!(heartbeat_delay_ms(Some(&json!({}))), 5000);
        assert_eq!(
            heartbeat_delay_ms(Some(&json!({ "heartbeatInterval": 0 }))),
            5000
        );
        assert_eq!(
            heartbeat_delay_ms(Some(&json!({ "heartbeatInterval": 0.2 }))),
            1000
        );
        assert_eq!(
            heartbeat_delay_ms(Some(&json!({ "heartbeatInterval": 3 }))),
            3000
        );
        assert_eq!(
            heartbeat_delay_ms(Some(&json!({ "heartbeatInterval": 900 }))),
            30000
        );
    }

    #[test]
    fn backoff_and_fatal_classification() {
        assert_eq!(backoff_wait(0), 10000);
        assert_eq!(backoff_wait(1), 30000);
        assert_eq!(backoff_wait(2), 60000);
        assert_eq!(backoff_wait(9), 60000);
        assert!(is_fatal("ROOM_EXPIRED", 0));
        assert!(is_fatal("WHATEVER", 410));
        assert!(!is_fatal("NETWORK", 500));
    }

    #[test]
    fn failure_text_matches_error_shapes() {
        // APIError 的 message 本身就是 "CODE: 文案"；普通 Error 只有文案
        assert_eq!(
            ApiFailure {
                code: "ROOM_NOT_FOUND".into(),
                message: "房间不存在".into(),
                status: 404
            }
            .text(),
            "ROOM_NOT_FOUND: 房间不存在"
        );
        assert_eq!(ApiFailure::plain("信令响应超时").text(), "信令响应超时");
    }

    #[test]
    fn payload_merges_over_identity_fields() {
        let session = VoxlinkSession::new(
            "ABCDEF".into(),
            "tok".into(),
            true,
            Arc::new(|| "https://p2p.wuhui.icu".into()),
            "0.0.0".into(),
        );
        let merged = session.payload(&json!({ "since": 12, "isHost": false }));
        assert_eq!(merged["code"].as_str(), Some("ABCDEF"));
        assert_eq!(merged["token"].as_str(), Some("tok"));
        assert_eq!(merged["since"].as_i64(), Some(12));
        // 后写的字段覆盖身份字段，与 TS 展开顺序一致
        assert_eq!(merged["isHost"].as_bool(), Some(false));
    }

    #[test]
    fn stop_is_idempotent_and_blocks_requests() {
        let session = VoxlinkSession::new(
            "ABCDEF".into(),
            "tok".into(),
            false,
            Arc::new(|| "https://invalid.invalid".into()),
            "0.0.0".into(),
        );
        assert!(!session.is_done());
        session.stop();
        session.stop();
        assert!(session.is_done());
        let err = session.request("/signal/poll", &json!({})).unwrap_err();
        assert_eq!(err.text(), "会话已取消");
    }

    #[test]
    fn http_fallback_rejects_bad_server_url_without_network() {
        let session = VoxlinkSession::new(
            "ABCDEF".into(),
            "tok".into(),
            false,
            Arc::new(|| "javascript:alert(1)".into()),
            "0.0.0".into(),
        );
        let err = session.http_post("/room/list", &json!({})).unwrap_err();
        assert_eq!(err.code, "NETWORK");
        assert_eq!(err.message, "服务器地址无效");
    }
}
