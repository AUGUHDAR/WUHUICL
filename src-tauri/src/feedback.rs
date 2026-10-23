//! feedback.rs — 「反馈与建议」：对接 wuhuicl-feedback 服务端（wuhuicl.voxdev.top）。
//!
//! 两套体系（与服务端一致）：
//! - 公开板 issue：浏览匿名可行；提交/评论需微软登录（会话 token 存本地 feedback.json）。
//! - 匿名私信 ticket：单号+密钥存本地（voxlink 离线语义：服务端删→poll removed→本地同步删；
//!   本地删→服务端仅软删保留）。
//!
//! 登录：配对码——本模块取码后在系统浏览器打开网页配对页，网页授权后轮询领取会话。
//! 启动器自身的游戏 token 永不发给反馈服务器。
//!
//! 附件：日志文件以原始字节 multipart 上传（绝不过 base64），gzip 由服务端处理。

use serde_json::{json, Value};
use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tauri::{AppHandle, Manager};

// 服务端基址：默认公网；FB_BASE 环境变量可指到本地实例联调。
fn fb_base() -> String {
    std::env::var("FB_BASE")
        .map(|v| v.trim_end_matches('/').to_string())
        .unwrap_or_else(|_| "https://wuhuicl.voxdev.top".to_string())
}

fn api_client() -> &'static reqwest::blocking::Client {
    static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .expect("feedback HTTP 客户端初始化失败")
    })
}

fn store_path(app: &AppHandle) -> PathBuf {
    crate::settings::user_data_dir(app).join("feedback.json")
}

fn load_store(app: &AppHandle) -> Value {
    let p = store_path(app);
    fs::read_to_string(&p)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({ "session": "", "identity": null, "tickets": [] }))
}

fn save_store(app: &AppHandle, v: &Value) {
    let p = store_path(app);
    if let Some(dir) = p.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let _ = fs::write(&p, serde_json::to_vec_pretty(v).unwrap_or_default());
}

