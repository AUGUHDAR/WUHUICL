//! vl 工单与 TURN/TCP 隧道层：移植 KAMUCL-1.1.21 `src/main/core/voxlink/tickets.ts`（70 行）
//! 与 `src/main/core/voxlink/turnTcp.ts`（46 行）；逐行对应关系见下方 `tickets.ts:NN` /
//! `turnTcp.ts:NN` 标注。上游为 VoxLink TicketClient.java c475faa9 与
//! TurnTcpChannel.java 721c7fae，原文件 SPDX 为 LGPL-3.0-only。
//!
//! 对照表（tickets.ts）：TicketError → :9；TicketFile / SecretCodec → :11-12；validId → :13；
//! safeCount → :14；attach → :15；load / save → :20-21；list → :22；owned / secret / url →
//! :23-25；send / json → :26-35；pollOnce → :36-39；detail → :40-42；rate → :43；upload →
//! :44-55；submit → :56-66；reply → :67；retract → :68；remove → :69。常量取
//! shared/voxlinkTickets.ts:7，validateServerURL 取 api.ts:15-17。
//! 对照表（turnTcp.ts）：openTurnTcp → :5；close → :9-12；3000ms 连接超时 → :17；setNoDelay
//! → :21；127.0.0.1:0 环回双 socket → :22-27；入口组帧与背压丢弃 → :28-32；u16BE 长度前缀
//! 解帧 → :33-43；返回 {socket,target,close} → :44。
//!
//! 移植约定：
//! - 出站 JSON 一律用 `serde_json::Map` 按 TS 字面量顺序插入（项目启用 `preserve_order`），
//!   数值经 `js_num` 保持 JSON.stringify 的整数字面量（1700000000000 而非 1700000000000.0）。
//! - 入站数值统一走 `js_number` + `safe_count`，复现 `Math.max(0,Math.trunc(Number(n)||0))`。
//! - JS `string.length` 是 UTF-16 码元数，故字数上限用 `utf16_len`，不用 `chars().count()`。
//! - 原版异常分两类：`TicketError`（ticketsIpc.ts:33 走 code/message）与普通 Error /
//!   AbortError（走 OPERATION_FAILED）；此处用 `TicketFailure::{Ticket, Other}` 区分，
//!   `Other` 只保留 message 供日志使用，不参与 IPC 返回码。
//! - AbortSignal 由 `Arc<AtomicBool>` 承载（兄弟模块 stun.rs 同风格）；取消原因只用原版
//!   'TURN TCP 已取消' / '工单请求已取消' 两句文案。

use regex::Regex;
use serde::Serialize;
use serde_json::{Map, Number, Value};
use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, ToSocketAddrs, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, UNIX_EPOCH};

use crate::vl::rudp::{now_ms, RudpTarget};

/// shared/voxlinkTickets.ts:7
pub const TICKET_DESCRIPTION_MAX: usize = 10000;
pub const TICKET_MESSAGE_MAX: usize = 2000;
pub const TICKET_MESSAGE_COUNT_MAX: usize = 200;
pub const TICKET_FILES_MAX: usize = 10;
pub const TICKET_BYTES_MAX: f64 = 500.0 * 1024.0 * 1024.0;
/// tickets.ts:37 每页 50 条
const POLL_PAGE_SIZE: usize = 50;
/// tickets.ts:15 的 slice(0,10) 与 name.slice(0,255) 是字面量，不引用上面的常量
const ATTACH_LIST_MAX: usize = 10;
const ATTACH_NAME_MAX: usize = 255;
/// tickets.ts:29 响应上限 4 MiB
const RESPONSE_LIMIT: usize = 4 * 1024 * 1024;
/// tickets.ts:27 上传 30 分钟，其余 15 秒
const UPLOAD_TIMEOUT_MS: u64 = 1_800_000;
const REQUEST_TIMEOUT_MS: u64 = 15_000;
/// tickets.ts:31 retryAfter 钳位与缺省
const RETRY_AFTER_MAX: f64 = 86_400.0;
const RETRY_AFTER_DEFAULT: f64 = 600.0;
/// tickets.ts:43 十分钟三发
const RATE_WINDOW_MS: f64 = 600_000.0;
const RATE_MAX: usize = 3;
/// tickets.ts:67 同文追问间隔
const DUPLICATE_WINDOW_MS: f64 = 15_000.0;
/// tickets.ts:51 createReadStream 高水位
const UPLOAD_CHUNK: usize = 65_536;

/// turnTcp.ts:17 / :29-31 / :38 / :44
pub const TURN_TCP_CONNECT_TIMEOUT_MS: u64 = 3_000;
pub const TURN_TCP_MAX_PACKET: usize = 2_048;
pub const TURN_TCP_BACKPRESSURE_BYTES: usize = 1024 * 1024;
pub const TURN_TCP_FRAME_PREFIX: usize = 2;
pub const TURN_TCP_SHIM_ADDRESS: &str = "127.0.0.1";
pub const TURN_TCP_TIMEOUT_CONNECT: &str = "TURN TCP 连接超时";
pub const TURN_TCP_TIMEOUT_CLOSED: &str = "TURN TCP 已关闭";
pub const TURN_TCP_TIMEOUT_CANCELLED: &str = "TURN TCP 已取消";

fn id_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[\w\-]{1,128}$").expect("工单标识正则可用"))
}

fn error_code_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[A-Z_0-9]{1,80}$").expect("错误码正则可用"))
}

fn filename_strip_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new("[\"\r\n]").expect("文件名清洗正则可用"))
}

fn is_js_space(c: char) -> bool {
    c.is_whitespace() || c == '\u{FEFF}'
}

fn js_trim(value: &str) -> &str {
    value.trim_matches(is_js_space)
}

/// JS string.length：UTF-16 码元数
pub fn utf16_len(value: &str) -> usize {
    value.encode_utf16().count()
}

/// 十进制字面量：允许 1. / .5 / 1e3，拒绝 inf、nan 与多余符号
fn decimal_number(body: &str) -> Option<f64> {
    let bytes = body.as_bytes();
    let mut index = 0usize;
    let mut digits = false;
    let mut dots = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            b'0'..=b'9' => {
                digits = true;
                index += 1;
            }
            b'.' => {
                if dots > 0 {
                    return None;
                }
                dots += 1;
                index += 1;
            }
            b'e' | b'E' => {
                if !digits {
                    return None;
                }
                index += 1;
                if index < bytes.len() && (bytes[index] == b'+' || bytes[index] == b'-') {
                    index += 1;
                }
                let mut exponents = false;
                while index < bytes.len() && bytes[index].is_ascii_digit() {
                    exponents = true;
                    index += 1;
                }
                if !exponents || index != bytes.len() {
                    return None;
                }
                return body.parse::<f64>().ok();
            }
            _ => return None,
        }
    }
    if !digits || index != bytes.len() {
        return None;
    }
    body.parse::<f64>().ok()
}

/// JS Number()：undefined/null→NaN（配合下面的 `||0`）
fn js_number(value: Option<&Value>) -> f64 {
    match value {
        None | Some(Value::Null) => f64::NAN,
        Some(Value::Bool(true)) => 1.0,
        Some(Value::Bool(false)) => 0.0,
        Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(text)) => {
            let head = js_trim(text);
            if head.is_empty() {
                return 0.0;
            }
            let (sign, rest) = match head.strip_prefix('-') {
                Some(body) => (-1.0, body),
                None => (1.0, head.strip_prefix('+').unwrap_or(head)),
            };
            if rest == "Infinity" {
                return sign * f64::INFINITY;
            }
            if rest.starts_with("0x") || rest.starts_with("0X") {
                let digits = &rest[2..];
                if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
                    return f64::NAN;
                }
                return i64::from_str_radix(digits, 16)
                    .map(|v| sign * v as f64)
                    .unwrap_or(f64::NAN);
            }
            decimal_number(rest).map(|v| sign * v).unwrap_or(f64::NAN)
        }
        Some(Value::Array(list)) => match list.len() {
            0 => 0.0,
            1 => js_number(Some(&list[0])),
            _ => f64::NAN,
        },
        Some(Value::Object(_)) => f64::NAN,
    }
}

/// tickets.ts:14 `Math.max(0,Math.trunc(Number(n)||0))`
pub fn safe_count(value: Option<&Value>) -> f64 {
    let raw = js_number(value);
    let base = if raw.is_nan() || raw == 0.0 { 0.0 } else { raw };
    base.trunc().max(0.0)
}

/// JSON.stringify 的数值写法：整数值出整数字面量，Infinity 出 null
pub fn js_num(value: f64) -> Value {
    if !value.is_finite() {
        return Value::Null;
    }
    if value.trunc() == value && value.abs() < 9_007_199_254_740_992.0 {
        return Value::Number(Number::from(value as i64));
    }
    Number::from_f64(value)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}

fn map_of<const N: usize>(pairs: [(&str, Value); N]) -> Value {
    let mut map = Map::with_capacity(N);
    for (key, value) in pairs {
        map.insert(key.to_string(), value);
    }
    Value::Object(map)
}

/// api.ts:15-17 validateServerURL
fn validate_server_url(value: &str) -> bool {
    match url::Url::parse(value) {
        Ok(u) => {
            matches!(u.scheme(), "https" | "http")
                && !u.host_str().unwrap_or_default().is_empty()
                && u.username().is_empty()
                && u.password().is_none()
        }
        Err(_) => false,
    }
}

fn system_mtime_ms(meta: &std::fs::Metadata) -> f64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or_default()
}

/// tickets.ts:9
#[derive(Clone, Debug, PartialEq)]
pub struct TicketError {
    pub code: String,
    pub message: String,
    pub retry_at: Option<f64>,
}

impl TicketError {
    pub fn new(code: &str, message: &str) -> Self {
        TicketError {
            code: code.to_string(),
            message: message.to_string(),
            retry_at: None,
        }
    }

    pub fn with_retry(code: &str, message: &str, retry_at: f64) -> Self {
        TicketError {
            code: code.to_string(),
            message: message.to_string(),
            retry_at: Some(retry_at),
        }
    }
}

impl std::fmt::Display for TicketError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for TicketError {}

#[derive(Clone, Debug, PartialEq)]
pub enum TicketFailure {
    Ticket(TicketError),
    /// 原版普通 Error / AbortError：ticketsIpc.ts:33 归为 OPERATION_FAILED
    Other(String),
}

impl TicketFailure {
    pub fn ticket(code: &str, message: &str) -> Self {
        TicketFailure::Ticket(TicketError::new(code, message))
    }

    /// tickets.ts:37/58/62 的 signal.throwIfAborted()
    pub fn aborted() -> Self {
        TicketFailure::Other("This operation was aborted".to_string())
    }

    /// ticketsIpc.ts:33：非 TicketError 一律回落 OPERATION_FAILED
    pub fn code(&self) -> &str {
        match self {
            TicketFailure::Ticket(error) => error.code.as_str(),
            TicketFailure::Other(_) => "OPERATION_FAILED",
        }
    }

    pub fn message(&self) -> &str {
        match self {
            TicketFailure::Ticket(error) => error.message.as_str(),
            TicketFailure::Other(text) => text.as_str(),
        }
    }

    pub fn retry_at(&self) -> Option<f64> {
        match self {
            TicketFailure::Ticket(error) => error.retry_at,
            TicketFailure::Other(_) => None,
        }
    }
}

impl From<TicketError> for TicketFailure {
    fn from(value: TicketError) -> Self {
        TicketFailure::Ticket(value)
    }
}

pub type TicketOutcome<T> = Result<T, TicketFailure>;

