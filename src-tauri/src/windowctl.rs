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
            #[cfg(desktop)]
            let _ = win.minimize();
        }
        "window:maximize" => {
            // 原版 toggleMaximize：最大化↔还原
            #[cfg(desktop)]
            if win.is_maximized().unwrap_or(false) {
                let _ = win.unmaximize();
            } else {
                let _ = win.maximize();
            }
            // tao 在 set_maximized 里会经 apply_diff 把 WS_CAPTION/WS_SYSMENU 写回
            ensure_frameless(&win);
            ensure_frameless_later(app);
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
            // 还原/激活兜底：最小化整理后若 Resized 时序没踩中，靠激活事件补 Resume
            if *focused {
                maybe_resume_from_trim(window);
            }
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
            schedule_frameless(window.app_handle());
        }
        WindowEvent::Resized(_) => {
            schedule_apply(window.app_handle());
            schedule_frameless(window.app_handle());
            self_mem_trim_on_resize(window);
        }
        _ => {}
    }
}

/// 最小化自整理（memTrimOnMinimize，默认开）：
/// ① 前端瘦身广播（idleTrim 注册表释放可重建缓存）；
/// ② 无后台任务时：挂起 WebView2 渲染进程（TrySuspend）+ 释放三类扫描目录 + 换出自身工作集。
/// 有任务在跑时跳过 ②：进度事件会排队在挂起的 WebView2 里，恢复瞬间才集中回放。
/// 恢复窗口时页面按需重建/换回，代价是一点恢复延迟。
fn self_mem_trim_on_resize(window: &Window) {
    if !window.is_minimized().unwrap_or(false) {
        maybe_resume_from_trim(window);
        return;
    }
    if TRIMMED_WHILE_MINIMIZED.swap(true, SeqCst) {
        return; // 已处理过这次最小化，不重复整理
    }
    let app = window.app_handle();
    let enabled = crate::settings::get_arc(app)
        .get("memTrimOnMinimize")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    if !enabled {
        return;
    }
    let _ = window.emit_to("main", "kamucl:mem-trim", Value::Null);
    if crate::tasks::has_active_tasks() {
        return;
    }
    crate::screenshots::release_catalog();
    crate::recordings::release_catalog();
    // 投影元数据改为持久缓存：压缩前先落盘，再释放内存与扫描目录
    crate::projections::release_catalog(app);
    set_webview_suspended(app, true);
    crate::misc::empty_self_working_set();
    spawn_restore_watcher(window);
}

/// 还原看门狗：挂起后轮询 IsIconic（400ms），翻转即经主线程恢复 WebView2。
/// 不能只靠窗口事件：restore 时尺寸未变会被 tao 去重不发 Resized，
/// "还原但未激活"（如第三方 SW_SHOWNOACTIVATE）也不会有 Focused(true)。
#[cfg(windows)]
fn spawn_restore_watcher(window: &Window) {
    use tauri::Manager as _;
    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    let app = window.app_handle().clone();
    let hwnd_raw = hwnd.0 as isize;
    std::thread::spawn(move || {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::IsIconic;
        let hwnd = HWND(hwnd_raw as *mut core::ffi::c_void);
        loop {
            std::thread::sleep(std::time::Duration::from_millis(400));
            if !unsafe { IsIconic(hwnd) }.as_bool() {
                break;
            }
        }
        // COM 恢复必须回主线程（WebView2 控制器是 UI 线程对象）
        let app2 = app.clone();
        let _ = app.run_on_main_thread(move || {
            if let Some(win) = app2.get_webview_window("main") {
                if !win.is_minimized().unwrap_or(false) && TRIMMED_WHILE_MINIMIZED.swap(false, SeqCst)
                {
                    set_webview_suspended(&app2, false);
                }
            }
        });
    });
}

#[cfg(not(windows))]
fn spawn_restore_watcher(_window: &Window) {}

/// 还原兜底：Resized 与 Focused 任一到达且窗口已非最小化时，恢复 WebView2。
/// （restore 时 tao 的 Resized 可能在 is_minimized 翻转前到达，Focused 保险）
fn maybe_resume_from_trim(window: &Window) {
    if window.is_minimized().unwrap_or(false) {
        return;
    }
    if TRIMMED_WHILE_MINIMIZED.swap(false, SeqCst) {
        set_webview_suspended(window.app_handle(), false);
    }
}

