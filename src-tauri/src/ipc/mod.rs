//! Tauri IPC boundary and ordered subsystem composition.

pub mod dispatch;
mod resources;

use crate::{centercmd, frp, gate, launch, projections, recordings, windowctl};
use resources::drag_resource_files;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager, WebviewWindow};

#[tauri::command]
pub async fn ipc_invoke(
    app: AppHandle,
    _window: WebviewWindow,
    channel: String,
    args: Vec<Value>,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || dispatch::invoke(&app, &channel, &args))
        .await
        .map_err(|error| format!("后台任务崩溃：{error}"))?
}

#[tauri::command]
pub fn ipc_send(
    app: AppHandle,
    window: WebviewWindow,
    channel: String,
    args: Vec<Value>,
) -> Result<(), String> {
    send(&app, window.label() == "main", &channel, &args)
}

fn send(app: &AppHandle, from_main: bool, channel: &str, args: &[Value]) -> Result<(), String> {
    if let Some(result) = projections::send(app, channel, args) {
        return result;
    }
    if let Some(result) = recordings::send(app, channel, args) {
        return result;
    }
    match channel {
        "boot:renderer-ready" => {
            gate::renderer_ready(app);
            frp::restore(app);
            let states = launch::active_launch_states();
            if !states.is_empty() {
                if let Some(window) = app.get_webview_window("main") {
                    for state in states {
                        let _ =
                            tauri::Emitter::emit_to(&window, "main", "event:launchState", state);
                    }
                }
            } else {
                launch::restore_running_game(app);
            }
            Ok(())
        }
        "boot:stage" => {
            if let Some(stage) = args.first().and_then(|value| value.as_str()) {
                gate::stage(app, stage);
            }
            Ok(())
        }
        "boot:splash-ready" => {
            gate::splash_ready(app);
            Ok(())
        }
        "boot:assembled" => {
            gate::assembled(app);
            Ok(())
        }
        "boot:finished" => {
            gate::finish(app);
            Ok(())
        }
        "boot:splash-failed" => {
            gate::splash_failed(app);
            Ok(())
        }
        "fs:drag" => {
            fs_drag(app, args);
            Ok(())
        }
        _ => windowctl::command(app, from_main, channel, args).or_else(|error| {
            if error.starts_with("未实现的 send 通道") {
                Ok(())
            } else {
                Err(error)
            }
        }),
    }
}

pub use resources::fs_resolve;

fn fs_drag(app: &AppHandle, args: &[Value]) {
    let rel = args.first().and_then(|value| value.as_str()).unwrap_or("");
    let names = args.get(1).cloned().unwrap_or(Value::Null);
    let folder = args.get(2).and_then(|value| value.as_str());
    let message: String = (|| {
        let parts: Vec<&str> = rel.split('/').collect();
        let valid = parts.len() == 3
            && parts[0] == "versions"
            && !parts[1].is_empty()
            && !parts[1].contains('\\')
            && matches!(parts[2], "mods" | "resourcepacks" | "shaderpacks");
        if !valid {
            return "不支持拖出该目录".to_string();
        }
        let folder = match folder {
            Some(folder) if !folder.is_empty() => folder,
            _ => return "未指定游戏文件夹".to_string(),
        };
        let target = json!({ "folder": folder, "id": parts[1] });
        let (_, dir, _, _) = match centercmd::center_target(app, &target) {
            Ok(value) => value,
            Err(error) => return error,
        };
        match drag_resource_files(&dir.join(parts[2]), &names) {
            Ok(_) => "当前运行环境不支持原生拖出到系统".to_string(),
            Err(error) => error,
        }
    })();
    if let Some(window) = app.get_webview_window("main") {
        let _ = tauri::Emitter::emit_to(
            &window,
            "main",
            "files:dragError",
            format!("Error: {}", message),
        );
    }
}