fn session(app: &AppHandle) -> String {
    load_store(app)
        .get("session")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn api_get(app: &AppHandle, route: &str, params: &[(&str, String)]) -> Result<Value, String> {
    let mut url = format!("{}{}", fb_base(), route);
    if !params.is_empty() {
        let q: Vec<String> = params
            .iter()
            .map(|(k, v)| format!("{}={}", k, urlencoding(v)))
            .collect();
        url.push('?');
        url.push_str(&q.join("&"));
    }
    let mut req = api_client().get(&url);
    let tok = session(app);
    if !tok.is_empty() {
        req = req.header("Authorization", format!("Bearer {}", tok));
    }
    let resp = req.send().map_err(|e| format!("网络错误：{e}"))?;
    let body: Value = resp.json().map_err(|e| format!("响应解析失败：{e}"))?;
    unwrap_ok(body)
}

fn api_post_json(app: &AppHandle, route: &str, payload: &Value) -> Result<Value, String> {
    let mut req = api_client().post(format!("{}{}", fb_base(), route));
    let tok = session(app);
    if !tok.is_empty() {
        req = req.header("Authorization", format!("Bearer {}", tok));
    }
    let resp = req
        .json(payload)
        .send()
        .map_err(|e| format!("网络错误：{e}"))?;
    let body: Value = resp.json().map_err(|e| format!("响应解析失败：{e}"))?;
    unwrap_ok(body)
}

fn unwrap_ok(body: Value) -> Result<Value, String> {
    if body.get("success").and_then(|v| v.as_bool()) == Some(true) {
        return Ok(body.get("data").cloned().unwrap_or(json!({})));
    }
    let code = body.get("error").and_then(|v| v.as_str()).unwrap_or("UNKNOWN");
    let msg = body.get("message").and_then(|v| v.as_str()).unwrap_or(code);
    let retry = body
        .pointer("/details/retryAfter")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    if retry > 0 {
        return Err(format!("{msg}（请 {retry} 秒后重试）"));
    }
    Err(msg.to_string())
}

fn urlencoding(s: &str) -> String {
    let mut out = String::new();
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(*b as char),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

// ---- multipart（手写边界，原始字节直传）----

struct MpForm {
    boundary: String,
    buf: Vec<u8>,
}

impl MpForm {
    fn new() -> Self {
        Self { boundary: format!("----wuhuiclfb{}", rand_hex()), buf: Vec::new() }
    }
    fn field(&mut self, name: &str, value: &str) {
        self.buf.extend_from_slice(format!("--{}\r\nContent-Disposition: form-data; name=\"{}\"\r\n\r\n{}\r\n", self.boundary, name, value).as_bytes());
    }
    fn file(&mut self, name: &str, filename: &str, data: &[u8]) {
        self.buf.extend_from_slice(format!("--{}\r\nContent-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\nContent-Type: application/octet-stream\r\n\r\n", self.boundary, name, filename).as_bytes());
        self.buf.extend_from_slice(data);
        self.buf.extend_from_slice(b"\r\n");
    }
    fn finish(&mut self) {
        self.buf.extend_from_slice(format!("--{}--\r\n", self.boundary).as_bytes());
    }
}

fn rand_hex() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mixed = nanos.rotate_left(23) ^ (std::process::id() as u64).wrapping_mul(0x9e3779b97f4a7c15);
    format!("{mixed:016x}")
}

fn api_post_multipart(app: &AppHandle, route: &str, form: &mut MpForm) -> Result<Value, String> {
    form.finish();
    let mut req = api_client()
        .post(format!("{}{}", fb_base(), route))
        .header("Content-Type", format!("multipart/form-data; boundary={}", form.boundary));
    let tok = session(app);
    if !tok.is_empty() {
        req = req.header("Authorization", format!("Bearer {}", tok));
    }
    let resp = req
        .body(form.buf.clone())
        .send()
        .map_err(|e| format!("网络错误：{e}"))?;
    let body: Value = resp.json().map_err(|e| format!("响应解析失败：{e}"))?;
    unwrap_ok(body)
}

// ---- 日志收集（当前实例）----

/// 收集当前实例可附日志：crash-reports 最新一份 + logs/latest.log（各截 2MB）。
/// 返回 (文件名, 字节)。找不到目录时返回空 vec（由调用方按类型规则决定是否放行）。
fn collect_instance_logs(app: &AppHandle) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let settings = crate::settings::get(app);
    let folder = settings
        .get("activeFolder")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .or_else(|| settings.get("gameDir").and_then(|v| v.as_str()))
        .unwrap_or("");
    if folder.is_empty() {
        return out;
    }
    let root = Path::new(folder);
    // 崩溃报告：crash-reports 下 mtime 最新
    let crash_dir = root.join("crash-reports");
    if let Ok(entries) = fs::read_dir(&crash_dir) {
        let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) == Some("txt") {
                if let Ok(t) = e.metadata().and_then(|m| m.modified()) {
                    if best.as_ref().map(|(bt, _)| t > *bt).unwrap_or(true) {
                        best = Some((t, p));
                    }
                }
            }
        }
        if let Some((_, p)) = best {
            if let Some(data) = read_capped(&p, 2 << 20) {
                let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                out.push((name, data));
            }
        }
    }
    // latest.log
    let log = root.join("logs").join("latest.log");
    if let Some(data) = read_capped(&log, 2 << 20) {
        out.push(("latest.log".to_string(), data));
    }
    out
}

/// 读文件尾部 limit 字节（日志大头在尾部）。
fn read_capped(path: &Path, limit: u64) -> Option<Vec<u8>> {
    let meta = fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() == 0 {
        return None;
    }
    use std::io::{Seek, SeekFrom};
    let mut f = fs::File::open(path).ok()?;
    let start = meta.len().saturating_sub(limit);
    if start > 0 {
        f.seek(SeekFrom::Start(start)).ok()?;
    }
    let mut buf = Vec::new();
    f.take(limit).read_to_end(&mut buf).ok()?;
    Some(buf)
}

// ---- 登录：配对码 ----
//
// 分两步，和 accounts:msBegin/msCancel 同构：
//  1. feedback:login 立刻返回 { code, verifyUrl }，同时在后台线程轮询领取会话；
//  2. 领取到（或失败/取消）后向前端发 event:feedbackLoginDone。
// 配对页上写着"请核对启动器里显示的码与此一致"——所以码必须真的显示出来；
// 且用户中途想放弃时要有取消入口，不能干等 300 秒。