/// tickets.ts:11
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketFile {
    pub path: String,
    pub name: String,
    pub size: f64,
    pub mtime: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketAttachment {
    pub name: String,
    pub size: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketMessage {
    /// 原版 `id:undefined` 经 JSON.stringify 后键消失
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub from: String,
    pub time_ms: Value,
    pub text: String,
    pub attachments: Vec<TicketAttachment>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketDetail {
    pub id: String,
    pub time_ms: Value,
    pub deleted: bool,
    pub description: String,
    pub attachments: Vec<TicketAttachment>,
    pub messages: Vec<TicketMessage>,
}

impl TicketDetail {
    /// tickets.ts:67 已用附件字节和
    pub fn used_bytes(&self) -> f64 {
        self.attachments
            .iter()
            .map(|a| a.size as f64)
            .chain(
                self.messages
                    .iter()
                    .flat_map(|m| m.attachments.iter())
                    .map(|a| a.size as f64),
            )
            .sum()
    }

    /// tickets.ts:68 倒序找最后一条有 id 的非 admin 消息
    pub fn own_last_message(&self) -> Option<&TicketMessage> {
        self.messages
            .iter()
            .rev()
            .find(|m| m.from != "admin" && m.id.as_deref().map(|v| !v.is_empty()).unwrap_or(false))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredTicket {
    pub id: String,
    pub time_ms: Value,
    pub deleted: bool,
    pub has_unread: bool,
    pub reply_count: Value,
    pub last_time_ms: Value,
    pub secret: String,
    pub server: String,
}

impl StoredTicket {
    /// tickets.ts:22 `({secret,server,...visible})=>visible`
    pub fn visible(&self) -> Value {
        map_of([
            ("id", Value::String(self.id.clone())),
            ("timeMs", self.time_ms.clone()),
            ("deleted", Value::Bool(self.deleted)),
            ("hasUnread", Value::Bool(self.has_unread)),
            ("replyCount", self.reply_count.clone()),
            ("lastTimeMs", self.last_time_ms.clone()),
        ])
    }

    fn sort_key(&self) -> f64 {
        let last = self.last_time_ms.as_f64().unwrap_or_default();
        if last != 0.0 {
            last
        } else {
            self.time_ms.as_f64().unwrap_or_default()
        }
    }
}

/// tickets.ts:12；原版 seal/open 直接抛异常，这里用 TicketError 承载
pub trait SecretCodec: Send + Sync {
    fn seal(&self, text: &str) -> Result<String, TicketError>;
    fn open(&self, text: &str) -> Result<String, TicketError>;
}

/// tickets.ts:19 的默认编解码器（原样透传）
#[derive(Clone, Copy, Debug, Default)]
pub struct PlainCodec;

impl SecretCodec for PlainCodec {
    fn seal(&self, text: &str) -> Result<String, TicketError> {
        Ok(text.to_string())
    }

    fn open(&self, text: &str) -> Result<String, TicketError> {
        Ok(text.to_string())
    }
}

pub type TicketProgress = Arc<dyn Fn(f64, f64) + Send + Sync>;

/// tickets.ts:13 `/^[\w-]{1,128}$/`：\w 只认 ASCII，故不用 Unicode \w
pub fn valid_id(value: Option<&Value>) -> bool {
    match value {
        Some(Value::String(text)) => id_re().is_match(text),
        _ => false,
    }
}

/// tickets.ts:15 附件表清洗
pub fn normalize_attachments(raw: Option<&Value>) -> Vec<TicketAttachment> {
    let Some(list) = raw.and_then(Value::as_array) else {
        return Vec::new();
    };
    list.iter()
        .take(ATTACH_LIST_MAX)
        .filter_map(|item| {
            let name = item.get("name").and_then(Value::as_str)?;
            let size = item.get("size").and_then(Value::as_f64)?;
            if size.fract() != 0.0 || size < 0.0 || size >= 9_007_199_254_740_992.0 {
                return None;
            }
            Some(TicketAttachment {
                name: name.chars().take(ATTACH_NAME_MAX).collect(),
                size: size as u64,
            })
        })
        .collect()
}

/// tickets.ts:31 错误码：仅 `^[A-Z_\d]{1,80}$` 原样透出
pub fn envelope_error_code(data: &Value) -> String {
    match data.get("error").and_then(Value::as_str) {
        Some(text) if error_code_re().is_match(text) => text.to_string(),
        _ => "BAD_RESPONSE".to_string(),
    }
}

/// tickets.ts:31 retryAt：只有 RATE_LIMITED 会设
pub fn envelope_retry_at(data: &Value, now: f64) -> Option<f64> {
    if envelope_error_code(data) != "RATE_LIMITED" {
        return None;
    }
    let seconds = js_number(
        data.get("details")
            .and_then(|details| details.get("retryAfter")),
    );
    let span = if seconds > 0.0 && seconds.is_finite() {
        seconds.min(RETRY_AFTER_MAX)
    } else {
        RETRY_AFTER_DEFAULT
    };
    Some(now + span * 1000.0)
}

/// tickets.ts:31-32 信封拆解。`data` 为 null 时原版读 data.success 抛 TypeError，
/// 落到 :33 的 catch 变成兜底文案，而不是 BAD_RESPONSE
pub fn envelope_unwrap(
    data: &Value,
    ok: bool,
    now: f64,
    aborted: bool,
) -> Result<Value, TicketError> {
    // tickets.ts:31 null 走 TypeError 兜底
    if data.is_null() {
        return Err(send_failure(aborted));
    }
    if ok && data.get("success").and_then(Value::as_bool) == Some(true) {
        return Ok(data.get("data").cloned().unwrap_or(Value::Null));
    }
    let code = envelope_error_code(data);
    if code == "RATE_LIMITED" {
        let retry_at = envelope_retry_at(data, now).unwrap_or(now);
        return Err(TicketError::with_retry(
            "RATE_LIMITED",
            "请求过于频繁，请等待倒计时后重试",
            retry_at,
        ));
    }
    Err(TicketError::new(&code, &format!("工单操作失败 ({})", code)))
}

/// tickets.ts:33 外层 catch 的兜底码
pub fn send_failure(aborted: bool) -> TicketError {
    if aborted {
        TicketError::new("CANCELLED", "工单请求已取消")
    } else {
        TicketError::new("NETWORK_ERROR", "工单网络请求失败，请检查网络后重试")
    }
}

/// tickets.ts:57
pub fn validate_description(description: Option<&str>) -> Result<(), TicketError> {
    let Some(text) = description else {
        return Err(invalid_description());
    };
    if js_trim(text).is_empty() || utf16_len(text) > TICKET_DESCRIPTION_MAX {
        return Err(invalid_description());
    }
    Ok(())
}

fn invalid_description() -> TicketError {
    TicketError::new("INVALID_DESCRIPTION", "请填写 1–10000 字的问题描述")
}

/// tickets.ts:67
pub fn validate_message(text: Option<&str>, files_len: usize) -> Result<(), TicketError> {
    let Some(body) = text else {
        return Err(invalid_message());
    };
    if utf16_len(body) > TICKET_MESSAGE_MAX || (js_trim(body).is_empty() && files_len == 0) {
        return Err(invalid_message());
    }
    Ok(())
}

fn invalid_message() -> TicketError {
    TicketError::new("INVALID_MESSAGE", "追问最多 2000 字，文字与附件不能都为空")
}

fn missing_secret() -> TicketError {
    TicketError::new("BAD_RESPONSE", "服务器未返回工单归属凭证")
}

/// tickets.ts:64 提交响应校验，返回 (id, ticketSecret)
pub fn submitted_ticket(data: &Value) -> Result<(String, String), TicketError> {
    if !valid_id(data.get("id")) {
        return Err(missing_secret());
    }
    let id = data["id"].as_str().unwrap_or_default().to_string();
    match data.get("ticketSecret").and_then(Value::as_str) {
        Some(secret) if !secret.is_empty() => Ok((id, secret.to_string())),
        _ => Err(missing_secret()),
    }
}

/// tickets.ts:40-41 详情装配（含完整性校验）
pub fn build_detail(data: &Value, id: &str) -> TicketOutcome<TicketDetail> {
    let same_id = data.get("id").and_then(Value::as_str) == Some(id);
    let description = data.get("description").and_then(Value::as_str);
    let messages = data.get("messages").and_then(Value::as_array);
    let too_many = messages
        .as_ref()
        .map(|list| list.len() > TICKET_MESSAGE_COUNT_MAX)
        .unwrap_or(true);
    if !same_id || description.is_none() || too_many {
        return Err(TicketFailure::ticket("BAD_RESPONSE", "工单详情数据不完整"));
    }
    let mut built = Vec::new();
    for item in messages.unwrap() {
        if item.is_null() {
            return Err(TicketFailure::Other(
                "TypeError: Cannot read properties of null (reading 'id')".to_string(),
            ));
        }
        built.push(TicketMessage {
            id: valid_id(item.get("id"))
                .then(|| item["id"].as_str().unwrap_or_default().to_string()),
            from: item
                .get("from")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string(),
            time_ms: js_num(safe_count(item.get("time")) * 1000.0),
            text: item
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            attachments: normalize_attachments(item.get("attachments")),
        });
    }
    Ok(TicketDetail {
        id: id.to_string(),
        time_ms: js_num(safe_count(data.get("time")) * 1000.0),
        deleted: data.get("deleted").and_then(Value::as_bool) == Some(true),
        description: description.unwrap_or_default().to_string(),
        attachments: normalize_attachments(data.get("attachments")),
        messages: built,
    })
}

/// tickets.ts:43 限频判定（history 需已按十分钟窗口过滤）
pub fn rate_failure(cooldown: f64, history: &[f64], now: f64) -> Option<TicketError> {
    if now < cooldown {
        return Some(TicketError::with_retry(
            "RATE_LIMITED",
            "请求过于频繁，请稍后重试",
            cooldown,
        ));
    }
    if history.len() >= RATE_MAX {
        return Some(TicketError::with_retry(
            "RATE_LIMITED",
            "每 10 分钟最多提交 3 次，请稍后重试",
            history[0] + RATE_WINDOW_MS,
        ));
    }
    None
}

/// tickets.ts:67 重发抑制
pub fn duplicate_failure(last: Option<(&str, f64)>, text: &str, now: f64) -> Option<TicketError> {
    match last {
        Some((previous, at)) if previous == text && now - at < DUPLICATE_WINDOW_MS => Some(
            TicketError::new("DUPLICATE", "同一段追问请间隔 15 秒再发送"),
        ),
        _ => None,
    }
}

/// tickets.ts:45
pub fn attachment_guard(files: &[TicketFile]) -> Result<(), TicketError> {
    if files.len() > TICKET_FILES_MAX
        || files.iter().map(|f| f.size).sum::<f64>() > TICKET_BYTES_MAX
    {
        return Err(TicketError::new(
            "TICKET_TOO_LARGE",
            "每条消息最多 10 个附件，附件总量不得超过 500 MB",
        ));
    }
    Ok(())
}

/// tickets.ts:29 响应体上限
pub fn response_size_error(total: usize) -> Option<TicketError> {
    if total > RESPONSE_LIMIT {
        return Some(TicketError::new("BAD_RESPONSE", "工单响应过大"));
    }
    None
}

/// tickets.ts:30 响应体解析
pub fn parse_ticket_response(bytes: &[u8]) -> Result<Value, TicketError> {
    serde_json::from_slice(bytes).map_err(|_| TicketError::new("BAD_RESPONSE", "工单服务器返回无效数据"))
}

/// tickets.ts:49 打开后必须与授权时同尺寸同修改时间
pub fn attachment_mismatch(metadata: &std::fs::Metadata, file: &TicketFile) -> Option<TicketError> {
    if !metadata.is_file()
        || metadata.len() as f64 != file.size
        || system_mtime_ms(metadata) != file.mtime
    {
        return Some(TicketError::new(
            "ATTACHMENT_CHANGED",
            &format!("{} 已变化，请重新选择", file.name),
        ));
    }
    None
}

/// tickets.ts:67 消息数与附件总量两道闸门
pub fn reply_capacity_guard(detail: &TicketDetail, files: &[TicketFile]) -> Result<(), TicketError> {
    if detail.messages.len() >= TICKET_MESSAGE_COUNT_MAX {
        return Err(TicketError::new("MESSAGE_LIMIT", "工单已达到 200 条消息上限"));
    }
    if detail.used_bytes() + files.iter().map(|f| f.size).sum::<f64>() > TICKET_BYTES_MAX {
        return Err(TicketError::new("TICKET_TOO_LARGE", "该工单附件总量不得超过 500 MB"));
    }
    Ok(())
}

/// tickets.ts:68 只允许撤回自己最后一条追问
pub fn retract_guard(detail: &TicketDetail, msg: &str) -> Result<(), TicketError> {
    if detail.own_last_message().and_then(|m| m.id.as_deref()) != Some(msg) {
        return Err(TicketError::new("TICKET_FORBIDDEN", "只能撤回自己的上一条追问"));
    }
    Ok(())
}

/// tickets.ts:63 clientInfo；键序即 TS 字面量序，client 固定写 KAMUCL
pub fn client_info(version: &str) -> String {
    map_of([
        ("client", Value::String("KAMUCL".to_string())),
        ("version", Value::String(version.to_string())),
        ("os", Value::String(node_platform().to_string())),
        ("arch", Value::String(node_arch().to_string())),
    ])
    .to_string()
}

/// JS process.platform / process.arch 的取值写法
pub fn node_platform() -> &'static str {
    match std::env::consts::OS {
        "windows" => "win32",
        "macos" => "darwin",
        other => other,
    }
}

pub fn node_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => other,
    }
}

/// tickets.ts:46 分节符
pub fn multipart_boundary(uuid: &str) -> String {
    format!("----VoxLinkTK{}", uuid.replace('-', ""))
}

/// tickets.ts:46 字段段
pub fn field_part(boundary: &str, key: &str, value: &str) -> Vec<u8> {
    format!(
        "--{}\r\nContent-Disposition: form-data; name=\"{}\"\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{}\r\n",
        boundary, key, value
    )
    .into_bytes()
}

/// tickets.ts:47 文件名清洗
pub fn sanitize_filename(name: &str) -> String {
    filename_strip_re().replace_all(name, "_").into_owned()
}

/// tickets.ts:47 附件段头
pub fn file_head(boundary: &str, name: &str) -> Vec<u8> {
    format!(
        "--{}\r\nContent-Disposition: form-data; name=\"attachments\"; filename=\"{}\"\r\nContent-Type: application/octet-stream\r\n\r\n",
        boundary,
        sanitize_filename(name)
    )
    .into_bytes()
}

/// tickets.ts:47 结束分节符
pub fn multipart_end(boundary: &str) -> Vec<u8> {
    format!("--{}--\r\n", boundary).into_bytes()
}

/// tickets.ts:50 申报总字节数（每个附件另加收尾 CRLF 的 2 字节）
pub fn multipart_total(
    parts: &[Vec<u8>],
    heads: &[Vec<u8>],
    files: &[TicketFile],
    end: &[u8],
) -> f64 {
    parts.iter().map(|b| b.len() as f64).sum::<f64>()
        + heads.iter().map(|b| b.len() as f64).sum::<f64>()
        + files.iter().map(|f| f.size + 2.0).sum::<f64>()
        + end.len() as f64
}

/// tickets.ts:36-37 轮询请求体
pub fn poll_body(ids: &[String]) -> Value {
    map_of([(
        "ids",
        Value::Array(ids.iter().map(|id| Value::String(id.clone())).collect()),
    )])
}

#[derive(Default)]
struct TicketState {
    loaded: bool,
    rows: Vec<StoredTicket>,
    polled: bool,
    cooldown: HashMap<String, f64>,
    history: HashMap<String, Vec<f64>>,
    last_reply: HashMap<String, (String, f64)>,
}

#[derive(Clone)]
pub struct TicketService {
    file: PathBuf,
    base: Arc<dyn Fn() -> String + Send + Sync>,
    codec: Arc<dyn SecretCodec>,
    version: String,
    short: reqwest::blocking::Client,
    upload: reqwest::blocking::Client,
    state: Arc<Mutex<TicketState>>,
}

fn ticket_client(timeout_ms: u64) -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_millis(timeout_ms))
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()
        .expect("工单 HTTP 客户端初始化失败")
}

