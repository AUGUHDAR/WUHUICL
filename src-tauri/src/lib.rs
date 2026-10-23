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

/// 注入到主窗口的「像应用不像浏览器」守卫：屏蔽刷新/打印/开发者工具/缩放快捷键与右键菜单。
/// 这些是 WebView2 默认行为，桌面应用不该把它们暴露给用户。
/// Ctrl+C/V/X/A 与 Ctrl+F 之类的文本编辑/查找不加拦截——输入框里仍要正常工作。
pub const DESKTOP_APP_GUARDS: &str = r#"
(() => {
    const isEditable = (el) =>
        !!el && (el.isContentEditable ||
            ['INPUT', 'TEXTAREA', 'SELECT'].includes(el.tagName));

    window.addEventListener(
        'keydown',
        (event) => {
            const key = (event.key || '').toLowerCase();
            const ctrl = event.ctrlKey || event.metaKey;
            // F5 / Ctrl+R：刷新
            if (key === 'f5' || (ctrl && key === 'r')) {
                event.preventDefault();
                return false;
            }
            // F7：光标浏览；F12 / Ctrl+Shift+I,J,C：开发者工具
            if (key === 'f7') {
                event.preventDefault();
                return false;
            }
            if (
                key === 'f12' ||
                (ctrl && event.shiftKey && ['i', 'j', 'c'].includes(key))
            ) {
                event.preventDefault();
                return false;
            }
            // Ctrl+P：打印
            if (ctrl && key === 'p') {
                event.preventDefault();
                return false;
            }
            // Ctrl+U：查看源码
            if (ctrl && key === 'u') {
                event.preventDefault();
                return false;
            }
            // Ctrl +/-/0：页面缩放（应用有自己的界面缩放，不吃浏览器缩放）
            if (ctrl && ['+', '=', '-', '_', '0'].includes(key)) {
                event.preventDefault();
                return false;
            }
        },
        true
    );

    // 右键菜单：输入框内保留系统编辑菜单（复制/粘贴），其余位置屏蔽
    window.addEventListener(
        'contextmenu',
        (event) => {
            const target = event.target;
            if (isEditable(target)) return;
            const selection = window.getSelection && window.getSelection();
            if (selection && selection.toString().length > 0) return;
            event.preventDefault();
        },
        true
    );
})();
"#;

pub mod accounts;
pub mod appearance;
pub mod assetproto;
pub mod dragout;
pub mod exportlogs;
pub mod feedback;
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
pub mod crashanalyzer;
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

use base64::Engine;
use gate::BootGate;
use std::sync::Mutex;
use tauri::{Emitter, Manager};


/// 当前线程正在处理的 IPC 通道(ipc_invoke 入口写入),OOM 探针用它归因
thread_local! {
    static CURRENT_IPC: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}
pub fn set_current_ipc(channel: &str) {
    CURRENT_IPC.with(|c| *c.borrow_mut() = channel.to_string());
}
pub fn current_ipc() -> String {
    CURRENT_IPC.with(|c| c.borrow().clone())
}

fn append_panic_log(text: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(crate::settings::user_data_dir_panic_log())
    {
        let _ = f.write_all(text.as_bytes());
    }
}

/// 分配失败探针：OOM(=c0000409 fastfail)前把「请求大小/对齐」落盘，
/// 否则崩溃后无从判断是巨大单次分配还是累积耗尽。
/// 只在分配失败那一刻写文件，正常路径零开销。
struct OomProbe;