/// 最小化自整理去重：最小化/还原途中 Resized 会连发，只在状态翻转时各处理一次
static TRIMMED_WHILE_MINIMIZED: AtomicBool = AtomicBool::new(false);

/// 主窗口的 WebView2 控制器（setup 阶段捕获）。
/// 不在事件回调里临时 with_webview：那条路径走 user-message 泵，
/// 最小化/还原的嵌套消息循环期间不可靠。
/// COM 指针仅主线程解引用（事件回调与 setup 都在主线程），包装只为满足 static 约束。
#[cfg(windows)]
struct SendController(webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Controller);
#[cfg(windows)]
unsafe impl Send for SendController {}
#[cfg(windows)]
unsafe impl Sync for SendController {}

#[cfg(windows)]
static MAIN_CONTROLLER: std::sync::OnceLock<SendController> = std::sync::OnceLock::new();

/// setup 阶段捕获主窗口 WebView2 控制器（仅 Windows）。
#[cfg(windows)]
pub fn capture_main_controller(win: &tauri::WebviewWindow) {
    let _ = win.with_webview(|platform| {
        let controller = platform.controller();
        let _ = MAIN_CONTROLLER.set(SendController(controller));
    });
}

/// WebView2 渲染进程挂起/恢复（ICoreWebView2_3::TrySuspend / Resume，官方为
/// 「WebView 不可见时省内存」提供的机制）。宿主须先自行把控制器 IsVisible 置 false
/// （wry 不会随窗口最小化代管），否则 TrySuspend 以「webview 可见」为由直接失败；
/// 挂起失败（如正弹对话框）静默忽略，不影响功能。恢复时先 Resume 再显示。
#[cfg(windows)]
fn set_webview_suspended(_app: &AppHandle, suspend: bool) {
    let Some(controller) = MAIN_CONTROLLER.get() else {
        return;
    };
    unsafe {
        use webview2_com::Microsoft::Web::WebView2::Win32::{
            ICoreWebView2TrySuspendCompletedHandler, ICoreWebView2_3,
        };
        use windows_core::Interface as _;
        // ICoreWebView2_3 挂在 CoreWebView2 上，不是 controller 上：先取 webview 再 cast
        let Ok(webview) = controller.0.CoreWebView2() else {
            return;
        };
        let Ok(wv) = webview.cast::<ICoreWebView2_3>() else {
            return;
        };
        let _ = controller.0.SetIsVisible(!suspend);
        if suspend {
            let _ = wv.TrySuspend(None::<&ICoreWebView2TrySuspendCompletedHandler>);
        } else {
            let _ = wv.Resume();
        }
    }
}

#[cfg(not(windows))]
fn set_webview_suspended(_app: &AppHandle, _suspend: bool) {}

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

static FRAMELESS_PENDING: AtomicU64 = AtomicU64::new(0);

/// 与 schedule_apply 同款去抖：拖拽/缩放会连续触发 Moved/Resized，只在停稳后复位一次样式位
pub fn schedule_frameless(app: &AppHandle) {
    let token = FRAMELESS_PENDING.fetch_add(1, SeqCst) + 1;
    let handle = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(90));
        if FRAMELESS_PENDING.load(SeqCst) == token {
            if let Some(win) = handle.get_webview_window("main") {
                ensure_frameless(&win);
            }
        }
    });
}