/// searchParams.set：已有键原位改值并丢掉同名重复项
fn set_query_pair(pairs: &mut Vec<(String, String)>, key: &str, value: &str) {
    let Some(position) = pairs.iter().position(|(name, _)| name.as_str() == key) else {
        pairs.push((key.to_string(), value.to_string()));
        return;
    };
    pairs[position].1 = value.to_string();
    let mut seen = false;
    pairs.retain(|(name, _)| name.as_str() != key || !std::mem::replace(&mut seen, true));
}

impl TicketService {
    /// tickets.ts:19 构造；第四参（注入 fetch）由 reqwest 承担，不移植
    pub fn new(
        file: PathBuf,
        base: Arc<dyn Fn() -> String + Send + Sync>,
        codec: Arc<dyn SecretCodec>,
        version: impl Into<String>,
    ) -> Self {
        TicketService {
            file,
            base,
            codec,
            version: version.into(),
            short: ticket_client(REQUEST_TIMEOUT_MS),
            upload: ticket_client(UPLOAD_TIMEOUT_MS),
            state: Arc::new(Mutex::new(TicketState::default())),
        }
    }

    /// tickets.ts:20 首次读盘：ENOENT 静默，其余归 STORE_INVALID
    fn load(&self) -> Result<(), TicketFailure> {
        let mut state = self.state.lock().unwrap();
        if state.loaded {
            return Ok(());
        }
        state.loaded = true;
        let text = match std::fs::read_to_string(&self.file) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(_) => {
                state.loaded = false;
                return Err(store_invalid());
            }
        };
        let parsed: Value = match serde_json::from_str(&text) {
            Ok(value) => value,
            Err(_) => {
                state.loaded = false;
                return Err(store_invalid());
            }
        };
        let Some(list) = parsed.as_array() else {
            state.loaded = false;
            return Err(store_invalid());
        };
        state.rows = list
            .iter()
            .filter(|row| {
                valid_id(row.get("id"))
                    && row.get("secret").and_then(Value::as_str).is_some()
                    && row
                        .get("server")
                        .and_then(Value::as_str)
                        .map(validate_server_url)
                        .unwrap_or(false)
            })
            .map(|row| StoredTicket {
                id: row["id"].as_str().unwrap_or_default().to_string(),
                time_ms: js_num(safe_count(row.get("timeMs"))),
                deleted: row.get("deleted").and_then(Value::as_bool) == Some(true),
                has_unread: row.get("hasUnread").and_then(Value::as_bool) == Some(true),
                reply_count: js_num(safe_count(row.get("replyCount"))),
                last_time_ms: js_num(safe_count(row.get("lastTimeMs"))),
                secret: row["secret"].as_str().unwrap_or_default().to_string(),
                server: row["server"].as_str().unwrap_or_default().to_string(),
            })
            .collect();
        Ok(())
    }

    /// tickets.ts:21 临时文件 + 改名落盘
    fn save(&self) -> Result<(), TicketFailure> {
        let dir = self
            .file
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        if std::fs::create_dir_all(dir).is_err() {
            return Err(TicketFailure::Other("工单索引目录不可用".to_string()));
        }
        let json = serde_json::to_string(&self.state.lock().unwrap().rows).unwrap_or_default();
        let temp = format!(
            "{}.{}.tmp",
            self.file.to_string_lossy(),
            crate::accounts::uuid_v4()
        );
        let mut result = Ok(());
        if std::fs::write(&temp, json).is_err() {
            result = Err(TicketFailure::Other("工单索引写入失败".to_string()));
        } else if std::fs::rename(&temp, &self.file).is_err() {
            result = Err(TicketFailure::Other("工单索引写入失败".to_string()));
        }
        let _ = std::fs::remove_file(&temp);
        result
    }

    /// tickets.ts:22
    pub fn list(&self) -> TicketOutcome<Vec<Value>> {
        self.load()?;
        let server = (self.base)();
        let mut rows: Vec<StoredTicket> = {
            let state = self.state.lock().unwrap();
            state
                .rows
                .iter()
                .filter(|row| row.server == server && !row.deleted)
                .cloned()
                .collect()
        };
        rows.sort_by(|a, b| {
            b.sort_key()
                .partial_cmp(&a.sort_key())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(rows.iter().map(StoredTicket::visible).collect())
    }

    /// tickets.ts:23
    fn owned(&self, id: &str) -> Result<usize, TicketFailure> {
        self.load()?;
        let server = (self.base)();
        let state = self.state.lock().unwrap();
        state
            .rows
            .iter()
            .position(|row| row.id == id && row.server == server && !row.deleted)
            .ok_or_else(|| TicketFailure::ticket("UNKNOWN_TICKET", "本机没有该工单的归属凭证"))
    }

    /// tickets.ts:24
    fn secret(&self, secret: &str) -> Result<String, TicketError> {
        self.codec.open(secret).map_err(|_| {
            TicketError::new(
                "SECRET_UNAVAILABLE",
                "工单归属凭证无法解密，请使用最初提交工单的系统账号",
            )
        })
    }

    fn secret_of(&self, id: &str) -> Result<String, TicketFailure> {
        let index = self.owned(id)?;
        let state = self.state.lock().unwrap();
        Ok(self.secret(&state.rows[index].secret)?)
    }

    fn row_secret(&self, index: usize) -> Result<String, TicketFailure> {
        let secret = self
            .state
            .lock()
            .unwrap()
            .rows
            .get(index)
            .map(|row| row.secret.clone());
        let Some(secret) = secret else {
            return Err(TicketFailure::ticket(
                "UNKNOWN_TICKET",
                "本机没有该工单的归属凭证",
            ));
        };
        Ok(self.secret(&secret)?)
    }

    fn update_row<F: FnOnce(&mut StoredTicket)>(&self, index: usize, id: &str, f: F) {
        let mut state = self.state.lock().unwrap();
        if let Some(row) = state.rows.get_mut(index).filter(|row| row.id == id) {
            f(row);
        }
    }

    /// tickets.ts:25
    pub fn ticket_url(&self, route: &str, query: &[(&str, &str)]) -> Result<String, TicketError> {
        let base = (self.base)();
        if !validate_server_url(&base) {
            return Err(TicketError::new("BAD_SERVER_URL", "工单服务器地址无效"));
        }
        let mut url = match url::Url::parse(&base) {
            Ok(value) => value,
            Err(_) => return Err(TicketError::new("BAD_SERVER_URL", "工单服务器地址无效")),
        };
        let mut pairs: Vec<(String, String)> = url
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        set_query_pair(&mut pairs, "route", route);
        for (key, value) in query {
            set_query_pair(&mut pairs, key, value);
        }
        {
            let mut writer = url.query_pairs_mut();
            writer.clear();
            for (key, value) in &pairs {
                writer.append_pair(key.as_str(), value.as_str());
            }
        }
        Ok(url.as_str().to_string())
    }

    /// tickets.ts:26-34
    fn send(
        &self,
        route: &str,
        method: &str,
        headers: Vec<(String, String)>,
        body: TicketBody,
        cancelled: &Arc<AtomicBool>,
        query: &[(&str, &str)],
    ) -> Result<Value, TicketError> {
        let url = self.ticket_url(route, query)?;
        let body_error = match &body {
            TicketBody::Multipart { body, .. } => body.error.clone(),
            _ => Arc::new(Mutex::new(None)),
        };
        let uploading = matches!(body, TicketBody::Multipart { .. });
        let client = self.client_for(uploading);
        let mut request = if method == "GET" {
            client.get(url.as_str())
        } else {
            client.post(url.as_str())
        };
        for (name, value) in headers {
            request = request.header(name.as_str(), value.as_str());
        }
        request = match body {
            TicketBody::Empty => request,
            TicketBody::Json(text) => request.body(text),
            TicketBody::Multipart {
                content_type,
                length,
                body,
            } => request
                .header("Content-Type", content_type)
                .body(reqwest::blocking::Body::sized(body, length)),
        };
        let reply = match request.send() {
            Ok(value) => value,
            Err(_) => return Err(self.local_failure(cancelled, &body_error)),
        };
        let ok = reply.status().is_success();
        // tickets.ts:27 3xx 视作网络失败
        if reply.status().is_redirection() {
            return Err(self.local_failure(cancelled, &body_error));
        }
        let mut chunks: Vec<u8> = Vec::new();
        let mut buffer = vec![0u8; UPLOAD_CHUNK];
        let mut total = 0usize;
        let mut reader = reply;
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(size) => {
                    total += size;
                    if let Some(error) = response_size_error(total) {
                        return Err(error);
                    }
                    chunks.extend_from_slice(&buffer[..size]);
                }
                Err(_) => return Err(self.local_failure(cancelled, &body_error)),
            }
        }
        let data: Value = match parse_ticket_response(&chunks) {
            Ok(value) => value,
            Err(error) => return Err(error),
        };
        let outcome = envelope_unwrap(&data, ok, now_ms(), cancelled.load(Ordering::SeqCst));
        if let Err(error) = &outcome {
            if error.code == "RATE_LIMITED" {
                if let Some(retry_at) = error.retry_at {
                    self.state
                        .lock()
                        .unwrap()
                        .cooldown
                        .insert(route.to_string(), retry_at);
                }
            }
        }
        outcome
    }

    fn client_for(&self, uploading: bool) -> &reqwest::blocking::Client {
        if uploading {
            &self.upload
        } else {
            &self.short
        }
    }

    /// tickets.ts:33 兜底：先取上传期记录的校验错误，再看取消位
    fn local_failure(
        &self,
        cancelled: &Arc<AtomicBool>,
        body_error: &Mutex<Option<TicketError>>,
    ) -> TicketError {
        if let Some(error) = body_error.lock().unwrap().take() {
            return error;
        }
        send_failure(cancelled.load(Ordering::SeqCst))
    }

    /// tickets.ts:35
    fn json(&self, route: &str, body: &Value, cancelled: &Arc<AtomicBool>) -> TicketOutcome<Value> {
        self.send(
            route,
            "POST",
            vec![("Content-Type".to_string(), "application/json".to_string())],
            TicketBody::Json(body.to_string()),
            cancelled,
            &[],
        )
        .map_err(TicketFailure::Ticket)
    }

    /// tickets.ts:36-39 只跑一次，后续直接返回本地表
    pub fn poll_once(&self, cancelled: &Arc<AtomicBool>) -> TicketOutcome<Vec<Value>> {
        let polled = self.state.lock().unwrap().polled;
        if polled {
            return self.list();
        }
        self.state.lock().unwrap().polled = true;
        self.load()?;
        let server = (self.base)();
        let rows: Vec<StoredTicket> = {
            let state = self.state.lock().unwrap();
            state
                .rows
                .iter()
                .filter(|row| row.server == server && !row.deleted)
                .cloned()
                .collect()
        };
        let mut offset = 0usize;
        while offset < rows.len() {
            if cancelled.load(Ordering::SeqCst) {
                return Err(TicketFailure::aborted());
            }
            let end = (offset + POLL_PAGE_SIZE).min(rows.len());
            let page: Vec<String> = rows[offset..end].iter().map(|row| row.id.clone()).collect();
            let data = self.json("/ticket/poll", &poll_body(&page), cancelled)?;
            if let Some(list) = data.get("tickets").and_then(Value::as_array) {
                for item in list {
                    let Some(id) = item.get("id").and_then(Value::as_str) else {
                        continue;
                    };
                    if !page.iter().any(|known| known.as_str() == id) {
                        continue;
                    }
                    let index = {
                        let state = self.state.lock().unwrap();
                        state
                            .rows
                            .iter()
                            .position(|row| row.id == id && row.server == server)
                    };
                    let Some(index) = index else {
                        return Err(TicketFailure::Other(
                            "TypeError: Cannot set properties of undefined (reading 'hasUnread')"
                                .to_string(),
                        ));
                    };
                    self.update_row(index, id, |row| {
                        row.has_unread =
                            item.get("hasUnread").and_then(Value::as_bool) == Some(true);
                        row.deleted = item.get("deleted").and_then(Value::as_bool) == Some(true);
                        row.reply_count = js_num(safe_count(item.get("replyCount")));
                        row.last_time_ms = js_num(safe_count(item.get("lastTime")) * 1000.0);
                    });
                }
            }
            let removed: Vec<String> = data
                .get("removed")
                .and_then(Value::as_array)
                .map(|list| {
                    list.iter()
                        .filter_map(|id| id.as_str())
                        .filter(|id| page.iter().any(|known| known.as_str() == *id))
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            {
                let mut state = self.state.lock().unwrap();
                state
                    .rows
                    .retain(|row| row.server != server || !removed.iter().any(|id| id == &row.id));
            }
            self.save()?;
            offset = end;
        }
        self.list()
    }

    /// tickets.ts:40-42
    pub fn detail(&self, id: &str, cancelled: &Arc<AtomicBool>) -> TicketOutcome<TicketDetail> {
        let index = self.owned(id)?;
        let secret = self.row_secret(index)?;
        let data = self.send(
            "/ticket/detail",
            "GET",
            Vec::new(),
            TicketBody::Empty,
            cancelled,
            &[("id", id), ("secret", secret.as_str())],
        )?;
        let detail = build_detail(&data, id)?;
        let admin_count = detail
            .messages
            .iter()
            .filter(|message| message.from == "admin")
            .count() as f64;
        self.update_row(index, id, |row| {
            let mut last = row.time_ms.as_f64().unwrap_or_default();
            for message in &detail.messages {
                last = last.max(message.time_ms.as_f64().unwrap_or_default());
            }
            row.has_unread = false;
            row.reply_count = js_num(admin_count);
            row.last_time_ms = js_num(last);
        });
        self.save()?;
        // tickets.ts:41 参数在调用点求值
        let viewed = map_of([
            ("id", Value::String(id.to_string())),
            ("secret", Value::String(self.secret_of(id)?)),
        ]);
        let service = self.clone();
        thread::spawn(move || {
            let _ = service.json("/ticket/viewed", &viewed, &Arc::new(AtomicBool::new(false)));
        });
        Ok(detail)
    }

    /// tickets.ts:43
    fn rate(&self, route: &str) -> Result<(), TicketError> {
        let now = now_ms();
        let mut state = self.state.lock().unwrap();
        let cooldown = state.cooldown.get(route).copied().unwrap_or_default();
        let history: Vec<f64> = state
            .history
            .get(route)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|at| now - at < RATE_WINDOW_MS)
            .collect();
        state.history.insert(route.to_string(), history.clone());
        rate_failure(cooldown, &history, now).map_or(Ok(()), Err)
    }

    /// tickets.ts:44-55
    fn upload(
        &self,
        route: &str,
        fields: Vec<(String, String)>,
        files: &[TicketFile],
        cancelled: &Arc<AtomicBool>,
        progress: Option<TicketProgress>,
    ) -> TicketOutcome<Value> {
        attachment_guard(files)?;
        self.rate(route)?;
        let boundary = multipart_boundary(&crate::accounts::uuid_v4());
        let parts: Vec<Vec<u8>> = fields
            .iter()
            .map(|(key, value)| field_part(&boundary, key, value))
            .collect();
        let heads: Vec<Vec<u8>> = files
            .iter()
            .map(|file| file_head(&boundary, &file.name))
            .collect();
        let end = multipart_end(&boundary);
        let mut handles = Vec::with_capacity(files.len());
        for file in files {
            let handle =
                File::open(&file.path).map_err(|error| TicketFailure::Other(error.to_string()))?;
            let metadata = handle
                .metadata()
                .map_err(|error| TicketFailure::Other(error.to_string()))?;
            if let Some(error) = attachment_mismatch(&metadata, file) {
                return Err(TicketFailure::Ticket(error));
            }
            handles.push(handle);
        }
        let total = multipart_total(&parts, &heads, files, &end);
        let length = total.max(0.0) as u64;
        let mut plan: VecDeque<ChunkSpec> = VecDeque::new();
        for part in parts {
            plan.push_back(ChunkSpec::Bytes(part));
        }
        for (index, head) in heads.into_iter().enumerate() {
            plan.push_back(ChunkSpec::Bytes(head));
            plan.push_back(ChunkSpec::FileRun(index));
            plan.push_back(ChunkSpec::Bytes(b"\r\n".to_vec()));
        }
        plan.push_back(ChunkSpec::Bytes(end));
        let body = MultipartBody {
            plan,
            files: handles,
            read_bytes: vec![0u64; files.len()],
            sizes: files.iter().map(|file| file.size.max(0.0) as u64).collect(),
            current: Vec::new(),
            offset: 0,
            finished: false,
            sent: 0.0,
            total,
            cancelled: cancelled.clone(),
            progress,
            error: Arc::new(Mutex::new(None)),
        };
        self.send(
            route,
            "POST",
            Vec::new(),
            TicketBody::Multipart {
                content_type: format!("multipart/form-data; boundary={}", boundary),
                length,
                body,
            },
            cancelled,
            &[],
        )
        .map(|value| {
            self.state
                .lock()
                .unwrap()
                .history
                .entry(route.to_string())
                .or_default()
                .push(now_ms());
            value
        })
        .map_err(TicketFailure::Ticket)
    }

    /// tickets.ts:56-66
    pub fn submit(
        &self,
        description: Option<&str>,
        files: &[TicketFile],
        cancelled: &Arc<AtomicBool>,
        progress: Option<TicketProgress>,
    ) -> TicketOutcome<Value> {
        validate_description(description)?;
        if cancelled.load(Ordering::SeqCst) {
            return Err(TicketFailure::aborted());
        }
        self.load()?;
        let probe = format!("ticket-storage-{}", crate::accounts::uuid_v4());
        let sealed = self.codec.seal(&probe)?;
        if self.codec.open(&sealed)? != probe {
            return Err(TicketFailure::ticket(
                "SECRET_UNAVAILABLE",
                "系统凭证保护无法验证，请稍后再提交",
            ));
        }
        self.save()?;
        if cancelled.load(Ordering::SeqCst) {
            return Err(TicketFailure::aborted());
        }
        let fields = vec![
            (
                "description".to_string(),
                js_trim(description.unwrap_or_default()).to_string(),
            ),
            ("client".to_string(), "app".to_string()),
            ("clientInfo".to_string(), client_info(&self.version)),
        ];
        let data = self.upload("/ticket/submit", fields, files, cancelled, progress)?;
        let (id, secret) = submitted_ticket(&data)?;
        let sealed = self.codec.seal(&secret)?;
        let server = (self.base)();
        let now = js_num(now_ms());
        self.state.lock().unwrap().rows.push(StoredTicket {
            id: id.clone(),
            time_ms: now.clone(),
            deleted: false,
            has_unread: false,
            reply_count: js_num(0.0),
            last_time_ms: now,
            secret: sealed,
            server,
        });
        self.save()?;
        Ok(map_of([("id", Value::String(id))]))
    }

    /// tickets.ts:67
    pub fn reply(
        &self,
        id: &str,
        text: Option<&str>,
        files: &[TicketFile],
        cancelled: &Arc<AtomicBool>,
        progress: Option<TicketProgress>,
    ) -> TicketOutcome<Value> {
        let index = self.owned(id)?;
        validate_message(text, files.len())?;
        let trimmed = js_trim(text.unwrap_or_default()).to_string();
        if files.is_empty() {
            let blocked = {
                let state = self.state.lock().unwrap();
                duplicate_failure(
                    state
                        .last_reply
                        .get(id)
                        .map(|(previous, at)| (previous.as_str(), *at)),
                    &trimmed,
                    now_ms(),
                )
            };
            if let Some(error) = blocked {
                return Err(TicketFailure::Ticket(error));
            }
        }
        let detail = self.detail(id, cancelled)?;
        if detail.messages.len() >= TICKET_MESSAGE_COUNT_MAX {
            return Err(TicketFailure::ticket(
                "MESSAGE_LIMIT",
                "工单已达到 200 条消息上限",
            ));
        }
        if detail.used_bytes() + files.iter().map(|file| file.size).sum::<f64>() > TICKET_BYTES_MAX
        {
            return Err(TicketFailure::ticket(
                "TICKET_TOO_LARGE",
                "该工单附件总量不得超过 500 MB",
            ));
        }
        let secret = self.row_secret(index)?;
        let fields = vec![
            ("id".to_string(), id.to_string()),
            ("secret".to_string(), secret),
            ("text".to_string(), trimmed.clone()),
        ];
        self.upload("/ticket/reply", fields, files, cancelled, progress)?;
        let now = now_ms();
        {
            let mut state = self.state.lock().unwrap();
            state.last_reply.insert(id.to_string(), (trimmed, now));
        }
        self.update_row(index, id, |row| row.last_time_ms = js_num(now));
        self.save()?;
        Ok(map_of([("id", Value::String(id.to_string()))]))
    }

    /// tickets.ts:68
    pub fn retract(
        &self,
        id: &str,
        msg: &str,
        cancelled: &Arc<AtomicBool>,
    ) -> TicketOutcome<Value> {
        let index = self.owned(id)?;
        let detail = self.detail(id, cancelled)?;
        if detail
            .own_last_message()
            .and_then(|message| message.id.as_deref())
            != Some(msg)
        {
            return Err(TicketFailure::ticket(
                "TICKET_FORBIDDEN",
                "只能撤回自己的上一条追问",
            ));
        }
        let secret = self.row_secret(index)?;
        let body = map_of([
            ("id", Value::String(id.to_string())),
            ("msg", Value::String(msg.to_string())),
            ("secret", Value::String(secret)),
        ]);
        self.json("/ticket/retract", &body, cancelled)?;
        Ok(map_of([("id", Value::String(id.to_string()))]))
    }

    /// tickets.ts:69
    pub fn remove(&self, id: &str, cancelled: &Arc<AtomicBool>) -> TicketOutcome<Value> {
        let index = self.owned(id)?;
        let secret = self.row_secret(index)?;
        let body = map_of([
            ("id", Value::String(id.to_string())),
            ("secret", Value::String(secret)),
        ]);
        self.json("/ticket/delete", &body, cancelled)?;
        self.update_row(index, id, |row| {
            row.deleted = true;
            row.has_unread = false;
        });
        self.save()?;
        Ok(map_of([("id", Value::String(id.to_string()))]))
    }
}