use std::sync::atomic::{AtomicBool, Ordering};

static FB_POLL_CANCEL: AtomicBool = AtomicBool::new(false);

/// 当前登录流程的配对码（前端切换视图后重进可恢复展示）。
static FB_PAIR_CODE: OnceLock<std::sync::Mutex<String>> = OnceLock::new();

fn pair_code_slot() -> &'static std::sync::Mutex<String> {
    FB_PAIR_CODE.get_or_init(|| std::sync::Mutex::new(String::new()))
}

fn pair_start(app: &AppHandle) -> Result<Value, String> {
    FB_POLL_CANCEL.store(false, Ordering::SeqCst);
    let data = api_post_json(app, "/api/auth/pair/start", &json!({}))?;
    let code = data.get("code").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let url = data.get("verifyUrl").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if code.is_empty() || url.is_empty() {
        return Err("服务端未返回配对码".into());
    }
    *pair_code_slot().lock().unwrap() = code.clone();
    // 打开系统浏览器到网页配对页（网页为登录正主）
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(&url, None::<&str>)
        .map_err(|e| format!("无法打开浏览器：{e}"))?;

    let app2 = app.clone();
    let code2 = code.clone();
    let interval = data.get("intervalSec").and_then(|v| v.as_i64()).unwrap_or(3).max(1) as u64;
    std::thread::spawn(move || {
        let payload = match pair_await(&app2, &code2, interval) {
            Ok(identity) => json!({ "ok": true, "identity": identity }),
            Err(e) if e == "已取消" => json!({ "ok": false, "cancelled": true }),
            Err(e) => json!({ "ok": false, "error": e }),
        };
        *pair_code_slot().lock().unwrap() = String::new();
        if let Some(win) = app2.get_webview_window("main") {
            let _ = tauri::Emitter::emit_to(&win, "main", "event:feedbackLoginDone", payload);
        }    });
    Ok(json!({ "code": code, "verifyUrl": url }))
}

/// 后台轮询领取会话；用户取消或超时即退出。成功时把会话落盘。
fn pair_await(app: &AppHandle, code: &str, interval: u64) -> Result<Value, String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(300);
    while std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_secs(interval));
        if FB_POLL_CANCEL.load(Ordering::SeqCst) {
            return Err("已取消".into());
        }
        let poll = api_post_json(app, "/api/auth/pair/poll", &json!({ "code": code }))?;
        match poll.get("status").and_then(|v| v.as_str()) {
            Some("approved") => {
                let tok = poll.get("session").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if tok.is_empty() {
                    return Err("领取会话失败".into());
                }
                let identity = poll.get("identity").cloned().unwrap_or(json!(null));
                let mut store = load_store(app);
                store["session"] = json!(tok);
                store["identity"] = identity.clone();
                save_store(app, &store);
                return Ok(identity);
            }
            Some("expired") => return Err("配对已过期，请重试".into()),
            _ => {}
        }
    }
    Err("等待登录超时（5 分钟），请重试".into())
}

/// feedback:loginCancel —— 中止后台轮询，前端立刻回到可操作状态。
fn pair_cancel(_app: &AppHandle) -> Value {
    FB_POLL_CANCEL.store(true, Ordering::SeqCst);
    *pair_code_slot().lock().unwrap() = String::new();
    json!({ "ok": true })
}

/// 反馈附件多选对话框：返回选中路径数组（取消返回空数组）。
fn pick_files(app: &AppHandle) -> Result<Value, String> {
    use tauri_plugin_dialog::DialogExt;
    let picked = app
        .dialog()
        .file()
        .add_filter("日志与报告", &["log", "txt", "gz"])
        .add_filter("图片", &["png", "jpg", "jpeg", "gif", "webp", "bmp"])
        .add_filter("压缩包", &["zip", "mrpack", "7z", "rar"])
        .add_filter("全部文件", &["*"])
        .blocking_pick_files();
    Ok(match picked {
        Some(paths) => json!(paths.iter().map(|p| p.to_string()).collect::<Vec<String>>()),
        None => json!([]),
    })
}

// ---- 公开板 ----