/// 对应原版 nativeAppearance.ts applyNativeAppearance：DWM 帧延入客户区 + 主题染色 + 圆角 + Acrylic。
/// 不做帧扩展时 DWM 不会把桌面模糊合成进客户区，玻璃层只剩窗口底色，比原版暗约 80 灰度。
/// DWM 部分仅 Windows；macOS 锁 WebView 主题并给 NSWindow 实底色（见 cfg 块）。
pub fn apply_native_appearance(win: &tauri::WebviewWindow, settings: &serde_json::Value) {
    let light = is_light_theme(settings);
    // 对应原版 nativeTheme.themeSource = light ? 'light' : 'dark'：
    // 不锁死的话 WebView 跟随系统深色，prefers-color-scheme/滚动条/输入框全部走暗色
    let _ = win.set_theme(Some(if light {
        tauri::Theme::Light
    } else {
        tauri::Theme::Dark
    }));
    #[cfg(target_os = "macos")]
    {
        // macOS 的 NSWindow 是唯一可见底色（WKWebView 不画底色，见 setup_windows 注释），
        // 必须按主题给实底，否则壁纸直透、浅色主题下还压着深色底。色值取色板锚点：
        // 深=默认·黑紫 #212121，浅=蓝白 #edf0f7。外观设置变更时 ipc.rs 会重进这里。
        let (r, g, b) = if light {
            (0xED, 0xF0, 0xF7)
        } else {
            (0x21, 0x21, 0x21)
        };
        let _ = win.set_background_color(Some(tauri::window::Color(r, g, b, 255)));
    }
    #[cfg(windows)]
    {
        apply_dwm_appearance(win, light);
        // 圆角与「隐藏原生边框/标题/文字色」统一由 ensure_frameless 按最大化状态处理
        ensure_frameless(win);
    }
}

/// DWM 帧延入客户区 + 深色属性 + Acrylic 背景（仅 Windows）
#[cfg(windows)]
fn apply_dwm_appearance(win: &tauri::WebviewWindow, light: bool) {
    let Ok(hwnd) = win.hwnd() else { return };
    let h = hwnd.0 as *mut std::ffi::c_void;
    let dark: u32 = if light { 0 } else { 1 };
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
    }
}

/// 隐藏 DWM 画的原生边框与标题栏（对应 Axolotl apps/app/src/window_frame.rs 的做法）。
///
/// 之前的做法是清 WS_CAPTION/WS_SYSMENU 样式位，但 tao 0.37 在任何窗口标志变化时
/// 都会把这两个位写回（window_state.rs to_window_styles 无条件置位），于是两边拉锯，
/// 样式位在一次 maximize/show/DPI 变化后就回来了；而只要窗口带 WS_CAPTION + acrylic
/// backdrop，DWM 就会在客户区顶部画出原生 caption（白条 + 幽灵标题文字）。
///
/// Axolotl 用 DWM 属性从根上关掉：把边框色、标题色、文字色都设为 DWMWA_COLOR_NONE，
/// 不去碰样式位，DWM 自然不再画那段 caption，也就没有需要反复复位的东西。
/// 圆角按「非最大化」设置（最大化窗口不该有圆角）。
/// 非 Windows：无 DWM，直接空实现（macOS 走原生装饰，Linux 由 WM 决定）。
pub fn ensure_frameless(win: &tauri::WebviewWindow) {
    #[cfg(windows)]
    ensure_frameless_win(win);
    #[cfg(not(windows))]
    let _ = win;
}

/// 剥掉 WS_SYSMENU 的窗口子类：tao 0.37 在 to_window_styles 里无条件写回
/// WS_CAPTION|WS_SYSMENU（maximize/show/DPI 变化都会触发样式重算），而只要
/// WS_SYSMENU 在，DWM 就会在右上角画原生的最小化/最大化/关闭幽灵按钮，
/// 叠在自绘按钮上（「双重按钮」故障：曾只在最大化/全屏/DPI 变化后出现）。
/// 这里装一个子类过程：样式一变就把 WS_SYSMENU 摘掉（保留 WS_CAPTION 供
/// acrylic 阴影条使用，webview 客户区会盖住它）。
#[cfg(windows)]
fn strip_caption_sysmenu(hwnd: isize) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, GWL_STYLE, WS_SYSMENU,
    };
    unsafe {
        let style = GetWindowLongPtrW(hwnd as _, GWL_STYLE);
        if style & WS_SYSMENU as isize != 0 {
            SetWindowLongPtrW(hwnd as _, GWL_STYLE, style & !(WS_SYSMENU as isize));
        }
    }
}

