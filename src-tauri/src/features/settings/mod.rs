//! Settings persistence at userData/settings.json.
//!
//! Defaults are shallow-merged with persisted values, with deep merges for the
//! structured appearance and layout fields.

use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::Manager;

static CACHE: Mutex<Option<Value>> = Mutex::new(None);

const MS_CLIENT_ID: &str = "ced46a35-61d5-434d-8bf7-9dbca71ef616";
const LEGACY_MS_CLIENT_ID: &str = "c36a9fb6-4f2a-41ff-90bd-ae7cc92031eb";

pub fn user_data_dir<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| dirs::data_dir().unwrap_or_default().join("WUHUICL"))
}

pub fn default_game_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".wuhuicl")
}

pub fn settings_file<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> PathBuf {
    user_data_dir(app).join("settings.json")
}

pub fn defaults() -> Value {
    let game_dir = default_game_dir();
    let game_dir_s = game_dir.to_string_lossy().to_string();
    json!({
        "gameDir": game_dir_s,
        "folders": [{ "path": game_dir_s, "name": "默认文件夹", "isDefault": true }],
        "activeFolder": game_dir_s,
        "javaPath": "",
        "javaAuto": true,
        "javaCustom": [],
        "javaHidden": [],
        "memoryMB": 4096,
        "memoryAuto": true,
        "jvmArgs": "",
        "resolution": { "width": 854, "height": 480, "mode": "windowed", "fullscreen": false },
        "mirror": "bmclapi",
        "downloadThreads": 16,
        "downloadSpeedKBps": 0,
        "defaultIsolation": true,
        "reduceMotion": false,
        "msClientId": MS_CLIENT_ID,
        "theme": "transparent",
        "custom": {
            "colors": {
                "accent": "#2563eb", "bg": "#edf0f7", "card": "#ffffff", "text": "#1b2437",
                "textDim": "#68718a", "border": "#e1e6f0", "sidebarBg": "#f5f7fb",
                "sidebarText": "#1b2437", "bannerText": "#ffffff"
            },
            "layout": { "sidebarWidth": 208, "bannerHeight": 430, "radius": 14 }
        },
        "disabledFeatures": [],
        "favoriteVersions": [],
        "homeLayout": {
            "main": [
                { "key": "banner", "visible": true },
                { "key": "logDrawer", "visible": true },
                { "key": "recentGames", "visible": true }
            ],
            "side": [
                { "key": "accountCard", "visible": true },
                { "key": "sysInfo", "visible": true },
                { "key": "quickActions", "visible": true },
                { "key": "authorCard", "visible": true }
            ]
        },
        "background": { "mode": "none", "color": "#1b2437", "image": "", "opacity": 0.5, "blur": 0, "fit": "crop" },
        "launchThumbnail": { "image": "", "fit": "crop" },
        "skinEditorPalette": { "custom": [], "recent": [], "color": "#000000", "alpha": 1.0 },
        "closeAfterLaunch": false,
        "configVersion": 1
    })
}

pub fn get(app: &tauri::AppHandle) -> Value {
    if let Some(cached) = CACHE.lock().unwrap().as_ref() {
        return cached.clone();
    }
    let defaults = self::defaults();
    let merged = std::fs::read(settings_file(app))
        .ok()
        .and_then(|raw| serde_json::from_slice::<Value>(&raw).ok())
        .map(|raw| merge(defaults, raw))
        .unwrap_or_else(|| {
            let defaults = self::defaults();
            let _ = std::fs::create_dir_all(default_game_dir());
            defaults
        });
    let mut merged = merged;
    if merged.get("msClientId").and_then(|value| value.as_str()) == Some(LEGACY_MS_CLIENT_ID) {
        merged["msClientId"] = Value::String(MS_CLIENT_ID.to_string());
    }
    *CACHE.lock().unwrap() = Some(merged.clone());
    merged
}