fn issues_list(app: &AppHandle, query: &Value) -> Result<Value, String> {
    // 页大小由前端决定并在这里透传：写死会与前端分页算式脱节（曾固定 50 而前端按 20 算页数，
    // 显示"第 1/5 页"实际每页 50 条，翻页直接跳过数据）。服务端上限 100。
    let size = query
        .get("size")
        .and_then(|v| v.as_i64())
        .filter(|n| *n > 0)
        .unwrap_or(20)
        .min(100);
    let mut params: Vec<(&str, String)> = vec![
        ("page", query.get("page").and_then(|v| v.as_i64()).unwrap_or(1).to_string()),
        ("size", size.to_string()),
    ];
    if let Some(k) = query.get("kind").and_then(|v| v.as_str()) {
        if !k.is_empty() {
            params.push(("kind", k.to_string()));
        }
    }
    if let Some(s) = query.get("status").and_then(|v| v.as_str()) {
        if !s.is_empty() {
            params.push(("status", s.to_string()));
        }
    }
    if let Some(q) = query.get("q").and_then(|v| v.as_str()) {
        if !q.is_empty() {
            params.push(("q", q.to_string()));
        }
    }
    if query.get("mine").and_then(|v| v.as_bool()).unwrap_or(false) {
        params.push(("mine", "1".into()));
    }
    api_get(app, "/api/issues/list", &params)
}

fn issue_get(app: &AppHandle, num: i64) -> Result<Value, String> {
    api_get(app, "/api/issues/get", &[("num", num.to_string())])
}

fn issue_comment(app: &AppHandle, args: &Value) -> Result<Value, String> {
    let num = args.get("num").and_then(|v| v.as_i64()).unwrap_or(0);
    let text = args.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let mut form = MpForm::new();
    form.field("num", &num.to_string());
    form.field("text", &text);
    // 评论附件：路径数组（原始字节直传）
    if let Some(paths) = args.get("files").and_then(|v| v.as_array()) {
        for p in paths.iter().take(5) {
            let Some(path) = p.as_str() else { continue };
            let Ok(data) = fs::read(path) else { continue };
            let name = Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "file".into());
            form.file("attachments", &name, &data);
        }
    }
    api_post_multipart(app, "/api/issues/comment", &mut form)
}

fn issue_retract(app: &AppHandle, num: i64, msg: &str) -> Result<Value, String> {
    api_post_json(app, "/api/issues/retract", &json!({ "num": num.to_string(), "msg": msg }))
}

fn issue_delete(app: &AppHandle, num: i64) -> Result<Value, String> {
    api_post_json(app, "/api/issues/delete", &json!({ "num": num.to_string() }))
}

fn issue_submit(app: &AppHandle, args: &Value) -> Result<Value, String> {
    let title = args.get("title").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    let body = args.get("body").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    let kind = args.get("kind").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if title.is_empty() {
        return Err("请填写标题".into());
    }
    if body.is_empty() {
        return Err("请填写详细描述".into());
    }
    let mut form = MpForm::new();
    form.field("title", &title);
    form.field("body", &body);
    form.field("kind", &kind);
    // 上下文：启动器/系统信息（服务端原样存档，详情页折叠展示）
    let sys = crate::sysinfo::system_info();
    let settings = crate::settings::get(app);
    let ctx = json!({
        "launcher": format!("WUHUICL {}", settings.get("version").and_then(|v| v.as_str()).unwrap_or("")),
        "os": format!("{} {}", sys.get("platform").and_then(|v| v.as_str()).unwrap_or(""), sys.get("architecture").and_then(|v| v.as_str()).unwrap_or("")),
        "memMB": sys.get("totalMemMB").cloned().unwrap_or(json!("")),
        "instance": settings.get("activeVersion").cloned().unwrap_or(json!("")),
    });
    form.field("context", &ctx.to_string());
    // 附件：用户显式选的文件 + 按类型自动收集的实例日志
    let mut names: Vec<String> = Vec::new();
    if let Some(paths) = args.get("files").and_then(|v| v.as_array()) {
        for p in paths.iter().take(10) {
            let Some(path) = p.as_str() else { continue };
            let Ok(data) = fs::read(path) else { continue };
            let name = Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "file".into());
            if names.len() < 10 {
                form.file("attachments", &name, &data);
                names.push(name);
            }
        }
    }
    if args.get("withLogs").and_then(|v| v.as_bool()).unwrap_or(true) {
        for (name, data) in collect_instance_logs(app) {
            if names.len() >= 10 {
                break;
            }
            if names.contains(&name) {
                continue;
            }
            form.file("attachments", &name, &data);
            names.push(name);
        }
    }
    api_post_multipart(app, "/api/issues/new", &mut form)
}

