//! Main window and splash lifecycle.

use tauri::{Emitter, Manager};

/// Create the hidden main window and click-through splash window.
pub fn setup(app: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    use tauri::utils::config::WindowEffectsConfig;
    use tauri::webview::PageLoadEvent;
    use tauri::window::Effect;

    // Every new main window clears skin editor and mascot confirmation state.
    crate::windowctl::reset_palette_state();
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
        // Windows itself must stay opaque so DWM can compose the Acrylic effect.
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
                crate::gate::set_painted(&main_handle);
            }
        })
        // All page-level window.open calls go to the system browser.
        .on_new_window(move |url, _| {
            use tauri_plugin_opener::OpenerExt;
            let _ = main_opener.opener().open_url(url.as_str(), None::<&str>);
            tauri::webview::NewWindowResponse::Deny
        })
        .build()?;
    crate::windowctl::apply_native_appearance(&main, &crate::settings::get(app));
    main.hide()?;

    // Full-screen, click-through, always-on-top splash without a taskbar entry.
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
        .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
        .build()?;
    splash.set_ignore_cursor_events(true)?;
    let auto_fit = crate::settings::get(app)["uiWindowAutoFit"]
        .as_bool()
        .unwrap_or(false);
    crate::windowctl::configure_auto_fit(app, auto_fit);
    crate::gate::arm_watchdog(app);

    // 32 ms splash pointer timer; coordinates are converted from physical to DIP.
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
            if let Some(p) = crate::sysinfo::cursor_pos() {
                let scale = splash.scale_factor().unwrap_or(1.0);
                let origin = splash
                    .outer_position()
                    .map(|o| (o.x, o.y))
                    .unwrap_or((0, 0));
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