fn merge(defaults: Value, raw: Value) -> Value {
    let mut output = defaults.as_object().cloned().unwrap_or_default();
    let raw_object = raw.as_object().cloned().unwrap_or_default();
    for (key, value) in raw_object {
        match key.as_str() {
            "resolution" | "custom" | "background" | "launchThumbnail" | "skinEditorPalette" => {
                let base = output.get(&key).cloned().unwrap_or(Value::Null);
                output.insert(key, deep_merge(base, value));
            }
            "homeLayout" => {
                let base = output.get(&key).cloned().unwrap_or(json!({}));
                output.insert(key, deep_merge(base, value));
            }
            "folders" => {
                if value.is_array()
                    && !value
                        .as_array()
                        .map(|items| items.is_empty())
                        .unwrap_or(true)
                {
                    output.insert(key, value);
                }
            }
            "activeFolder" => {
                let fallback = output.get("gameDir").cloned().unwrap_or(Value::Null);
                output.insert(key, if value.is_null() { fallback } else { value });
            }
            _ => {
                output.insert(key, value);
            }
        }
    }
    Value::Object(output)
}

fn deep_merge(base: Value, overlay: Value) -> Value {
    match (base, overlay) {
        (Value::Object(mut base), Value::Object(overlay)) => {
            for (key, value) in overlay {
                let previous = base.remove(&key).unwrap_or(Value::Null);
                base.insert(key, deep_merge(previous, value));
            }
            Value::Object(base)
        }
        (_, overlay) => overlay,
    }
}

pub fn save(app: &tauri::AppHandle, patch: Value) -> Result<Value, String> {
    let current = merge_and_validate(&get(app), patch)?;
    let file = settings_file(app);
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    }
    let bytes = serde_json::to_vec_pretty(&current).map_err(|error| error.to_string())?;
    std::fs::write(&file, bytes).map_err(|error| error.to_string())?;
    *CACHE.lock().unwrap() = Some(current.clone());
    crate::download::configure_limiter_from_settings(app);
    Ok(current)
}

fn merge_and_validate(current: &Value, patch: Value) -> Result<Value, String> {
    let mut merged = current.clone();
    let object = merged.as_object_mut().ok_or("设置损坏")?;
    for (key, value) in patch.as_object().cloned().unwrap_or_default() {
        object.insert(key, value);
    }
    crate::download::validate_limits(&merged)?;
    Ok(merged)
}

pub fn managed_dir(app: &tauri::AppHandle) -> PathBuf {
    user_data_dir(app).join("launcher-managed")
}

pub fn invoke(
    app: &tauri::AppHandle,
    channel: &str,
    args: &[Value],
) -> Option<Result<Value, String>> {
    match channel {
        "settings:get" => Some(Ok(get(app))),
        "settings:set" => Some(save(
            app,
            args.first().cloned().unwrap_or_else(|| json!({})),
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Value {
        json!({ "appearance": { "theme": "default" }, "downloadThreads": 16, "downloadSpeedKBps": 0 })
    }

    #[test]
    fn theme_only_patch_is_not_blocked_by_download_limits() {
        let merged =
            merge_and_validate(&base(), json!({ "appearance": { "theme": "whiteBlue" } })).unwrap();
        assert_eq!(merged["appearance"]["theme"], "whiteBlue");
        assert_eq!(merged["downloadThreads"], 16);
    }

    #[test]
    fn out_of_range_threads_still_rejected() {
        let error = merge_and_validate(&base(), json!({ "downloadThreads": 999 })).unwrap_err();
        assert!(error.contains("1–64"), "实际 {}", error);
    }

    #[test]
    fn partial_limit_pair_validates_against_merged_value() {
        let merged = merge_and_validate(&base(), json!({ "downloadSpeedKBps": 512 })).unwrap();
        assert_eq!(merged["downloadSpeedKBps"], 512);
        assert_eq!(merged["downloadThreads"], 16);
    }

    #[test]
    fn missing_limits_in_storage_falls_back_to_defaults() {
        let stored = json!({ "appearance": {} });
        let merged = merge_and_validate(&stored, json!({ "appearance": { "theme": "pink" } }));
        assert!(merged.is_err(), "默认值缺失时应报限额非法，不能静默写盘");
    }
}