fn store_invalid() -> TicketFailure {
    TicketFailure::ticket("STORE_INVALID", "本地工单索引无法读取，已保留原文件")
}

enum TicketBody {
    Empty,
    Json(String),
    Multipart {
        content_type: String,
        length: u64,
        body: MultipartBody,
    },
}

enum ChunkSpec {
    Bytes(Vec<u8>),
    FileRun(usize),
}

/// tickets.ts:51 的分块体生成器：段边界即进度上报点
struct MultipartBody {
    plan: VecDeque<ChunkSpec>,
    files: Vec<File>,
    read_bytes: Vec<u64>,
    sizes: Vec<u64>,
    current: Vec<u8>,
    offset: usize,
    finished: bool,
    sent: f64,
    total: f64,
    cancelled: Arc<AtomicBool>,
    progress: Option<TicketProgress>,
    error: Arc<Mutex<Option<TicketError>>>,
}

impl MultipartBody {
    fn fail(&mut self, error: TicketError) -> std::io::Error {
        *self.error.lock().unwrap() = Some(error);
        std::io::Error::other("工单附件校验失败")
    }

    fn pull(&mut self) -> Option<Result<Vec<u8>, std::io::Error>> {
        loop {
            let running = match self.plan.front() {
                Some(ChunkSpec::FileRun(index)) => Some(*index),
                _ => None,
            };
            if let Some(index) = running {
                let mut buffer = vec![0u8; UPLOAD_CHUNK];
                match self.files[index].read(&mut buffer) {
                    Ok(0) => {
                        if self.read_bytes[index] != self.sizes[index] {
                            return Some(Err(self.fail(TicketError::new(
                                "ATTACHMENT_CHANGED",
                                "上传期间附件已变化",
                            ))));
                        }
                        self.plan.pop_front();
                        continue;
                    }
                    Ok(size) => {
                        self.read_bytes[index] += size as u64;
                        buffer.truncate(size);
                        return Some(Ok(buffer));
                    }
                    Err(error) => {
                        return Some(Err(std::io::Error::new(error.kind(), "工单附件读取失败")))
                    }
                }
            }
            match self.plan.pop_front() {
                None => return None,
                Some(ChunkSpec::Bytes(bytes)) => {
                    if !bytes.is_empty() {
                        return Some(Ok(bytes));
                    }
                }
                Some(ChunkSpec::FileRun(_)) => continue,
            }
        }
    }
}

impl Read for MultipartBody {
    fn read(&mut self, into: &mut [u8]) -> std::io::Result<usize> {
        if self.finished || into.is_empty() {
            return Ok(0);
        }
        loop {
            if self.offset >= self.current.len() {
                match self.pull() {
                    None => {
                        self.finished = true;
                        return Ok(0);
                    }
                    Some(Err(error)) => return Err(error),
                    Some(Ok(chunk)) => {
                        if self.cancelled.load(Ordering::SeqCst) {
                            return Err(std::io::Error::other("工单请求已取消"));
                        }
                        self.sent += chunk.len() as f64;
                        // tickets.ts:50 超申报长度按网络失败
                        if self.sent > self.total {
                            return Err(std::io::Error::other("工单附件超出申报长度"));
                        }
                        if let Some(progress) = &self.progress {
                            progress(self.sent, self.total);
                        }
                        self.current = chunk;
                        self.offset = 0;
                    }
                }
            }
            let size = into.len().min(self.current.len() - self.offset);
            if size > 0 {
                into[..size].copy_from_slice(&self.current[self.offset..self.offset + size]);
                self.offset += size;
                return Ok(size);
            }
        }
    }
}

/// 环回侧一个数据报的处置（turnTcp.ts:29-31）
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShimVerdict {
    /// 非本机环回口 / 空包 / 超 2048：静默丢弃
    Ignore,
    /// 待写字节超过 1 MiB：整条链路关闭
    Close,
    Frame(Vec<u8>),
}

/// u16BE 长度前缀组帧（turnTcp.ts:31）
pub fn tcp_frame(packet: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(TURN_TCP_FRAME_PREFIX + packet.len());
    frame.extend_from_slice(&(packet.len() as u16).to_be_bytes());
    frame.extend_from_slice(packet);
    frame
}

/// 入口一个数据报的处置（turnTcp.ts:29-31）
pub fn shim_verdict(packet: &[u8], from_client_port: bool, writable_length: usize) -> ShimVerdict {
    if !from_client_port || packet.is_empty() || packet.len() > TURN_TCP_MAX_PACKET {
        return ShimVerdict::Ignore;
    }
    if writable_length > TURN_TCP_BACKPRESSURE_BYTES {
        return ShimVerdict::Close;
    }
    ShimVerdict::Frame(tcp_frame(packet))
}