unsafe impl std::alloc::GlobalAlloc for OomProbe {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        if layout.size() >= 64 * 1024 * 1024 {
            let bt = std::backtrace::Backtrace::force_capture();
            append_panic_log(&format!(
                "[alloc-big] size={} align={} thread={:?} ipc={}\n{bt}\n",
                layout.size(),
                layout.align(),
                std::thread::current().name().unwrap_or("<unnamed>"),
                current_ipc()
            ));
        }
        let ptr = std::alloc::System.alloc(layout);
        if ptr.is_null() {
            let bt = std::backtrace::Backtrace::force_capture();
            append_panic_log(&format!(
                "[alloc-fail] size={} align={} thread={:?} ipc={}\n{bt}\n",
                layout.size(),
                layout.align(),
                std::thread::current().name().unwrap_or("<unnamed>"),
                current_ipc()
            ));
        }
        ptr
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        std::alloc::System.dealloc(ptr, layout)
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: std::alloc::Layout, new_size: usize) -> *mut u8 {
        // ≥64MB 的扩容先记一笔(成功也记):崩溃前的"最后一次大分配"就是调用方
        if new_size >= 64 * 1024 * 1024 {
            let bt = std::backtrace::Backtrace::force_capture();
            append_panic_log(&format!(
                "[realloc-big] old={} new={} thread={:?} ipc={}\n{bt}\n",
                layout.size(),
                new_size,
                std::thread::current().name().unwrap_or("<unnamed>"),
                current_ipc()
            ));
        }
        let p = std::alloc::System.realloc(ptr, layout, new_size);
        if p.is_null() {
            let bt = std::backtrace::Backtrace::force_capture();
            append_panic_log(&format!(
                "[realloc-fail] old={} new={} thread={:?} ipc={}\n{bt}\n",
                layout.size(),
                new_size,
                std::thread::current().name().unwrap_or("<unnamed>"),
                current_ipc()
            ));
        }
        p
    }
}

#[global_allocator]
static ALLOC: OomProbe = OomProbe;

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
    // panic = abort：panic 消息只写 stderr，双击启动时无人看见。
    // 把 panic 信息落盘 user_data/logs/panic.log，崩溃后能直接定位。
    std::panic::set_hook(Box::new(|info| {
        let thread = std::thread::current();
        let name = thread.name().unwrap_or("<unnamed>").to_string();
        let msg = format!(
            "[{}] panic in thread `{}`: {}\n  at {}\n",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
            name,
            info,
            info.location()
                .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
                .unwrap_or_default()
        );
        let _ = std::fs::write(
            crate::settings::user_data_dir_panic_log(),
            &msg,
        );
        eprintln!("{}", msg);
    }));
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![ipc::ipc_invoke, ipc::ipc_send])
        .register_uri_scheme_protocol("kamucl-asset", assetproto::handle)
        .setup(|app| {
            // PCL 式目录：exe 旁 .minecraft\versions 与 WUHUICL\，并把旧 %APPDATA% 数据一次性搬来
            crate::settings::ensure_exe_side_dirs(app.handle());
            crate::vlcli::session::bind(app.handle());
            // 社区整合包下载完成后的安装走 modpacks.rs（对应原版动态 import installModpack）
            crate::community::set_modpack_install_hook(std::sync::Arc::new(
                crate::modpacks::install_modpack_job,
            ));
            // 模组迁移到未安装版本时现场安装实例（mods.rs 迁移钩子）
            crate::mods::set_migration_install_hook(std::sync::Arc::new(
                crate::installer::migration_install_hook,
            ));
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
        // 应用而非浏览器：拦掉 WebView2 自带的刷新/打印/缩放/查看源码/右键菜单。
        // 页面脚本运行前注入，所以这些按键在任何视图里都不会漏出去。
        .initialization_script(DESKTOP_APP_GUARDS)
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
    // 自定义开屏头像：设置指向受管方块头 PNG 时以 data URL 注入开屏页；读取失败走内置默认图
    let splash_face_script = {
        let s = crate::settings::get(app);
        let path = s["splashFacePath"].as_str().unwrap_or("");
        let custom = s["splashFaceSource"].as_str() == Some("custom") && !path.is_empty();
        if custom {
            std::fs::read(path)
                .ok()
                .map(|bytes| base64::engine::general_purpose::STANDARD.encode(bytes))
                .map(|b64| format!("window.__KAMUCL_SPLASH_FACE__='data:image/png;base64,{b64}';"))
                .unwrap_or_else(|| "window.__KAMUCL_SPLASH_FACE__='';".into())
        } else {
            "window.__KAMUCL_SPLASH_FACE__='';".to_string()
        }
    };
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
        .initialization_script(splash_face_script.as_str())
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
