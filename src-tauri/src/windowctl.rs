//! 窗口控制与生命周期（对应原版 main/index.ts 的 window:* 通道 + 可见性广播）

use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::SeqCst};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, Window, WindowEvent};

/// ipc_send 路由的窗口动作（from_main = 调用方是否主窗口，对应原版 event.sender === win.webContents）
pub fn command(
    app: &AppHandle,
    from_main: bool,
    channel: &str,
    args: &[serde_json::Value],
) -> Result<(), String> {
    if !from_main && channel.starts_with("window:") {
        return Ok(()); // 原版：非主窗口发来的一律忽略
    }
    let Some(win) = app.get_webview_window("main") else {
        return Ok(());
    };
    match channel {
        "window:minimize" => {
            let _ = win.minimize();
        }
        "window:maximize" => {
            // 原版 toggleMaximize：最大化↔还原
            if win.is_maximized().unwrap_or(false) {
                let _ = win.unmaximize();
            } else {
                let _ = win.maximize();
            }
        }
        "window:close" => {
            let _ = win.close();
        }
        // 原版 index.ts:247：仅在编辑器不脏、不忙、无待写偏好时才真正退出
        "window:skinEditorQuit" => {
            if !palette_blocked() {
                app.exit(0);
            }
        }
        // 原版 index.ts:268：桌宠确认中也不退出
        "window:mascotQuit" => {
            if !MASCOT_PENDING.load(SeqCst) && !palette_blocked() {
                app.exit(0);
            }
        }
        "window:skinEditorDirty" => {
            SKIN_DIRTY.store(args.first().and_then(|v| v.as_bool()) == Some(true), SeqCst);
        }
        "window:mascotPending" => {
            MASCOT_PENDING.store(args.first().and_then(|v| v.as_bool()) == Some(true), SeqCst);
        }
        "window:skinEditorBusy" => {
            if let Some((owner, pending)) = owner_flag(args) {
                let mut owners = BUSY_OWNERS.lock().unwrap();
                if pending {
                    if !owners.contains(&owner) {
                        owners.push(owner.clone());
                    }
                } else {
                    owners.retain(|item| *item != owner);
                }
            }
        }
        "window:skinEditorPrefsPending" => {
            if let Some((owner, pending)) = owner_flag(args) {
                {
                    let mut owners = PREFS_OWNERS.lock().unwrap();
                    if pending {
                        if !owners.contains(&owner) {
                            owners.push(owner.clone());
                        }
                    } else {
                        owners.retain(|item| *item != owner);
                    }
                }
                // 原版 index.ts:260-265：待写偏好清空时，补完被拦截的关窗/退出
                if !prefs_pending() && !SKIN_DIRTY.load(SeqCst) && !busy_pending() {
                    let close = DEFERRED_CLOSE.swap(false, SeqCst);
                    let quit = DEFERRED_QUIT.swap(false, SeqCst);
                    if quit {
                        app.exit(0);
                    } else if close {
                        let _ = win.close();
                    }
                }
            }
        }
        "boot:stage" => {
            if let Some(s) = args.first().and_then(|v| v.as_str()) {
                crate::gate::stage(app, s);
            }
        }
        _ => return Err(format!("未实现的 send 通道：{}", channel)),
    }
    Ok(())
}

// ---------------- 皮肤编辑器 / 桌宠关窗确认（对应原版 main/index.ts:89-95、143-144、247-268、327） ----------------

static SKIN_DIRTY: AtomicBool = AtomicBool::new(false);
static MASCOT_PENDING: AtomicBool = AtomicBool::new(false);
static DEFERRED_CLOSE: AtomicBool = AtomicBool::new(false);
static DEFERRED_QUIT: AtomicBool = AtomicBool::new(false);
static BUSY_OWNERS: Mutex<Vec<String>> = Mutex::new(Vec::new());
static PREFS_OWNERS: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn busy_pending() -> bool {
    !BUSY_OWNERS.lock().unwrap().is_empty()
}

fn prefs_pending() -> bool {
    !PREFS_OWNERS.lock().unwrap().is_empty()
}

/// 原版口径：脏态 / 忙 / 待写偏好任一成立都要先问渲染层
pub fn palette_blocked() -> bool {
    SKIN_DIRTY.load(SeqCst) || prefs_pending() || busy_pending()
}

/// 原版 index.ts:143：仅「只卡在写偏好」时，写完后补一次关窗
pub fn note_deferred_close() {
    DEFERRED_CLOSE.store(
        prefs_pending() && !SKIN_DIRTY.load(SeqCst) && !busy_pending(),
        SeqCst,
    );
}

/// 原版 index.ts:327：退出流程卡在写偏好时，写完后补一次退出
pub fn note_deferred_quit() {
    DEFERRED_QUIT.store(
        prefs_pending() && !SKIN_DIRTY.load(SeqCst) && !busy_pending(),
        SeqCst,
    );
}

