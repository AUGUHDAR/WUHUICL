//! 内联浏览器：在启动器内开一个受控子窗口显示网页（MC 百科 / CurseForge 项目页）。
//!
//! 为什么不塞 iframe：主窗口 CSP 是 `default-src 'self'`，外部站点会被直接拦掉；
//! 而放宽 CSP 等于给所有页面开洞。用独立 Webview 窗口（`WebviewUrl::External`）
//! 既不受主窗口 CSP 约束，也和主页面地址空间天然隔离。
//!
//! 安全口径：
//! - 只允许 https；
//! - 只允许白名单域名及其子域（MC 百科 / CurseForge / Modrinth / GitHub）；
//! - 窗口带标题栏与「回主界面/在系统浏览器打开」按钮，玩家随时能脱身；
//! - 单例：重复调用只把已有窗口带到前台并换页，不堆一串窗口。

use serde_json::{json, Value};
use std::sync::Mutex;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

/// 允许内联打开的站点（子域一并放行）
const ALLOWED_HOSTS: &[&str] = &[
    "mcmod.cn",
    "search.mcmod.cn",
    "www.mcmod.cn",
    "curseforge.com",
    "www.curseforge.com",
    "modrinth.com",
    "www.modrinth.com",
    "github.com",
    "gitee.com",
];

const WINDOW_LABEL: &str = "inline-web";

static LAST_URL: Mutex<Option<String>> = Mutex::new(None);

fn host_allowed(url: &tauri::Url) -> bool {
    if url.scheme() != "https" {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    let h = host.to_ascii_lowercase();
    ALLOWED_HOSTS
        .iter()
        .any(|allowed| h == *allowed || h.ends_with(&format!(".{allowed}")))
}

/// 在内联浏览器里打开 url。`title` 只用于窗口标题。
pub fn open(app: &tauri::AppHandle, url: &str, title: &str) -> Result<Value, String> {
    let parsed: tauri::Url = url.parse().map_err(|_| "网址格式无效".to_string())?;
    if !host_allowed(&parsed) {
        return Err("这个站点不支持在启动器内浏览，请用系统浏览器打开".into());
    }
    *LAST_URL.lock().unwrap() = Some(url.to_string());

    // 已有窗口：换页并聚焦，不新开
    if let Some(win) = app.get_webview_window(WINDOW_LABEL) {
        let _ = win.navigate(parsed.clone());
        let _ = win.set_title(title);
        let _ = win.show();
        let _ = win.set_focus();
        return Ok(json!({ "ok": true, "reused": true }));
    }

    let win = WebviewWindowBuilder::new(app, WINDOW_LABEL, WebviewUrl::External(parsed))
        .title(format!("{} · WUHUICL", title))
        .inner_size(1100.0, 800.0)
        .min_inner_size(720.0, 520.0)
        .center()
        .resizable(true)
        // 原生装饰：网页里的关窗/拖拽都交给系统，避免和自绘标题栏打架
        .decorations(true)
        .build()
        .map_err(|e| format!("打开内联浏览器失败：{e}"))?;
    let _ = win.set_focus();
    Ok(json!({ "ok": true, "reused": false }))
}

pub fn close(app: &tauri::AppHandle) -> Value {
    if let Some(win) = app.get_webview_window(WINDOW_LABEL) {
        let _ = win.close();
    }
    json!({ "ok": true })
}

pub fn invoke(app: &tauri::AppHandle, channel: &str, args: &[Value]) -> Option<Result<Value, String>> {
    let s = |i: usize| args.get(i).and_then(|v| v.as_str()).unwrap_or("").to_string();
    match channel {
        "inlineWeb:open" => Some(open(app, &s(0), &s(1))),
        "inlineWeb:close" => Some(Ok(close(app))),
        "inlineWeb:lastUrl" => Some(Ok(json!(LAST_URL.lock().unwrap().clone()))),
        _ => None,
    }
}