/// TCP 侧解帧（turnTcp.ts:34-42）；Ok(vec![]) 表示还欠半个帧，Err 表示该关链路
pub fn decode_turn_tcp(pending: &mut Vec<u8>) -> Result<Vec<Vec<u8>>, ()> {
    let mut frames = Vec::new();
    while pending.len() >= TURN_TCP_FRAME_PREFIX {
        let size = u16::from_be_bytes([pending[0], pending[1]]) as usize;
        if size == 0 || size > TURN_TCP_MAX_PACKET {
            return Err(());
        }
        if pending.len() < size + TURN_TCP_FRAME_PREFIX {
            break;
        }
        frames.push(pending[TURN_TCP_FRAME_PREFIX..size + TURN_TCP_FRAME_PREFIX].to_vec());
        pending.drain(..size + TURN_TCP_FRAME_PREFIX);
    }
    Ok(frames)
}

struct TurnTcpInner {
    closed: AtomicBool,
    tx: Mutex<Option<Sender<Vec<u8>>>>,
    shim: Arc<UdpSocket>,
    peer: Mutex<TcpStream>,
}

impl TurnTcpInner {
    /// turnTcp.ts:9-12 幂等关闭；socket 的 error/close 都汇到这里
    fn close(&self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        let _ = self.tx.lock().unwrap().take();
        let _ = self.peer.lock().unwrap().shutdown(std::net::Shutdown::Both);
    }
}

/// turnTcp.ts:44 的返回：交给 RUDP 的 socket、环回目标与关闭器
pub struct TurnTcpChannel {
    pub socket: Arc<UdpSocket>,
    pub target: RudpTarget,
    inner: Arc<TurnTcpInner>,
}

impl TurnTcpChannel {
    pub fn close(&self) {
        self.inner.close();
    }

    pub fn is_closed(&self) -> bool {
        self.inner.closed.load(Ordering::SeqCst)
    }
}

impl Drop for TurnTcpChannel {
    fn drop(&mut self) {
        self.close();
    }
}

fn tcp_connect(host: &str, port: u16, cancelled: &AtomicBool) -> Result<TcpStream, String> {
    let host = host.to_string();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let outcome = (host.as_str(), port)
            .to_socket_addrs()
            .ok()
            .and_then(|mut addrs| addrs.next())
            .and_then(|addr| {
                TcpStream::connect_timeout(
                    &addr,
                    Duration::from_millis(TURN_TCP_CONNECT_TIMEOUT_MS),
                )
                .ok()
            });
        let _ = tx.send(outcome.ok_or_else(|| TURN_TCP_TIMEOUT_CLOSED.to_string()));
    });
    let deadline = Instant::now() + Duration::from_millis(TURN_TCP_CONNECT_TIMEOUT_MS);
    loop {
        // turnTcp.ts:19 abort 先到报已关闭
        if cancelled.load(Ordering::SeqCst) {
            return Err(TURN_TCP_TIMEOUT_CLOSED.to_string());
        }
        let now = Instant::now();
        if now >= deadline {
            return Err(TURN_TCP_TIMEOUT_CONNECT.to_string());
        }
        match rx.recv_timeout((deadline - now).min(Duration::from_millis(50))) {
            Ok(outcome) => return outcome,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => return Err(TURN_TCP_TIMEOUT_CLOSED.to_string()),
        }
    }
}