pub fn emit_palette_close(app: &AppHandle, quit: bool) {
    let payload = if quit {
        json!({ "quit": true })
    } else {
        json!({})
    };
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.emit_to("main", "window:skinEditorClose", payload);
    }
}

pub fn mascot_pending() -> bool {
    MASCOT_PENDING.load(SeqCst)
}

pub fn emit_mascot_close(app: &AppHandle, quit: bool) {
    let payload = if quit {
        json!({ "quit": true })
    } else {
        json!({})
    };
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.emit_to("main", "window:mascotClose", payload);
    }
}

/// 用户开始拖动标题栏 → 通知渲染层收起顶部下拉（原版 will-move / caption 命中）
pub fn emit_caption_pointerdown(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.emit_to("main", "window:caption-pointerdown", Value::Null);
    }
}

/// 主窗口重建时清空脏态（对应原版 createWindow 内的复位）
pub fn reset_palette_state() {
    SKIN_DIRTY.store(false, SeqCst);
    MASCOT_PENDING.store(false, SeqCst);
    DEFERRED_CLOSE.store(false, SeqCst);
    DEFERRED_QUIT.store(false, SeqCst);
    BUSY_OWNERS.lock().unwrap().clear();
    PREFS_OWNERS.lock().unwrap().clear();
}

/// 原版对 ownerId 的校验：^[a-zA-Z0-9-]{1,64}$，pending 必须是布尔
fn owner_flag(args: &[serde_json::Value]) -> Option<(String, bool)> {
    let value = args.first()?;
    let owner = value.get("ownerId").and_then(|v| v.as_str())?;
    let pending = value.get("pending").and_then(|v| v.as_bool())?;
    if owner.is_empty()
        || owner.len() > 64
        || !owner
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return None;
    }
    Some((owner.to_string(), pending))
}

/// 窗口事件 → 广播 window:visibility（对应原版 minimize/hide/show/restore 上报）
pub fn on_window_event(window: &Window, event: &WindowEvent) {
    if window.label() != "main" {
        return;
    }
    match event {
        WindowEvent::Focused(focused) => {
            let visible = *focused || window.is_visible().unwrap_or(false);
            let _ = window.emit_to("main", "window:visibility", visible);
        }
        WindowEvent::CloseRequested { api, .. } => {
            // 原版 index.ts:143-144：编辑器脏态/待写偏好/写入中 → 拦下关窗，改让渲染层确认
            if palette_blocked() {
                api.prevent_close();
                note_deferred_close();
                emit_palette_close(window.app_handle(), false);
                return;
            }
            // 原版：桌宠正在确认 → 同样拦下
            if MASCOT_PENDING.load(SeqCst) {
                api.prevent_close();
                emit_mascot_close(window.app_handle(), false);
            }
        }
        WindowEvent::Moved(_) => {
            emit_caption_pointerdown(window.app_handle());
            schedule_apply(window.app_handle());
        }
        WindowEvent::Resized(_) => {
            schedule_apply(window.app_handle());
        }
        _ => {}
    }
}

/// 启动后当前可见性查询（window:visibility handle）
pub fn visibility(app: &AppHandle) -> serde_json::Value {
    let Some(win) = app.get_webview_window("main") else {
        return json!(false);
    };
    json!(win.is_visible().unwrap_or(false) && !win.is_minimized().unwrap_or(false))
}

// ---------------- UI 窗口自适应（对应 shared/uiSizing.ts + main/uiWindowSizing.ts） ----------------