// ---- 匿名私信（本地凭证文件，单向删除）----

fn tickets_local(app: &AppHandle) -> Vec<Value> {
    load_store(app)
        .get("tickets")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
}

fn tickets_poll(app: &AppHandle) -> Result<Value, String> {
    let list = tickets_local(app);
    let ids: Vec<String> = list.iter().filter_map(|t| t.get("id").and_then(|v| v.as_str()).map(String::from)).collect();
    if ids.is_empty() {
        return Ok(json!({ "tickets": [], "removed": [] }));
    }
    let data = api_post_json(app, "/api/tickets/poll", &json!({ "ids": ids }))?;
    // 服务端已硬删 → 本地同步移除（单向删除）
    let removed: Vec<String> = data
        .get("removed")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|x| x.as_str().map(String::from)).collect())
        .unwrap_or_default();
    if !removed.is_empty() {
        let mut store = load_store(app);
        if let Some(arr) = store.get_mut("tickets").and_then(|v| v.as_array_mut()) {
            arr.retain(|t| t.get("id").and_then(|v| v.as_str()).map(|id| !removed.iter().any(|r| r == id)).unwrap_or(false));
        }
        save_store(app, &store);
    }
    Ok(data)
}

fn ticket_submit(app: &AppHandle, args: &Value) -> Result<Value, String> {
    let title = args.get("title").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    let desc = args.get("description").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    let kind = args.get("kind").and_then(|v| v.as_str()).unwrap_or("other").to_string();
    if desc.is_empty() {
        return Err("请写点内容".into());
    }
    let mut form = MpForm::new();
    form.field("title", &title);
    form.field("description", &desc);
    form.field("kind", &kind);
    form.field("client", "launcher");
    let sys = crate::sysinfo::system_info();
    form.field("device", &format!("{} {}", sys.get("platform").and_then(|v| v.as_str()).unwrap_or(""), sys.get("architecture").and_then(|v| v.as_str()).unwrap_or("")));
    if args.get("withLogs").and_then(|v| v.as_bool()).unwrap_or(false) {
        for (name, data) in collect_instance_logs(app).into_iter().take(10) {
            form.file("attachments", &name, &data);
        }
    }
    let data = api_post_multipart(app, "/api/tickets/new", &mut form)?;
    // 存本地凭证（单号+密钥仅此一次明文回显）
    let mut store = load_store(app);
    if let Some(arr) = store.get_mut("tickets").and_then(|v| v.as_array_mut()) {
        arr.insert(0, json!({
            "id": data.get("id").cloned().unwrap_or(json!("")),
            "secret": data.get("secret").cloned().unwrap_or(json!("")),
            "title": title,
            "kind": kind,
            "timeMs": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0),
            "lastTimeMs": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0),
            "replyCount": 0,
            "hasUnread": false,
        }));
        arr.truncate(50);
    } else {
        store["tickets"] = json!([]);
    }
    save_store(app, &store);
    Ok(data)
}

fn ticket_secret(app: &AppHandle, id: &str) -> String {
    tickets_local(app)
        .into_iter()
        .find(|t| t.get("id").and_then(|v| v.as_str()) == Some(id))
        .and_then(|t| t.get("secret").and_then(|v| v.as_str()).map(String::from))
        .unwrap_or_default()
}

fn ticket_detail(app: &AppHandle, id: &str) -> Result<Value, String> {
    let secret = ticket_secret(app, id);
    if secret.is_empty() {
        return Err("这台设备没有该私信的凭证".into());
    }
    api_get(app, "/api/tickets/get", &[("id", id.to_string()), ("secret", secret)])
}

