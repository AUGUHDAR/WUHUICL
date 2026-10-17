//! Application composition: Tauri builder, managed state, and window lifecycle.

pub mod windows;

use crate::gate::BootGate;
use std::sync::Mutex;

/// Global runtime state shared by the boot gate and window lifecycle.
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
        .invoke_handler(tauri::generate_handler![
            crate::ipc::ipc_invoke,
            crate::ipc::ipc_send
        ])
        .register_uri_scheme_protocol("kamucl-asset", crate::assetproto::handle)
        .setup(|app| {
            crate::vlcli::session::bind(app.handle());
            windows::setup(app.handle())?;
            Ok(())
        })
        .on_window_event(|window, event| {
            crate::windowctl::on_window_event(window, event);
        })
        .build(tauri::generate_context!())
        .expect("WUHUICL 启动失败")
        .run(|app_handle, event| {
            // Mirror the Electron before-quit guards for skin editor and mascot state.
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                if crate::windowctl::palette_blocked() {
                    api.prevent_exit();
                    crate::windowctl::note_deferred_quit();
                    crate::windowctl::emit_palette_close(app_handle, true);
                    return;
                }
                if crate::windowctl::mascot_pending() {
                    api.prevent_exit();
                    crate::windowctl::emit_mascot_close(app_handle, true);
                }
                return;
            }
            // Release multiplayer resources after all windows are closed.
            if let tauri::RunEvent::Exit = event {
                crate::frp::shutdown();
                crate::terracotta::stop_on_quit(app_handle);
            }
        });
}