/// turnTcp.ts:5-46 TURN-over-TCP 隧道；失败原因只用原版三句文案
pub fn open_turn_tcp(
    host: &str,
    port: u16,
    cancelled: &Arc<AtomicBool>,
) -> Result<TurnTcpChannel, String> {
    if cancelled.load(Ordering::SeqCst) {
        return Err(TURN_TCP_TIMEOUT_CANCELLED.to_string());
    }
    let stream = tcp_connect(host, port, cancelled)?;
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
    let client = Arc::new(
        UdpSocket::bind((TURN_TCP_SHIM_ADDRESS, 0))
            .map_err(|_| TURN_TCP_TIMEOUT_CANCELLED.to_string())?,
    );
    let shim = Arc::new(
        UdpSocket::bind((TURN_TCP_SHIM_ADDRESS, 0))
            .map_err(|_| TURN_TCP_TIMEOUT_CANCELLED.to_string())?,
    );
    let _ = shim.set_read_timeout(Some(Duration::from_millis(200)));
    if cancelled.load(Ordering::SeqCst) {
        return Err(TURN_TCP_TIMEOUT_CANCELLED.to_string());
    }
    let client_port = client
        .local_addr()
        .map_err(|_| TURN_TCP_TIMEOUT_CANCELLED.to_string())?
        .port();
    let shim_port = shim
        .local_addr()
        .map_err(|_| TURN_TCP_TIMEOUT_CANCELLED.to_string())?
        .port();
    let inner = Arc::new(TurnTcpInner {
        closed: AtomicBool::new(false),
        tx: Mutex::new(None),
        shim: shim.clone(),
        peer: Mutex::new(
            stream
                .try_clone()
                .map_err(|_| TURN_TCP_TIMEOUT_CLOSED.to_string())?,
        ),
    });
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    *inner.tx.lock().unwrap() = Some(tx.clone());
    let queued = Arc::new(AtomicUsize::new(0));
    let local = IpAddr::V4(Ipv4Addr::LOCALHOST);
    let client_addr = SocketAddr::from((Ipv4Addr::LOCALHOST, client_port));

    let mut writer_stream = stream
        .try_clone()
        .map_err(|_| TURN_TCP_TIMEOUT_CLOSED.to_string())?;
    let writer_inner = inner.clone();
    let writer_queued = queued.clone();
    thread::spawn(move || {
        while let Ok(frame) = rx.recv() {
            let len = frame.len();
            if writer_stream.write_all(&frame).is_err() {
                writer_inner.closed.swap(true, Ordering::SeqCst);
                return;
            }
            writer_queued.fetch_sub(len, Ordering::SeqCst);
        }
    });

    let mut reader_stream = stream;
    let reader_inner = inner.clone();
    thread::spawn(move || {
        let mut pending: Vec<u8> = Vec::new();
        let mut buffer = vec![0u8; 8192];
        loop {
            if reader_inner.closed.load(Ordering::SeqCst) {
                break;
            }
            match reader_stream.read(&mut buffer) {
                Ok(0) => break,
                Ok(size) => {
                    pending.extend_from_slice(&buffer[..size]);
                    match decode_turn_tcp(&mut pending) {
                        Ok(frames) => {
                            for frame in frames {
                                let _ = reader_inner.shim.send_to(&frame, client_addr);
                            }
                        }
                        Err(_) => break,
                    }
                }
                Err(ref error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        || error.kind() == std::io::ErrorKind::TimedOut =>
                {
                    continue
                }
                Err(_) => break,
            }
        }
        reader_inner.closed.swap(true, Ordering::SeqCst);
    });

    let ingress_inner = inner.clone();
    let ingress_queued = queued;
    thread::spawn(move || {
        let mut buffer = vec![0u8; 65_536];
        loop {
            if ingress_inner.closed.load(Ordering::SeqCst) {
                break;
            }
            let (size, from) = match ingress_inner.shim.recv_from(&mut buffer) {
                Ok(value) => value,
                Err(ref error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        || error.kind() == std::io::ErrorKind::TimedOut =>
                {
                    continue
                }
                Err(_) => break,
            };
            let from_client = from.ip() == local && from.port() == client_port;
            match shim_verdict(
                &buffer[..size],
                from_client,
                ingress_queued.load(Ordering::SeqCst),
            ) {
                ShimVerdict::Ignore => continue,
                ShimVerdict::Close => break,
                ShimVerdict::Frame(frame) => {
                    ingress_queued.fetch_add(frame.len(), Ordering::SeqCst);
                    let sent = match ingress_inner.tx.lock().unwrap().as_ref() {
                        Some(sender) => sender.send(frame),
                        None => break,
                    };
                    if sent.is_err() {
                        break;
                    }
                }
            }
        }
        ingress_inner.closed.swap(true, Ordering::SeqCst);
    });

    Ok(TurnTcpChannel {
        socket: client,
        target: RudpTarget {
            address: TURN_TCP_SHIM_ADDRESS.to_string(),
            port: shim_port,
        },
        inner,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::net::TcpListener;

    fn temp_path(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "wuhuicl-ticket-{}-{}.json",
            tag,
            crate::accounts::uuid_v4()
        ))
    }

    fn service(file: PathBuf, base: &str) -> TicketService {
        let base = base.to_string();
        TicketService::new(
            file,
            Arc::new(move || base.clone()),
            Arc::new(PlainCodec),
            "1.1.21",
        )
    }

    fn stored(id: &str) -> StoredTicket {
        StoredTicket {
            id: id.to_string(),
            time_ms: js_num(1_700_000_000_000.0),
            deleted: false,
            has_unread: true,
            reply_count: js_num(2.0),
            last_time_ms: js_num(1_700_000_900_000.0),
            secret: "enc:AAA".to_string(),
            server: "https://p2p.wuhui.icu".to_string(),
        }
    }

    fn ticket_file(name: &str, size: f64) -> TicketFile {
        TicketFile {
            path: format!("C:/tmp/{}", name),
            name: name.to_string(),
            size,
            mtime: 1.0,
        }
    }

    fn aborted() -> Arc<AtomicBool> {
        Arc::new(AtomicBool::new(true))
    }

    fn live() -> Arc<AtomicBool> {
        Arc::new(AtomicBool::new(false))
    }

    #[test]
    fn valid_id_matches_ascii_word_class_only() {
        assert!(valid_id(Some(&json!("abc-1_2"))));
        assert!(valid_id(Some(&json!("ab".repeat(64)))));
        assert!(!valid_id(Some(&json!("ab".repeat(64) + "c"))));
        assert!(!valid_id(Some(&json!(""))));
        assert!(!valid_id(Some(&json!("a.b"))));
        assert!(!valid_id(Some(&json!("工单"))));
        assert!(!valid_id(Some(&json!(1))));
        assert!(!valid_id(None));
    }

    #[test]
    fn safe_count_matches_js_number_trunc_and_floor() {
        assert_eq!(safe_count(Some(&json!("5.9"))), 5.0);
        assert_eq!(safe_count(Some(&json!(" 12 "))), 12.0);
        assert_eq!(safe_count(Some(&json!("1e3"))), 1000.0);
        assert_eq!(safe_count(Some(&json!("0x10"))), 16.0);
        assert_eq!(safe_count(Some(&json!(""))), 0.0);
        assert_eq!(safe_count(Some(&json!("abc"))), 0.0);
        assert_eq!(safe_count(Some(&json!("inf"))), 0.0);
        assert_eq!(safe_count(Some(&json!("Infinity"))), f64::INFINITY);
        assert_eq!(safe_count(Some(&json!(null))), 0.0);
        assert_eq!(safe_count(Some(&Value::Null)), 0.0);
        assert_eq!(safe_count(None), 0.0);
        assert_eq!(safe_count(Some(&json!(true))), 1.0);
        assert_eq!(safe_count(Some(&json!(-3.7))), 0.0);
        assert_eq!(safe_count(Some(&json!(1e21))), 1e21);
        assert_eq!(safe_count(Some(&json!([7]))), 7.0);
        assert_eq!(safe_count(Some(&json!({}))), 0.0);
        assert_eq!(js_num(1e21).as_f64(), Some(1e21));
        assert_eq!(js_num(f64::INFINITY), Value::Null);
        assert_eq!(js_num(1700000000000.0).to_string(), "1700000000000");
    }

    #[test]
    fn utf16_length_matches_js_string_length() {
        assert_eq!(utf16_len("问题"), 2);
        assert_eq!(utf16_len("😀"), 2);
        assert_eq!(utf16_len("\u{1F600}\u{FEFF}"), 3);
    }

    #[test]
    fn attachments_slice_filter_and_truncate_name() {
        let raw = json!([
            {"name":"一".repeat(300),"size":0},
            {"name":"ok","size":1.5},
            {"name":"neg","size":-1},
            {"name":"x","size":7},
            {"size":3},
            null,
            {"name":2,"size":3}
        ]);
        let list = normalize_attachments(Some(&raw));
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name.chars().count(), ATTACH_NAME_MAX);
        assert_eq!(list[1].name, "x");
        assert_eq!(list[1].size, 7);
        assert_eq!(normalize_attachments(Some(&json!("nope"))).len(), 0);
        assert_eq!(normalize_attachments(None).len(), 0);
        let many: Vec<Value> = (0..14).map(|_| json!({"name":"n","size":1})).collect();
        assert_eq!(
            normalize_attachments(Some(&Value::Array(many))).len(),
            ATTACH_LIST_MAX
        );
    }

    #[test]
    fn visible_ticket_keeps_interface_key_order() {
        assert_eq!(
            stored("t-1").visible().to_string(),
            r#"{"id":"t-1","timeMs":1700000000000,"deleted":false,"hasUnread":true,"replyCount":2,"lastTimeMs":1700000900000}"#
        );
        assert_eq!(
            serde_json::to_string(&stored("t-1")).unwrap(),
            r#"{"id":"t-1","timeMs":1700000000000,"deleted":false,"hasUnread":true,"replyCount":2,"lastTimeMs":1700000900000,"secret":"enc:AAA","server":"https://p2p.wuhui.icu"}"#
        );
    }

    #[test]
    fn request_json_payloads_use_original_key_order() {
        assert_eq!(
            poll_body(&["a".to_string(), "b".to_string()]).to_string(),
            r#"{"ids":["a","b"]}"#
        );
        assert_eq!(poll_body(&[]).to_string(), r#"{"ids":[]}"#);
        assert_eq!(
            map_of([
                ("id", json!("t")),
                ("secret", json!("s")),
                ("text", json!("hi"))
            ])
            .to_string(),
            r#"{"id":"t","secret":"s","text":"hi"}"#
        );
        assert_eq!(
            map_of([
                ("id", json!("t")),
                ("msg", json!("m")),
                ("secret", json!("s"))
            ])
            .to_string(),
            r#"{"id":"t","msg":"m","secret":"s"}"#
        );
        assert_eq!(
            map_of([("id", json!("t")), ("secret", json!("s"))]).to_string(),
            r#"{"id":"t","secret":"s"}"#
        );
        assert_eq!(map_of([("id", json!("t"))]).to_string(), r#"{"id":"t"}"#);
    }

    #[test]
    fn client_info_pins_kamucl_identity() {
        let expected = format!(
            "{{\"client\":\"KAMUCL\",\"version\":\"1.1.21\",\"os\":\"{}\",\"arch\":\"{}\"}}",
            node_platform(),
            node_arch()
        );
        assert_eq!(client_info("1.1.21"), expected);
        assert!(client_info("1.1.21").starts_with(r#"{"client":"KAMUCL""#));
    }

    #[test]
    fn envelope_code_and_retry_follow_original_math() {
        assert_eq!(
            envelope_error_code(&json!({"error":"TICKET_FORBIDDEN"})),
            "TICKET_FORBIDDEN"
        );
        assert_eq!(
            envelope_error_code(&json!({"error":"rate_limited"})),
            "BAD_RESPONSE"
        );
        assert_eq!(envelope_error_code(&json!({"error":"123_ABC"})), "123_ABC");
        assert_eq!(
            envelope_error_code(&json!({"error":"A".repeat(81)})),
            "BAD_RESPONSE"
        );
        assert_eq!(envelope_error_code(&json!({})), "BAD_RESPONSE");
        assert_eq!(envelope_error_code(&json!({"error":5})), "BAD_RESPONSE");
        assert_eq!(
            envelope_retry_at(
                &json!({"error":"RATE_LIMITED","details":{"retryAfter":30}}),
                1_000.0
            ),
            Some(31_000.0)
        );
        assert_eq!(
            envelope_retry_at(
                &json!({"error":"RATE_LIMITED","details":{"retryAfter":90000}}),
                0.0
            ),
            Some(86_400_000.0)
        );
        assert_eq!(
            envelope_retry_at(
                &json!({"error":"RATE_LIMITED","details":{"retryAfter":"-5"}}),
                0.0
            ),
            Some(600_000.0)
        );
        assert_eq!(
            envelope_retry_at(
                &json!({"error":"RATE_LIMITED","details":{"retryAfter":30.5}}),
                0.0
            ),
            Some(30_500.0)
        );
        assert_eq!(
            envelope_retry_at(&json!({"error":"RATE_LIMITED"}), 0.0),
            Some(600_000.0)
        );
        assert_eq!(
            envelope_retry_at(&json!({"error":"BAD_RESPONSE"}), 0.0),
            None
        );
    }

    #[test]
    fn envelope_success_and_failure_texts() {
        assert_eq!(
            envelope_unwrap(&json!({"success":true,"data":{"x":1}}), true, 0.0, false).unwrap(),
            json!({"x":1})
        );
        assert_eq!(
            envelope_unwrap(&json!({"success":true}), true, 0.0, false).unwrap(),
            Value::Null
        );
        assert_eq!(
            envelope_unwrap(&json!({"success":1}), true, 0.0, false)
                .unwrap_err()
                .code,
            "BAD_RESPONSE"
        );
        assert_eq!(
            envelope_unwrap(
                &json!({"success":false,"error":"TICKET_FORBIDDEN"}),
                true,
                0.0,
                false
            ),
            Err(TicketError::new(
                "TICKET_FORBIDDEN",
                "工单操作失败 (TICKET_FORBIDDEN)"
            ))
        );
        assert_eq!(
            envelope_unwrap(
                &json!({"success":true,"error":"RATE_LIMITED"}),
                false,
                1_000.0,
                false
            )
            .unwrap_err(),
            TicketError::with_retry(
                "RATE_LIMITED",
                "请求过于频繁，请等待倒计时后重试",
                601_000.0
            )
        );
        assert_eq!(
            envelope_unwrap(&json!({"error":"oops"}), false, 0.0, false)
                .unwrap_err()
                .message,
            "工单操作失败 (BAD_RESPONSE)"
        );
        assert_eq!(
            envelope_unwrap(&Value::Null, true, 0.0, false).unwrap_err(),
            send_failure(false)
        );
        assert_eq!(
            envelope_unwrap(&Value::Null, false, 0.0, true).unwrap_err(),
            send_failure(true)
        );
        assert_eq!(send_failure(false).code, "NETWORK_ERROR");
        assert_eq!(
            send_failure(false).message,
            "工单网络请求失败，请检查网络后重试"
        );
        assert_eq!(send_failure(true).code, "CANCELLED");
        assert_eq!(send_failure(true).message, "工单请求已取消");
        assert_eq!(TicketError::new("A", "B").to_string(), "B");
    }

    #[test]
    fn detail_mapping_matches_shared_shape() {
        let data = json!({
            "id":"t-1","time":"1700000000","deleted":true,"description":"描述",
            "attachments":[{"name":"a.log","size":3}],
            "messages":[{"id":"bad.id","from":1,"time":"1700000001","text":null,"attachments":"x"},
                        {"id":"m2","from":"admin","time":1700000002,"text":"回复","attachments":[]}]
        });
        let detail = build_detail(&data, "t-1").unwrap();
        assert_eq!(
            serde_json::to_string(&detail).unwrap(),
            r#"{"id":"t-1","timeMs":1700000000000,"deleted":true,"description":"描述","attachments":[{"name":"a.log","size":3}],"messages":[{"from":"unknown","timeMs":1700000001000,"text":"","attachments":[]},{"id":"m2","from":"admin","timeMs":1700000002000,"text":"回复","attachments":[]}]}"#
        );
        assert_eq!(detail.used_bytes(), 3.0);
        for bad in [
            json!({"id":"other","description":"d","messages":[]}),
            json!({"id":"t-1","description":5,"messages":[]}),
            json!({"id":"t-1","messages":{}}),
            json!({"id":"t-1"}),
            json!({"id":"t-1","description":"d"}),
        ] {
            match build_detail(&bad, "t-1") {
                Err(TicketFailure::Ticket(error)) => {
                    assert_eq!(error.code, "BAD_RESPONSE");
                    assert_eq!(error.message, "工单详情数据不完整");
                }
                other => panic!("应判定详情不完整：{:?}", other),
            }
        }
        let many_201 = Value::Array((0..201).map(|_| json!({"id":"x"})).collect());
        assert!(build_detail(
            &json!({"id":"t-1","description":"d","messages":many_201}),
            "t-1"
        )
        .is_err());
        let exactly_200 = Value::Array((0..200).map(|_| json!({"id":"x"})).collect());
        assert!(build_detail(
            &json!({"id":"t-1","description":"d","messages":exactly_200}),
            "t-1"
        )
        .is_ok());
        assert!(matches!(
            build_detail(
                &json!({"id":"t-1","description":"d","messages":[null]}),
                "t-1"
            ),
            Err(TicketFailure::Other(_))
        ));
    }

    #[test]
    fn own_last_message_picks_newest_non_admin_with_id() {
        let detail = build_detail(
            &json!({"id":"t","description":"d","messages":[
                {"id":"a","from":"user"},{"from":"user"},{"id":"b","from":"admin"},{"id":"c","from":"user"}
            ]}),
            "t",
        )
        .unwrap();
        assert_eq!(
            detail
                .own_last_message()
                .and_then(|message| message.id.as_deref()),
            Some("c")
        );
        let none = build_detail(
            &json!({"id":"t","description":"d","messages":[{"from":"admin"}]}),
            "t",
        )
        .unwrap();
        assert!(none.own_last_message().is_none());
    }

    #[test]
    fn guards_texts_and_thresholds() {
        assert_eq!(
            validate_description(None).unwrap_err().message,
            "请填写 1–10000 字的问题描述"
        );
        assert_eq!(
            validate_description(Some("   ")).unwrap_err().message,
            "请填写 1–10000 字的问题描述"
        );
        assert_eq!(
            validate_description(Some("\u{FEFF}")).unwrap_err().code,
            "INVALID_DESCRIPTION"
        );
        let desc_max = "字".repeat(TICKET_DESCRIPTION_MAX);
        let desc_over = "字".repeat(TICKET_DESCRIPTION_MAX + 1);
        let emoji_max = "😀".repeat(TICKET_DESCRIPTION_MAX / 2);
        let emoji_over = "😀".repeat(TICKET_DESCRIPTION_MAX / 2 + 1);
        assert!(validate_description(Some(desc_max.as_str())).is_ok());
        assert_eq!(
            validate_description(Some(desc_over.as_str()))
                .unwrap_err()
                .code,
            "INVALID_DESCRIPTION"
        );
        assert!(validate_description(Some(emoji_max.as_str())).is_ok());
        assert_eq!(
            validate_description(Some(emoji_over.as_str()))
                .unwrap_err()
                .code,
            "INVALID_DESCRIPTION"
        );
        assert_eq!(
            validate_message(None, 0).unwrap_err().message,
            "追问最多 2000 字，文字与附件不能都为空"
        );
        assert_eq!(
            validate_message(Some(""), 0).unwrap_err().message,
            "追问最多 2000 字，文字与附件不能都为空"
        );
        assert!(validate_message(Some(""), 1).is_ok());
        let text_max = "字".repeat(TICKET_MESSAGE_MAX);
        let text_over = "字".repeat(TICKET_MESSAGE_MAX + 1);
        assert!(validate_message(Some(text_max.as_str()), 0).is_ok());
        assert_eq!(
            validate_message(Some(text_over.as_str()), 1)
                .unwrap_err()
                .code,
            "INVALID_MESSAGE"
        );
        assert_eq!(
            submitted_ticket(&json!({"id":"t","ticketSecret":""}))
                .unwrap_err()
                .message,
            "服务器未返回工单归属凭证"
        );
        assert_eq!(
            submitted_ticket(&json!({"id":"t"})).unwrap_err().code,
            "BAD_RESPONSE"
        );
        assert_eq!(
            submitted_ticket(&json!({"id":"bad.id","ticketSecret":"s"}))
                .unwrap_err()
                .code,
            "BAD_RESPONSE"
        );
        assert_eq!(
            submitted_ticket(&json!({"id":"t","ticketSecret":"enc:1"})).unwrap(),
            ("t".to_string(), "enc:1".to_string())
        );
        assert_eq!(
            attachment_guard(&[ticket_file("a", TICKET_BYTES_MAX + 1.0)])
                .unwrap_err()
                .message,
            "每条消息最多 10 个附件，附件总量不得超过 500 MB"
        );
        assert_eq!(
            attachment_guard(&[ticket_file("a", TICKET_BYTES_MAX)]),
            Ok(())
        );
        let eleven: Vec<TicketFile> = (0..11).map(|i| ticket_file(&i.to_string(), 1.0)).collect();
        assert_eq!(
            attachment_guard(&eleven).unwrap_err().code,
            "TICKET_TOO_LARGE"
        );
        let ten: Vec<TicketFile> = (0..10).map(|i| ticket_file(&i.to_string(), 1.0)).collect();
        assert_eq!(attachment_guard(&ten), Ok(()));
        assert_eq!(
            duplicate_failure(Some(("你好", 1_000.0)), "你好", 15_999.0)
                .unwrap()
                .message,
            "同一段追问请间隔 15 秒再发送"
        );
        assert_eq!(
            duplicate_failure(Some(("你好", 1_000.0)), "你好", 16_000.0),
            None
        );
        assert_eq!(
            duplicate_failure(Some(("你好", 1_000.0)), "你 好", 1_001.0),
            None
        );
        assert_eq!(duplicate_failure(None, "你好", 1_001.0), None);
    }

    #[test]
    fn response_and_capacity_guard_texts() {
        assert_eq!(response_size_error(RESPONSE_LIMIT), None);
        assert_eq!(
            response_size_error(RESPONSE_LIMIT + 1).unwrap(),
            TicketError::new("BAD_RESPONSE", "工单响应过大")
        );
        assert_eq!(RESPONSE_LIMIT, 4 * 1024 * 1024);
        assert_eq!(parse_ticket_response(b"{\"success\":true}")["success"], json!(true));
        assert_eq!(
            parse_ticket_response(b"not json").unwrap_err(),
            TicketError::new("BAD_RESPONSE", "工单服务器返回无效数据")
        );
        assert_eq!(
            parse_ticket_response(b"").unwrap_err().message,
            "工单服务器返回无效数据"
        );
        let detail = build_detail(
            &json!({"id":"t","time":1,"deleted":false,"description":"d",
                    "attachments":[{"name":"a","size":10}],
                    "messages":[{"id":"m1","from":"user","time":2,"text":"x","attachments":[{"name":"b","size":20}]}]}),
            "t",
        )
        .unwrap();
        assert_eq!(detail.used_bytes(), 30.0);
        assert!(reply_capacity_guard(&detail, &[ticket_file("c", 5.0)]).is_ok());
        assert_eq!(
            reply_capacity_guard(&detail, &[ticket_file("c", TICKET_BYTES_MAX)])
                .unwrap_err()
                .message,
            "该工单附件总量不得超过 500 MB"
        );
        let full = Value::Array(
            (0..TICKET_MESSAGE_COUNT_MAX)
                .map(|i| json!({"id":format!("m{i}"),"from":"user","time":1,"text":"x","attachments":[]}))
                .collect(),
        );
        let at_limit = build_detail(&json!({"id":"t","description":"d","messages":full}), "t").unwrap();
        assert_eq!(
            reply_capacity_guard(&at_limit, &[]).unwrap_err().message,
            "工单已达到 200 条消息上限"
        );
        assert!(retract_guard(&detail, "m1").is_ok());
        assert_eq!(
            retract_guard(&detail, "m2").unwrap_err().message,
            "只能撤回自己的上一条追问"
        );
        assert_eq!(retract_guard(&at_limit, "m0").unwrap_err().code, "TICKET_FORBIDDEN");
    }

    #[test]
    fn attachment_mismatch_covers_dir_size_and_mtime() {
        let dir = std::env::temp_dir().join(format!("wuhuicl-ticket-stat-{}", crate::accounts::uuid_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("x.bin");
        std::fs::write(&path, b"12345").unwrap();
        let meta = std::fs::metadata(&path).unwrap();
        let good = TicketFile {
            path: path.to_string_lossy().to_string(),
            name: "x.bin".to_string(),
            size: 5.0,
            mtime: system_mtime_ms(&meta),
        };
        assert_eq!(attachment_mismatch(&meta, &good), None);
        let wrong_size = TicketFile { size: 6.0, ..good.clone() };
        assert_eq!(
            attachment_mismatch(&meta, &wrong_size).unwrap(),
            TicketError::new("ATTACHMENT_CHANGED", "x.bin 已变化，请重新选择")
        );
        let wrong_mtime = TicketFile {
            mtime: good.mtime + 1.0,
            ..good.clone()
        };
        assert_eq!(attachment_mismatch(&meta, &wrong_mtime).unwrap().code, "ATTACHMENT_CHANGED");
        let dir_meta = std::fs::metadata(&dir).unwrap();
        assert!(attachment_mismatch(&dir_meta, &good).is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rate_limits_cooldown_then_three_per_window() {
        let now = 1_000_000.0;
        let error = rate_failure(now + 5_000.0, &[], now).unwrap();
        assert_eq!(
            (error.code.as_str(), error.message.as_str(), error.retry_at),
            (
                "RATE_LIMITED",
                "请求过于频繁，请稍后重试",
                Some(now + 5_000.0)
            )
        );
        let history = [now - 10.0, now - 20.0, now - 30.0];
        let error = rate_failure(0.0, &history, now).unwrap();
        assert_eq!(error.message, "每 10 分钟最多提交 3 次，请稍后重试");
        assert_eq!(error.retry_at, Some(now - 10.0 + RATE_WINDOW_MS));
        assert_eq!(rate_failure(0.0, &history[..2], now), None);
        assert_eq!(rate_failure(0.0, &[], now), None);
        let service = service(temp_path("rate"), "https://a.test");
        for _ in 0..RATE_MAX {
            assert!(service.rate("/ticket/submit").is_ok());
            service
                .state
                .lock()
                .unwrap()
                .history
                .get_mut("/ticket/submit")
                .unwrap()
                .push(now_ms());
        }
        assert_eq!(
            service.rate("/ticket/submit").unwrap_err().message,
            "每 10 分钟最多提交 3 次，请稍后重试"
        );
        assert!(service.rate("/ticket/reply").is_ok());
    }

    #[test]
    fn multipart_layout_is_byte_exact() {
        let boundary = multipart_boundary("11112222-3333-4444-5555-666677778888");
        assert_eq!(boundary, "----VoxLinkTK11112222333344445555666677778888");
        let part = field_part(&boundary, "description", "问题");
        assert_eq!(
            String::from_utf8(part.clone()).unwrap(),
            "----VoxLinkTK11112222333344445555666677778888\r\nContent-Disposition: form-data; name=\"description\"\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n问题\r\n"
        );
        assert_eq!(sanitize_filename("日志\"名\r\n"), "日志_名__");
        let head = file_head(&boundary, "日志\"名\r\n");
        assert_eq!(
            String::from_utf8(head.clone()).unwrap(),
            "----VoxLinkTK11112222333344445555666677778888\r\nContent-Disposition: form-data; name=\"attachments\"; filename=\"日志_名__\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        );
        let end = multipart_end(&boundary);
        assert_eq!(
            String::from_utf8(end.clone()).unwrap(),
            "----VoxLinkTK11112222333344445555666677778888--\r\n"
        );
        let total = multipart_total(
            &[part.clone()],
            &[head.clone()],
            &[ticket_file("a", 5.0), ticket_file("b", 7.0)],
            &end,
        );
        assert_eq!(
            total,
            part.len() as f64 + head.len() as f64 + 5.0 + 2.0 + 7.0 + 2.0 + end.len() as f64
        );
    }

    /// 走完整 Read 路径，返回（实到字节, 申报总长, 进度上报序列）
    fn drain_body(payload: &[u8]) -> (Vec<u8>, Vec<u8>, f64, Vec<(f64, f64)>) {
        let dir =
            std::env::temp_dir().join(format!("wuhuicl-ticket-{}", crate::accounts::uuid_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.bin");
        std::fs::write(&path, payload).unwrap();
        let mtime = system_mtime_ms(&std::fs::metadata(&path).unwrap());
        let boundary = "----VoxLinkTKt";
        let part = field_part(boundary, "text", "hi");
        let head = file_head(boundary, "a.bin");
        let end = multipart_end(boundary);
        let file = TicketFile {
            path: path.to_string_lossy().to_string(),
            name: "a.bin".to_string(),
            size: payload.len() as f64,
            mtime,
        };
        let total = multipart_total(&[part.clone()], &[head.clone()], &[file.clone()], &end);
        let mut expected = Vec::new();
        expected.extend_from_slice(&part);
        expected.extend_from_slice(&head);
        expected.extend_from_slice(payload);
        expected.extend_from_slice(b"\r\n");
        expected.extend_from_slice(&end);
        assert_eq!(total, expected.len() as f64);

        let mut plan = VecDeque::new();
        plan.push_back(ChunkSpec::Bytes(part));
        plan.push_back(ChunkSpec::Bytes(head));
        plan.push_back(ChunkSpec::FileRun(0));
        plan.push_back(ChunkSpec::Bytes(b"\r\n".to_vec()));
        plan.push_back(ChunkSpec::Bytes(end));
        let reports = Arc::new(Mutex::new(Vec::<(f64, f64)>::new()));
        let sink = reports.clone();
        let mut body = MultipartBody {
            plan,
            files: vec![File::open(&path).unwrap()],
            read_bytes: vec![0u64; 1],
            sizes: vec![payload.len() as u64],
            current: Vec::new(),
            offset: 0,
            finished: false,
            sent: 0.0,
            total,
            cancelled: Arc::new(AtomicBool::new(false)),
            progress: Some(Arc::new(move |bytes, sum| {
                sink.lock().unwrap().push((bytes, sum))
            })),
            error: Arc::new(Mutex::new(None)),
        };
        let mut streamed = Vec::new();
        let mut scratch = vec![0u8; 4096];
        loop {
            let size = body.read(&mut scratch).unwrap();
            if size == 0 {
                break;
            }
            streamed.extend_from_slice(&scratch[..size]);
        }
        let _ = std::fs::remove_dir_all(&dir);
        (streamed, expected, total, reports.lock().unwrap().clone())
    }

    #[test]
    fn multipart_body_streams_declared_bytes_and_reports_progress() {
        let (streamed, expected, total, reports) = drain_body(b"0123456789");
        assert_eq!(streamed, expected);
        let part = field_part("----VoxLinkTKt", "text", "hi");
        let head = file_head("----VoxLinkTKt", "a.bin");
        assert_eq!(reports.len(), 5);
        assert_eq!(reports[0].0, part.len() as f64);
        assert_eq!(reports[0].1, total);
        assert_eq!(reports[1].0, part.len() as f64 + head.len() as f64);
        assert_eq!(reports[2].0, part.len() as f64 + head.len() as f64 + 10.0);
        assert_eq!(reports[3].0, part.len() as f64 + head.len() as f64 + 12.0);
        assert_eq!(reports[4], (total, total));
        assert!(streamed
            .starts_with(b"----VoxLinkTKt\r\nContent-Disposition: form-data; name=\"text\""));
        assert!(streamed.ends_with(b"----VoxLinkTKt--\r\n"));
    }

    #[test]
    fn multipart_body_splits_large_files_at_64kib() {
        let payload: Vec<u8> = (0..70_000u32).map(|i| (i % 251) as u8).collect();
        let (streamed, expected, total, reports) = drain_body(&payload);
        assert_eq!(streamed, expected);
        assert_eq!(reports.len(), 6);
        assert_eq!(reports[2].0 - reports[1].0, UPLOAD_CHUNK as f64);
        assert_eq!(reports[3].0 - reports[2].0, (70_000 - UPLOAD_CHUNK) as f64);
        assert_eq!(reports[4].0 - reports[3].0, 2.0);
        assert_eq!(*reports.last().unwrap(), (total, total));
    }

    #[test]
    fn multipart_reader_flags_changed_attachment() {
        let dir = std::env::temp_dir().join(format!(
            "wuhuicl-ticket-short-{}",
            crate::accounts::uuid_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s.bin");
        std::fs::write(&path, b"12345").unwrap();
        let error_slot = Arc::new(Mutex::new(None));
        let mut plan = VecDeque::new();
        plan.push_back(ChunkSpec::FileRun(0));
        let mut body = MultipartBody {
            plan,
            files: vec![File::open(&path).unwrap()],
            read_bytes: vec![0u64; 1],
            sizes: vec![9],
            current: Vec::new(),
            offset: 0,
            finished: false,
            sent: 0.0,
            total: 9.0,
            cancelled: Arc::new(AtomicBool::new(false)),
            progress: None,
            error: error_slot.clone(),
        };
        let mut out = vec![0u8; 64];
        assert_eq!(body.read(&mut out).unwrap(), 5);
        assert!(body.read(&mut out).is_err());
        assert_eq!(
            *error_slot.lock().unwrap(),
            Some(TicketError::new("ATTACHMENT_CHANGED", "上传期间附件已变化"))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn multipart_reader_stops_when_cancelled() {
        let mut plan = VecDeque::new();
        plan.push_back(ChunkSpec::Bytes(b"abc".to_vec()));
        let flag = Arc::new(AtomicBool::new(true));
        let mut body = MultipartBody {
            plan,
            files: Vec::new(),
            read_bytes: Vec::new(),
            sizes: Vec::new(),
            current: Vec::new(),
            offset: 0,
            finished: false,
            sent: 0.0,
            total: 3.0,
            cancelled: flag,
            progress: None,
            error: Arc::new(Mutex::new(None)),
        };
        let mut out = vec![0u8; 8];
        assert!(body.read(&mut out).is_err());
    }

    #[test]
    fn ticket_url_matches_urlsearchparams_encoding() {
        let url = service(temp_path("url"), "https://p2p.wuhui.icu")
            .ticket_url(
                "/ticket/detail",
                &[("id", "abc-1_2"), ("secret", "enc:Ab+c/d==*~%.")],
            )
            .unwrap();
        assert_eq!(url, "https://p2p.wuhui.icu/?route=%2Fticket%2Fdetail&id=abc-1_2&secret=enc%3AAb%2Bc%2Fd%3D%3D*%7E%25.");
        assert_eq!(
            service(temp_path("url"), "https://p2p.wuhui.icu")
                .ticket_url("/ticket/poll", &[])
                .unwrap(),
            "https://p2p.wuhui.icu/?route=%2Fticket%2Fpoll"
        );
        let kept = service(temp_path("url"), "https://p2p.wuhui.icu/?lang=fr&route=old")
            .ticket_url("/ticket/poll", &[])
            .unwrap();
        assert_eq!(
            kept,
            "https://p2p.wuhui.icu/?lang=fr&route=%2Fticket%2Fpoll"
        );
        for base in [
            "ftp://p2p.wuhui.icu",
            "https://",
            "//p2p.wuhui.icu",
            "not a url",
        ] {
            let error = service(temp_path("url"), base)
                .ticket_url("/ticket/poll", &[])
                .unwrap_err();
            assert_eq!(
                (error.code.as_str(), error.message.as_str()),
                ("BAD_SERVER_URL", "工单服务器地址无效"),
                "{}",
                base
            );
        }
    }

    #[test]
    fn store_missing_file_is_silent_and_bad_index_is_store_invalid() {
        let broken = temp_path("broken");
        std::fs::write(&broken, "{\"rows\":[]}").unwrap();
        let svc = service(broken.clone(), "https://a.test");
        assert_eq!(
            svc.list().unwrap_err(),
            TicketFailure::ticket("STORE_INVALID", "本地工单索引无法读取，已保留原文件")
        );
        std::fs::write(&broken, "[]").unwrap();
        assert_eq!(svc.list().unwrap(), Vec::<Value>::new());

        let fresh = service(temp_path("missing"), "https://a.test");
        assert_eq!(fresh.list().unwrap(), Vec::<Value>::new());
        let _ = std::fs::remove_file(&broken);
    }

    #[test]
    fn load_drops_rows_without_id_secret_or_server() {
        let file = temp_path("filter");
        std::fs::write(
            &file,
            json!([
                {"id":"ok","secret":"s","server":"https://a.test","timeMs":"9.7","lastTimeMs":null,"replyCount":"2","deleted":"yes","hasUnread":true},
                {"id":"bad id","secret":"s","server":"https://a.test"},
                {"id":"nosecret","server":"https://a.test"},
                {"id":"badserver","secret":"s","server":"ftp://a.test"},
                {"secret":"s","server":"https://a.test"},
                null,
                [1,2],
                "text"
            ]),
        )
        .unwrap();
        let rows = service(file.clone(), "https://a.test").list().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].to_string(),
            r#"{"id":"ok","timeMs":9,"deleted":false,"hasUnread":true,"replyCount":2,"lastTimeMs":0}"#
        );
        let _ = std::fs::remove_file(&file);
    }

    #[test]
    fn list_filters_by_server_and_sorts_by_last_then_first_time() {
        let file = temp_path("list");
        std::fs::write(
            &file,
            json!([
                {"id":"old","secret":"s","server":"https://a.test","timeMs":10,"lastTimeMs":0,"deleted":false,"hasUnread":false,"replyCount":0},
                {"id":"new","secret":"s","server":"https://a.test","timeMs":1,"lastTimeMs":99,"deleted":false,"hasUnread":true,"replyCount":1},
                {"id":"gone","secret":"s","server":"https://a.test","timeMs":500,"lastTimeMs":500,"deleted":true,"hasUnread":false,"replyCount":0},
                {"id":"other","secret":"s","server":"https://b.test","timeMs":800,"lastTimeMs":800,"deleted":false,"hasUnread":false,"replyCount":0}
            ]),
        )
        .unwrap();
        let service = service(file.clone(), "https://a.test");
        let rows = service.list().unwrap();
        let ids: Vec<&str> = rows.iter().map(|row| row["id"].as_str().unwrap()).collect();
        assert_eq!(ids, vec!["new", "old"]);
        assert_eq!(
            rows[0].to_string(),
            r#"{"id":"new","timeMs":1,"deleted":false,"hasUnread":true,"replyCount":1,"lastTimeMs":99}"#
        );
        for id in ["gone", "nope", "other"] {
            assert_eq!(
                service.owned(id).unwrap_err(),
                TicketFailure::ticket("UNKNOWN_TICKET", "本机没有该工单的归属凭证"),
                "{}",
                id
            );
        }
        let _ = std::fs::remove_file(&file);
    }

    #[test]
    fn local_guards_run_before_any_network_call() {
        let service = service(temp_path("guards"), "https://a.test");
        let error = service
            .reply("nope", Some("hi"), &[], &live(), None)
            .unwrap_err();
        assert_eq!(
            error,
            TicketFailure::ticket("UNKNOWN_TICKET", "本机没有该工单的归属凭证")
        );
        assert_eq!(
            service.detail("nope", &live()).unwrap_err().code(),
            "UNKNOWN_TICKET"
        );
        assert_eq!(
            service.retract("nope", "m", &live()).unwrap_err().code(),
            "UNKNOWN_TICKET"
        );
        assert_eq!(
            service.remove("nope", &live()).unwrap_err().code(),
            "UNKNOWN_TICKET"
        );
        assert_eq!(
            service.submit(None, &[], &live(), None).unwrap_err().code(),
            "INVALID_DESCRIPTION"
        );
        assert_eq!(
            service
                .reply("nope", None, &[], &live(), None)
                .unwrap_err()
                .code(),
            "UNKNOWN_TICKET"
        );
    }

    #[test]
    fn poll_once_aborts_before_first_request_and_marks_polled() {
        let file = temp_path("poll");
        std::fs::write(
            &file,
            json!([{"id":"x","secret":"s","server":"https://a.test","timeMs":1,"lastTimeMs":1,"deleted":false,"hasUnread":false,"replyCount":0}]),
        )
        .unwrap();
        let service = service(file.clone(), "https://a.test");
        assert_eq!(
            service.poll_once(&aborted()).unwrap_err(),
            TicketFailure::Other("This operation was aborted".to_string())
        );
        assert_eq!(service.poll_once(&aborted()).unwrap().len(), 1);
        let _ = std::fs::remove_file(&file);
    }

    #[test]
    fn secret_unavailable_texts() {
        struct Broken;
        impl SecretCodec for Broken {
            fn seal(&self, text: &str) -> Result<String, TicketError> {
                Ok(text.to_string())
            }
            fn open(&self, _text: &str) -> Result<String, TicketError> {
                Err(TicketError::new(
                    "SECRET_UNAVAILABLE",
                    "系统凭证保护不可用，请启用系统密钥服务后提交",
                ))
            }
        }
        let service = TicketService::new(
            temp_path("codec"),
            Arc::new(|| "https://a.test".to_string()),
            Arc::new(Broken),
            "1.1.21",
        );
        assert_eq!(
            (
                service.secret("enc:xxx").unwrap_err().code.as_str(),
                service.secret("enc:xxx").unwrap_err().message.as_str()
            ),
            (
                "SECRET_UNAVAILABLE",
                "工单归属凭证无法解密，请使用最初提交工单的系统账号"
            )
        );

        struct Flipped;
        impl SecretCodec for Flipped {
            fn seal(&self, text: &str) -> Result<String, TicketError> {
                Ok(format!("{}!", text))
            }
            fn open(&self, text: &str) -> Result<String, TicketError> {
                Ok(text.to_string())
            }
        }
        let service = TicketService::new(
            temp_path("flip"),
            Arc::new(|| "https://a.test".to_string()),
            Arc::new(Flipped),
            "1.1.21",
        );
        assert_eq!(
            service
                .submit(Some("问题"), &[], &live(), None)
                .unwrap_err(),
            TicketFailure::ticket("SECRET_UNAVAILABLE", "系统凭证保护无法验证，请稍后再提交")
        );
    }

    #[test]
    fn framed_packet_is_big_endian_length_prefixed() {
        let packet: Vec<u8> = (0..16u8).collect();
        let frame = tcp_frame(&packet);
        assert_eq!(&frame[..2], &[0x00, 0x10]);
        assert_eq!(&frame[2..], packet.as_slice());
        assert_eq!(frame.len(), 18);
        assert_eq!(tcp_frame(&[]), vec![0u8, 0u8]);
        assert_eq!(&tcp_frame(&[7u8; TURN_TCP_MAX_PACKET])[..2], &[0x08, 0x00]);
        assert_eq!(
            tcp_frame(&[7u8; TURN_TCP_MAX_PACKET]).len(),
            TURN_TCP_MAX_PACKET + TURN_TCP_FRAME_PREFIX
        );
        assert_eq!(tcp_frame(&[1, 2, 3]), vec![0x00, 0x03, 1, 2, 3]);
        let mut three_hundred = vec![0x01u8, 0x2c];
        three_hundred.extend_from_slice(&[0xAA; 300]);
        assert_eq!(tcp_frame(&[0xAA; 300]), three_hundred);
    }

    #[test]
    fn shim_drops_oversize_empty_and_foreign_packets() {
        let packet = vec![1u8; TURN_TCP_MAX_PACKET];
        assert_eq!(
            shim_verdict(&packet, true, 0),
            ShimVerdict::Frame(tcp_frame(&packet))
        );
        assert_eq!(
            shim_verdict(&vec![1u8; TURN_TCP_MAX_PACKET + 1], true, 0),
            ShimVerdict::Ignore
        );
        assert_eq!(shim_verdict(&[], true, 0), ShimVerdict::Ignore);
        assert_eq!(shim_verdict(&packet, false, 0), ShimVerdict::Ignore);
        assert_eq!(
            shim_verdict(&packet, true, TURN_TCP_BACKPRESSURE_BYTES),
            ShimVerdict::Frame(tcp_frame(&packet))
        );
        assert_eq!(
            shim_verdict(&packet, true, TURN_TCP_BACKPRESSURE_BYTES + 1),
            ShimVerdict::Close
        );
        assert_eq!(TURN_TCP_BACKPRESSURE_BYTES, 1024 * 1024);
        assert_eq!(TURN_TCP_MAX_PACKET, 2048);
    }

    #[test]
    fn decoder_splits_stream_and_rejects_bad_sizes() {
        let mut pending = Vec::new();
        pending.extend_from_slice(&tcp_frame(&[1, 2, 3]));
        pending.push(0x00);
        assert_eq!(
            decode_turn_tcp(&mut pending).unwrap(),
            vec![vec![1u8, 2, 3]]
        );
        assert_eq!(pending, vec![0u8]);
        pending.extend_from_slice(&tcp_frame(&[4])[1..]);
        assert_eq!(decode_turn_tcp(&mut pending).unwrap(), vec![vec![4u8]]);
        assert!(pending.is_empty());
        let mut zero = vec![0u8, 0u8, 9u8];
        assert_eq!(decode_turn_tcp(&mut zero), Err(()));
        let mut oversize = vec![0x08u8, 0x01];
        assert_eq!(decode_turn_tcp(&mut oversize), Err(()));
        let mut half = vec![0u8];
        assert_eq!(decode_turn_tcp(&mut half).unwrap(), Vec::<Vec<u8>>::new());
        assert_eq!(half.len(), 1);
        let mut max_ok = vec![0x08u8, 0x00];
        max_ok.extend_from_slice(&[5u8; TURN_TCP_MAX_PACKET]);
        assert_eq!(
            decode_turn_tcp(&mut max_ok).unwrap(),
            vec![vec![5u8; TURN_TCP_MAX_PACKET]]
        );
        let mut many = Vec::new();
        many.extend_from_slice(&tcp_frame(&[9]));
        many.extend_from_slice(&tcp_frame(&[8, 8]));
        assert_eq!(
            decode_turn_tcp(&mut many).unwrap(),
            vec![vec![9u8], vec![8u8, 8]]
        );
        assert!(many.is_empty());
    }

    #[test]
    fn turn_tcp_error_texts_are_the_three_originals() {
        assert_eq!(TURN_TCP_TIMEOUT_CONNECT, "TURN TCP 连接超时");
        assert_eq!(TURN_TCP_TIMEOUT_CLOSED, "TURN TCP 已关闭");
        assert_eq!(TURN_TCP_TIMEOUT_CANCELLED, "TURN TCP 已取消");
        assert_eq!(TURN_TCP_CONNECT_TIMEOUT_MS, 3000);
        assert_eq!(
            open_turn_tcp(TURN_TCP_SHIM_ADDRESS, 9, &aborted()).err(),
            Some(TURN_TCP_TIMEOUT_CANCELLED.to_string())
        );
        let closed_port = TcpListener::bind((TURN_TCP_SHIM_ADDRESS, 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        assert_eq!(
            open_turn_tcp(TURN_TCP_SHIM_ADDRESS, closed_port, &live()).err(),
            Some(TURN_TCP_TIMEOUT_CLOSED.to_string())
        );
    }

    fn accept_deadline(listener: &TcpListener) -> TcpStream {
        let deadline = Instant::now() + Duration::from_secs(4);
        loop {
            match listener.accept() {
                Ok((stream, _)) => return stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "未在期限内建立 TCP 连接");
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("accept 失败：{}", error),
            }
        }
    }

    fn read_deadline(stream: &mut TcpStream, out: &mut [u8]) {
        stream.set_nonblocking(false).unwrap();
        let deadline = Instant::now() + Duration::from_secs(4);
        let mut done = 0usize;
        while done < out.len() {
            stream
                .set_read_timeout(Some(Duration::from_millis(200)))
                .unwrap();
            match stream.read(&mut out[done..]) {
                Ok(0) => panic!("隧道被对端关闭"),
                Ok(size) => done += size,
                Err(_) => assert!(Instant::now() < deadline, "未在期限内读到完整帧"),
            }
        }
    }

    #[test]
    fn turn_tcp_loopback_tunnel_round_trips_frames() {
        let listener = TcpListener::bind((TURN_TCP_SHIM_ADDRESS, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        listener.set_nonblocking(true).unwrap();
        let channel = open_turn_tcp(TURN_TCP_SHIM_ADDRESS, port, &live()).unwrap();
        assert_eq!(channel.target.address, TURN_TCP_SHIM_ADDRESS);
        assert_ne!(channel.target.port, 0);
        assert!(!channel.is_closed());

        channel
            .socket
            .send_to(b"ping", (TURN_TCP_SHIM_ADDRESS, channel.target.port))
            .unwrap();
        let mut peer = accept_deadline(&listener);
        let mut head = [0u8; 2];
        read_deadline(&mut peer, &mut head);
        assert_eq!(head, [0x00, 0x04]);
        let mut body = [0u8; 4];
        read_deadline(&mut peer, &mut body);
        assert_eq!(&body, b"ping");

        peer.write_all(&tcp_frame(b"pong")).unwrap();
        channel
            .socket
            .set_read_timeout(Some(Duration::from_millis(2_000)))
            .unwrap();
        let mut datagram = [0u8; 16];
        let (size, from) = channel.socket.recv_from(&mut datagram).unwrap();
        assert_eq!(&datagram[..size], &b"pong"[..]);
        assert_eq!(from.ip(), IpAddr::V4(Ipv4Addr::LOCALHOST));
        assert_eq!(from.port(), channel.target.port);

        channel.close();
        assert!(channel.is_closed());
        channel.close();
        listener.set_nonblocking(false).unwrap();
    }

    #[test]
    fn turn_tcp_drops_oversize_datagram_before_tcp() {
        let listener = TcpListener::bind((TURN_TCP_SHIM_ADDRESS, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        listener.set_nonblocking(true).unwrap();
        let channel = open_turn_tcp(TURN_TCP_SHIM_ADDRESS, port, &live()).unwrap();
        channel
            .socket
            .send_to(
                &vec![0u8; TURN_TCP_MAX_PACKET + 1],
                (TURN_TCP_SHIM_ADDRESS, channel.target.port),
            )
            .unwrap();
        channel
            .socket
            .send_to(
                &vec![3u8; TURN_TCP_MAX_PACKET],
                (TURN_TCP_SHIM_ADDRESS, channel.target.port),
            )
            .unwrap();
        let mut peer = accept_deadline(&listener);
        let mut head = [0u8; 2];
        read_deadline(&mut peer, &mut head);
        assert_eq!(head, [0x08, 0x00]);
        channel.close();
        listener.set_nonblocking(false).unwrap();
    }
}
