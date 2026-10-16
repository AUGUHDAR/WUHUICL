//! WUHUICL — Tauri 版主进程。
//!
//! 结构对齐 Electron 原版 main/：
//! - ipc.rs        通用 IPC 路由（通道名与原版 1:1）
//! - gate.rs       启动门（boot:stage / renderer-ready / splash 编排，对应 startupSplash.ts）
//! - settings.rs   设置持久化（对应 core/settings.ts 的 defaults/merge）
//! - accounts.rs   账号（对应 core/accounts.ts 的本地部分）
//! - versions.rs   已安装版本扫描（对应 core/versions.ts 的读取部分）
//! - sysinfo.rs    系统信息 / 光标 / 内存
//! - windowctl.rs  窗口控制（minimize/maximize/close，对应 main/index.ts 的 window:* 通道）
//! - assetproto.rs kamucl-asset 受管图片协议（对应 protocol.handle('kamucl-asset')）

pub mod accounts;
pub mod appearance;
pub mod assetproto;
pub mod exportlogs;
pub mod gamedir;
pub mod gameoptions;
pub mod download;
pub mod misc;
pub mod mods;
pub mod mslogin;
pub mod modpacks;
pub mod nbt;
pub mod net;
pub mod recordings;
pub mod typednbt;
pub mod bridge;
pub mod direct;
pub mod frp;
pub mod terracotta;
pub mod voxlink;
pub mod vl;
pub mod projections;
pub mod catalog;
pub mod centercmd;
pub mod community;
pub mod gate;
pub mod gamefolders;
pub mod installer;
pub mod ipc;
pub mod javaman;
pub mod launch;
pub mod selfupdate;
pub mod servers;
pub mod settings;
pub mod skins;
pub mod sysinfo;
pub mod tasks;
pub mod versions;
pub mod vlcli;
pub mod windowctl;
pub mod worlds;

use gate::BootGate;
use std::sync::Mutex;
use tauri::{Emitter, Manager};

/// 全局状态：启动门 + 各子系统的惰性句柄
pub struct AppState {
    pub gate: Mutex<BootGate>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            gate: Mutex::new(BootGate::new()),
        }
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![ipc::ipc_invoke, ipc::ipc_send])
        .register_uri_scheme_protocol("kamucl-asset", assetproto::handle)
        .setup(|app| {
            crate::vlcli::session::bind(app.handle());
            crate::setup_windows(app.handle())?;
            Ok(())
        })
        .on_window_event(|window, event| {
            windowctl::on_window_event(window, event);
        })
        .build(tauri::generate_context!())
        .expect("WUHUICL 启动失败")
        .run(|app_handle, event| {
            // 对应原版 index.ts:326-330 before-quit：皮肤编辑器/桌宠确认中先拦住退出
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                if windowctl::palette_blocked() {
                    api.prevent_exit();
                    windowctl::note_deferred_quit();
                    windowctl::emit_palette_close(app_handle, true);
                    return;
                }
                if windowctl::mascot_pending() {
                    api.prevent_exit();
                    windowctl::emit_mascot_close(app_handle, true);
                }
                return;
            }
            // 对应原版 index.ts:331-335：窗口全部关闭后清理联机资源（voxlink/陶瓦/frp）
            if let tauri::RunEvent::Exit = event {
                frp::shutdown();
                terracotta::stop_on_quit(app_handle);
            }
        });
}