fn ticket_reply(app: &AppHandle, args: &Value) -> Result<Value, String> {
    let id = args.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let text = args.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    let secret = ticket_secret(app, &id);
    if secret.is_empty() {
        return Err("这台设备没有该私信的凭证".into());
    }
    let mut form = MpForm::new();
    form.field("id", &id);
    form.field("text", &text);
    form.field("secret", &secret);
    if args.get("withLogs").and_then(|v| v.as_bool()).unwrap_or(false) {
        for (name, data) in collect_instance_logs(app).into_iter().take(10) {
            form.file("attachments", &name, &data);
        }
    }
    api_post_multipart(app, "/api/tickets/reply", &mut form)
}

fn ticket_viewed(app: &AppHandle, id: &str) -> Result<Value, String> {
    let secret = ticket_secret(app, id);
    api_post_json(app, "/api/tickets/viewed", &json!({ "id": id, "secret": secret }))
}

fn ticket_delete(app: &AppHandle, id: &str) -> Result<Value, String> {
    let secret = ticket_secret(app, id);
    let r = api_post_json(app, "/api/tickets/delete", &json!({ "id": id, "secret": secret }));
    // 本地移除条目（服务端仅软删保留——单向删除）
    let mut store = load_store(app);
    if let Some(arr) = store.get_mut("tickets").and_then(|v| v.as_array_mut()) {
        arr.retain(|t| t.get("id").and_then(|v| v.as_str()) != Some(id));
    }
    save_store(app, &store);
    r
}

fn ticket_retract(app: &AppHandle, id: &str, msg: &str) -> Result<Value, String> {
    let secret = ticket_secret(app, id);
    api_post_json(app, "/api/tickets/retract", &json!({ "id": id, "msg": msg, "secret": secret }))
}

// ---- 元信息 / 会话 ----

fn meta(app: &AppHandle) -> Result<Value, String> {
    let kinds = api_get(app, "/api/issues/meta", &[])?;
    let mut store = load_store(app);
    let mut out = kinds;
    let logged = !session(app).is_empty();
    out["loggedIn"] = json!(logged);
    out["identity"] = if logged {
        store.get("identity").cloned().unwrap_or(json!(null))
    } else {
        json!(null)
    };
    // 会话可能已失效：/api/auth/me 校验并刷新展示名
    if logged {
        match api_get(app, "/api/auth/me", &[]) {
            Ok(me) => {
                store["identity"] = me.clone();
                save_store(app, &store);
                out["identity"] = me;
            }
            Err(_) => {
                store["session"] = json!("");
                store["identity"] = json!(null);
                save_store(app, &store);
                out["loggedIn"] = json!(false);
                out["identity"] = json!(null);
            }
        }
    }
    out["tickets"] = json!(tickets_local(app).len());
    Ok(out)
}

fn logout(app: &AppHandle) -> Value {
    let mut store = load_store(app);
    store["session"] = json!("");
    store["identity"] = json!(null);
    save_store(app, &store);
    json!({ "ok": true })
}

/// 反馈附件下载：返回本地临时文件路径（前端经 kamucl-asset 协议展示）。
fn attachment_path(app: &AppHandle, kind: &str, owner: &str, fid: &str) -> Result<Value, String> {
    let (route, mut params) = match kind {
        "issue" => ("/api/issues/attachment".to_string(), vec![("num", owner.to_string()), ("fid", fid.to_string())]),
        "ticket" => {
            let secret = ticket_secret(app, owner);
            ("/api/tickets/attachment".to_string(), vec![("id", owner.to_string()), ("fid", fid.to_string()), ("secret", secret)])
        }
        _ => return Err("未知的附件来源".into()),
    };
    params.retain(|(_, v)| !v.is_empty());
    let url = format!(
        "{}{}?{}",
        fb_base(),
        route,
        params.iter().map(|(k, v)| format!("{}={}", k, urlencoding(v))).collect::<Vec<_>>().join("&")
    );
    let mut req = api_client().get(&url);
    let tok = session(app);
    if !tok.is_empty() {
        req = req.header("Authorization", format!("Bearer {}", tok));
    }
    let resp = req.send().map_err(|e| format!("网络错误：{e}"))?;
    if !resp.status().is_success() {
        return Err("附件下载失败".into());
    }
    let data = resp.bytes().map_err(|e| format!("读取失败：{e}"))?;
    // 缓存目录必须落在 launcher-managed/ 下：kamucl-asset 协议只授权该根（图片预览走它）。
    let dir = crate::settings::user_data_dir(app)
        .join("launcher-managed")
        .join("feedback-cache");
    let _ = fs::create_dir_all(&dir);
    let key = format!("{kind}:{owner}:{fid}");
    let path = dir.join(format!("att-{}.bin", fnv_hex(&key)));
    fs::write(&path, &data).map_err(|e| format!("保存失败：{e}"))?;
    Ok(json!({ "path": path.to_string_lossy(), "size": data.len() }))
}