#[cfg(windows)]
unsafe extern "system" fn frame_subclass_proc(
    hwnd: windows_sys::Win32::Foundation::HWND,
    msg: u32,
    wparam: windows_sys::Win32::Foundation::WPARAM,
    lparam: windows_sys::Win32::Foundation::LPARAM,
    _id: usize,
    _data: usize,
) -> windows_sys::Win32::Foundation::LRESULT {
    use windows_sys::Win32::UI::Shell::DefSubclassProc;
    use windows_sys::Win32::UI::WindowsAndMessaging::{GWL_STYLE, WM_STYLECHANGED};
    // wparam 高位 = GWL_STYLE 时表示窗口样式刚被改写，重新剥一次 WS_SYSMENU
    if msg == WM_STYLECHANGED && wparam as i32 == GWL_STYLE {
        strip_caption_sysmenu(hwnd as isize);
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}

#[cfg(windows)]
fn ensure_frameless_win(win: &tauri::WebviewWindow) {
    let maximized = win.is_maximized().unwrap_or(false);
    const DWMWCP_DEFAULT: u32 = 0;
    const DWMWCP_ROUND: u32 = 2;
    // 圆角按「非最大化」设置：最大化窗口不该有圆角。
    let corner = if maximized { DWMWCP_DEFAULT } else { DWMWCP_ROUND };
    strip_dwm_frame(win, corner);
}

/// 闪屏专用：铺满屏幕的透明图层不该有圆角，也不该有描边。
/// Windows 11 默认给每个窗口套圆角 + 1px 边框，屏幕边缘就会露出一圈透明框；
/// 闪屏要的是「只有碎片和头像」，所以这里用 DONOTROUND 而不是主窗口那套 ROUND。
pub fn ensure_borderless(win: &tauri::WebviewWindow) {
    #[cfg(windows)]
    {
        const DWMWCP_DONOTROUND: u32 = 1;
        strip_dwm_frame(win, DWMWCP_DONOTROUND);
    }
    #[cfg(not(windows))]
    let _ = win;
}

/// 去掉 DWM 画的窗口框：不圆角（或指定圆角）、描边/caption/文字色全设为「无」，
/// 并剥掉 WS_SYSMENU（否则右上角会叠原生幽灵按钮）。
#[cfg(windows)]
fn strip_dwm_frame(win: &tauri::WebviewWindow, corner: u32) {
    use windows_sys::Win32::UI::Shell::SetWindowSubclass;
    let Ok(hwnd) = win.hwnd() else { return };
    let h = hwnd.0 as *mut std::ffi::c_void;
    extern "system" {
        fn DwmSetWindowAttribute(
            hwnd: *mut std::ffi::c_void,
            attr: u32,
            value: *const u32,
            size: u32,
        ) -> i32;
    }
    const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
    const DWMWA_BORDER_COLOR: u32 = 34;
    const DWMWA_CAPTION_COLOR: u32 = 35;
    const DWMWA_TEXT_COLOR: u32 = 36;
    const DWMWA_COLOR_NONE: u32 = 0xFFFF_FFFE;
    // 装子类并立即剥一次 WS_SYSMENU（幂等：同 id 重复 SetWindowSubclass 只是替换）
    unsafe {
        SetWindowSubclass(hwnd.0, Some(frame_subclass_proc), 0x5748_4353, 0);
    }
    strip_caption_sysmenu(hwnd.0 as isize);
    unsafe {
        DwmSetWindowAttribute(h, DWMWA_WINDOW_CORNER_PREFERENCE, &corner, 4);
        // 三者一起设才有效：只设其一仍会留下原生 caption 的一段
        DwmSetWindowAttribute(h, DWMWA_BORDER_COLOR, &DWMWA_COLOR_NONE, 4);
        DwmSetWindowAttribute(h, DWMWA_CAPTION_COLOR, &DWMWA_COLOR_NONE, 4);
        DwmSetWindowAttribute(h, DWMWA_TEXT_COLOR, &DWMWA_COLOR_NONE, 4);
    }
}

/// 延后一小段再复位（最大化等窗口状态变化会重置圆角偏好，需在布局稳定后补设）
fn ensure_frameless_later(app: &AppHandle) {
    let handle = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(80));
        if let Some(win) = handle.get_webview_window("main") {
            ensure_frameless(&win);
        }
    });
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