/// 逻辑像素矩形：原版全程用 DIP，这里不混入 scale_factor
#[derive(Clone, Copy, PartialEq)]
pub struct RectL {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

pub fn fit_ui_window_bounds(b: RectL, work: RectL) -> RectL {
    let w = b.w.min((work.w * 0.92).floor().max(1.0));
    let h = b.h.min((work.h * 0.92).floor().max(1.0));
    RectL {
        w,
        h,
        x: (b.x.max(work.x)).min(work.x + work.w - w).round(),
        y: (b.y.max(work.y)).min(work.y + work.h - h).round(),
    }
}

pub fn ui_window_minimum(work: RectL) -> (f64, f64) {
    (
        (work.w * 0.92).floor().max(1.0).min(960.0),
        (work.h * 0.92).floor().max(1.0).min(620.0),
    )
}

static AUTO_FIT: AtomicBool = AtomicBool::new(false);
static PENDING: AtomicU64 = AtomicU64::new(0);

fn work_area_of(win: &tauri::WebviewWindow) -> Option<RectL> {
    let monitor = win.current_monitor().ok()??;
    let area = monitor.work_area();
    let s = monitor.scale_factor().max(0.01);
    Some(RectL {
        x: area.position.x as f64 / s,
        y: area.position.y as f64 / s,
        w: area.size.width as f64 / s,
        h: area.size.height as f64 / s,
    })
}

fn bounds_of(win: &tauri::WebviewWindow) -> Option<RectL> {
    let s = win.scale_factor().ok()?;
    let p = win.outer_position().ok()?;
    let size = win.outer_size().ok()?;
    Some(RectL {
        x: p.x as f64 / s,
        y: p.y as f64 / s,
        w: size.width as f64 / s,
        h: size.height as f64 / s,
    })
}

fn apply(app: &AppHandle, constrain: bool) {
    if !AUTO_FIT.load(SeqCst) {
        return;
    }
    let Some(win) = app.get_webview_window("main") else {
        return;
    };
    let Some(area) = work_area_of(&win) else {
        return;
    };
    let (mw, mh) = ui_window_minimum(area);
    let _ = win.set_min_size(Some(tauri::LogicalSize::new(mw, mh)));
    if win.is_maximized().unwrap_or(false) || win.is_fullscreen().unwrap_or(false) {
        return;
    }
    let Some(b) = bounds_of(&win) else {
        return;
    };
    let outside = b.x + b.w <= area.x
        || b.y + b.h <= area.y
        || b.x >= area.x + area.w
        || b.y >= area.y + area.h;
    if !(constrain || outside || b.w > area.w || b.h > area.h) {
        return;
    }
    let fitted = fit_ui_window_bounds(b, area);
    if fitted != b {
        let _ = win.set_size(tauri::LogicalSize::new(fitted.w, fitted.h));
        let _ = win.set_position(tauri::LogicalPosition::new(fitted.x, fitted.y));
    }
}

/// 对应 attachUiWindowSizing 的 configure(enabled)
pub fn configure_auto_fit(app: &AppHandle, enabled: bool) {
    if AUTO_FIT.swap(enabled, SeqCst) == enabled {
        return;
    }
    if !enabled {
        if let Some(win) = app.get_webview_window("main") {
            let _ = win.set_min_size(Some(tauri::LogicalSize::new(960.0, 620.0)));
        }
    }
    apply(app, enabled);
}

/// 60ms 去抖（对应原版 schedule）
pub fn schedule_apply(app: &AppHandle) {
    if !AUTO_FIT.load(SeqCst) {
        return;
    }
    let token = PENDING.fetch_add(1, SeqCst) + 1;
    let handle = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(60));
        if PENDING.load(SeqCst) == token {
            apply(&handle, false);
        }
    });
}

/// 对应原版 nativeAppearance.ts applyNativeAppearance：DWM 帧延入客户区 + 主题染色 + 圆角 + Acrylic。
/// 不做帧扩展时 DWM 不会把桌面模糊合成进客户区，玻璃层只剩窗口底色，比原版暗约 80 灰度。
pub fn apply_native_appearance(win: &tauri::WebviewWindow, settings: &serde_json::Value) {
    let light = is_light_theme(settings);
    // 对应原版 nativeTheme.themeSource = light ? 'light' : 'dark'：
    // 不锁死的话 WebView2 跟随系统深色，prefers-color-scheme/滚动条/输入框全部走暗色
    let _ = win.set_theme(Some(if light {
        tauri::Theme::Light
    } else {
        tauri::Theme::Dark
    }));
    let Ok(hwnd) = win.hwnd() else { return };
    let h = hwnd.0 as *mut std::ffi::c_void;
    let dark: u32 = if light { 0 } else { 1 };
    let round: u32 = 2;
    let acrylic: u32 = 3;
    #[repr(C)]
    struct Margins {
        l: i32,
        t: i32,
        r: i32,
        b: i32,
    }
    extern "system" {
        fn DwmExtendFrameIntoClientArea(
            hwnd: *mut std::ffi::c_void,
            margins: *const Margins,
        ) -> i32;
        fn DwmSetWindowAttribute(
            hwnd: *mut std::ffi::c_void,
            attr: u32,
            value: *const u32,
            size: u32,
        ) -> i32;
    }
    let margins = Margins {
        l: -1,
        t: -1,
        r: -1,
        b: -1,
    };
    unsafe {
        DwmExtendFrameIntoClientArea(h, &margins);
        DwmSetWindowAttribute(h, 38, &acrylic, 4);
        DwmSetWindowAttribute(h, 20, &dark, 4);
        DwmSetWindowAttribute(h, 33, &round, 4);
    }
}

/// 对应原版 light 判定：blue-white / white-pink，或 custom 背景亮度 > 128
fn is_light_theme(s: &serde_json::Value) -> bool {
    let theme = s["theme"].as_str().unwrap_or("");
    if theme == "blue-white" || theme == "white-pink" {
        return true;
    }
    if theme == "custom" {
        if let Some(bg) = s["custom"]["colors"]["bg"].as_str() {
            let hex = match bg.strip_prefix('#') {
                Some(v) => v,
                None => return false,
            };
            if hex.len() != 6 || !hex.bytes().all(|c| c.is_ascii_hexdigit()) {
                return false;
            }
            let ch = |i: usize| u32::from_str_radix(&hex[i..i + 2], 16).unwrap_or(0) as f64;
            return ch(0) * 0.299 + ch(2) * 0.587 + ch(4) * 0.114 > 128.0;
        }
    }
    false
}