/// 创建主窗口（隐藏，等渲染层就绪）与全屏闪屏（点击穿透）。
/// 对应原版 startupSplash.ts createElectronStartupSplash + index.ts createWindow。
fn setup_windows(app: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    use tauri::webview::PageLoadEvent;
    use tauri::utils::config::WindowEffectsConfig;
    use tauri::window::Effect;

    // 对应原版 createWindow：新窗口一律清空编辑器/桌宠的确认态
    windowctl::reset_palette_state();
    use tauri::{WebviewUrl, WebviewWindowBuilder};

    let main_handle = app.clone();
    let main_opener = app.clone();
    let main = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("WUHUICL 启动器")
        .inner_size(1360.0, 860.0)
        .min_inner_size(960.0, 620.0)
        .center()
        .decorations(false)
        .resizable(true)
        .visible(false)
        // 对应原版 windowAppearance.ts：Windows 上窗口本身不能 transparent（layered window 会绕过
        // DWM 的 Acrylic 合成），透明只交给 WebContents 的 backgroundColor alpha。
        .transparent(false)
        .background_color(tauri::window::Color(0, 0, 0, 0))
        .effects(WindowEffectsConfig {
            effects: vec![Effect::Acrylic],
            state: None,
            radius: None,
            color: None,
            interactive: false,
        })
        .on_page_load(move |_window, event| {
            if matches!(event.event(), PageLoadEvent::Finished) {
                gate::set_painted(&main_handle);
            }
        })
        // 对应原版 index.ts:189-192 setWindowOpenHandler：页内所有 window.open 交给系统浏览器，
        // 应用内不开第二个窗口。少了这一段，Tauri 默认拒绝新窗口 → 全站链接点了没反应。
        .on_new_window(move |url, _| {
            use tauri_plugin_opener::OpenerExt;
            let _ = main_opener.opener().open_url(url.as_str(), None::<&str>);
            tauri::webview::NewWindowResponse::Deny
        })
        .build()?;
    // 对应原版 index.ts:126 applyNativeAppearance(win, getSettings())
    windowctl::apply_native_appearance(&main, &settings::get(app));
    main.hide()?;

    // 闪屏：全屏、点击穿透、置顶、无任务栏
    // 原版取 screen.getPrimaryDisplay().bounds（DIP），Tauri 的 position/inner_size 也按逻辑像素解释，
    // 所以物理尺寸必须除以 scale_factor，否则高 DPI 下闪屏会放大并偏出屏幕。
    let monitor = app.primary_monitor()?.ok_or("没有主显示器")?;
    let scale = monitor.scale_factor();
    let size = monitor.size();
    let pos = monitor.position();
    let splash = WebviewWindowBuilder::new(app, "splash", WebviewUrl::App("splash.html".into()))
        .title("WUHUICL · 正在启动")
        .position(pos.x as f64 / scale, pos.y as f64 / scale)
        .inner_size(size.width as f64 / scale, size.height as f64 / scale)
        .decorations(false)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .skip_taskbar(true)
        .focused(false)
        .visible(false)
        .transparent(true)
        .always_on_top(true)
        // 对应原版 startupSplash.ts:30：闪屏一律拒绝新窗口（不转发到浏览器）
        .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
        .build()?;
    splash.set_ignore_cursor_events(true)?;
    let auto_fit = crate::settings::get(app)["uiWindowAutoFit"].as_bool().unwrap_or(false);
    windowctl::configure_auto_fit(app, auto_fit);
    gate::arm_watchdog(app);

    // 光标跟随定时器（32ms，对应原版 pointerTimer）
    let timer_app = app.clone();
    std::thread::spawn(move || {
        let app = timer_app;
        loop {
        std::thread::sleep(std::time::Duration::from_millis(32));
        let Some(splash) = app.get_webview_window("splash") else {
            break;
        };
        if !splash.is_visible().unwrap_or(false) {
            continue;
        }
        if let Some(p) = sysinfo::cursor_pos() {
            // GetCursorPos 与 outer_position 都是物理像素，渲染层要的是 DIP（对应原版 screen.*）
            let scale = splash.scale_factor().unwrap_or(1.0);
            let origin = splash.outer_position().map(|o| (o.x, o.y)).unwrap_or((0, 0));
            let _ = splash.emit_to(
                "splash",
                "boot:pointer",
                serde_json::json!({
                    "x": (p.0 - origin.0) as f64 / scale,
                    "y": (p.1 - origin.1) as f64 / scale,
                }),
            );
            }
        }
    });

    Ok(())
}