/// 附件缓存文件名只要稳定唯一即可（本机临时文件，非安全边界）：双 64 位 FNV 混拼 32 hex。
fn fnv_hex(s: &str) -> String {
    let mut h1: u64 = 0xcbf29ce484222325;
    let mut h2: u64 = 0x9e3779b97f4a7c15;
    for b in s.as_bytes() {
        h1 = (h1 ^ *b as u64).wrapping_mul(0x100000001b3);
        h2 = (h2 ^ *b as u64).wrapping_mul(0x100000001b3).rotate_left(7);
    }
    format!("{h1:016x}{h2:016x}")
}

pub fn invoke(app: &AppHandle, channel: &str, args: &[Value]) -> Option<Result<Value, String>> {
    let a0 = args.first().cloned().unwrap_or(Value::Null);
    match channel {
        "feedback:meta" => Some(meta(app)),
        "feedback:login" => Some(pair_start(app)),
        "feedback:loginCancel" => Some(Ok(pair_cancel(app))),
        "feedback:logout" => Some(Ok(logout(app))),
        "feedback:issues:list" => Some(issues_list(app, &a0)),
        "feedback:issue:get" => a0.as_i64().map(|n| issue_get(app, n)),
        "feedback:issue:submit" => Some(issue_submit(app, &a0)),
        "feedback:issue:comment" => Some(issue_comment(app, &a0)),
        "feedback:issue:retract" => Some(
            args.first()
                .and_then(|v| v.get("num").and_then(|n| n.as_i64()))
                .zip(args.first().and_then(|v| v.get("msg").and_then(|m| m.as_str())))
                .map(|(n, m)| issue_retract(app, n, m))
                .unwrap_or(Err("参数缺失".into())),
        ),
        "feedback:issue:delete" => Some(
            a0.get("num")
                .and_then(|v| v.as_i64())
                .map(|n| issue_delete(app, n))
                .unwrap_or_else(|| Err("缺少 num".into())),
        ),
        "feedback:tickets:list" => Some(Ok(json!(tickets_local(app)))),
        "feedback:tickets:poll" => Some(tickets_poll(app)),
        "feedback:ticket:submit" => Some(ticket_submit(app, &a0)),
        "feedback:ticket:detail" => a0.as_str().map(|id| ticket_detail(app, id)),
        "feedback:ticket:reply" => Some(ticket_reply(app, &a0)),
        "feedback:ticket:viewed" => a0.as_str().map(|id| ticket_viewed(app, id)),
        "feedback:ticket:delete" => a0.as_str().map(|id| ticket_delete(app, id)),
        "feedback:ticket:retract" => Some(
            args.first()
                .and_then(|v| v.get("id").and_then(|i| i.as_str()).zip(v.get("msg").and_then(|m| m.as_str())))
                .map(|(i, m)| ticket_retract(app, i, m))
                .unwrap_or(Err("参数缺失".into())),
        ),
        "feedback:attachment" => Some(
            args.first()
                .and_then(|v| {
                    Some((
                        v.get("kind").and_then(|x| x.as_str())?,
                        v.get("owner").and_then(|x| x.as_str())?,
                        v.get("fid").and_then(|x| x.as_str())?,
                    ))
                })
                .map(|(k, o, f)| attachment_path(app, k, o, f))
                .unwrap_or(Err("参数缺失".into())),
        ),
        "feedback:logs:preview" => Some(Ok(json!(collect_instance_logs(app)
            .into_iter()
            .map(|(n, d)| json!({ "name": n, "size": d.len() }))
            .collect::<Vec<Value>>()))),
        // 反馈附件选择：多选 + 通用过滤（日志/崩溃报告/图片/压缩包），不动 app:selectFile 的整合包语义。
        "feedback:pickFiles" => Some(pick_files(app)),
        _ => None,
    }
}
