//! 设置持久化：userData/settings.json（对应原版 core/settings.ts）。
//!
//! 策略与原版一致：默认值 JSON（镜像 defaults()）+ 文件内容浅合并 +
//! 少数字段深合并（resolution/custom/homeLayout/background/launchThumbnail）。

use serde_json::{json, Value};
use tauri::Manager;
use std::path::PathBuf;
use std::sync::Mutex;

static CACHE: Mutex<Option<Value>> = Mutex::new(None);

/// WUHUICL 自己的 Entra 应用注册：任何 Entra ID 租户 + 个人 Microsoft 账户，SPA 重定向 http://localhost
const MS_CLIENT_ID: &str = "ced46a35-61d5-434d-8bf7-9dbca71ef616";
/// 原版 shared/types.ts:978 的公共 client_id，社区版不再借用
const LEGACY_MS_CLIENT_ID: &str = "c36a9fb6-4f2a-41ff-90bd-ae7cc92031eb";

/// userData 根目录（WUHUICL 自己的，不碰原版 KAMUCL 数据）
pub fn user_data_dir<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| dirs::data_dir().unwrap_or_default().join("WUHUICL"))
}

/// 游戏根目录默认值：%APPDATA%/.wuhuicl（与原版 .kamucl 平行，不共用）
pub fn default_game_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".wuhuicl")
}

pub fn settings_file<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> PathBuf {
    user_data_dir(app).join("settings.json")
}

/// 镜像 core/settings.ts defaults()（含 shared/types.ts 各 DEFAULT_* 常量）
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

/// 读取设置（内存缓存 → 文件 → 默认值），合并规则照抄 getSettings()
pub fn get(app: &tauri::AppHandle) -> Value {
    if let Some(cached) = CACHE.lock().unwrap().as_ref() {
        return cached.clone();
    }
    let def = defaults();
    let merged = std::fs::read(settings_file(app))
        .ok()
        .and_then(|raw| serde_json::from_slice::<Value>(&raw).ok())
        .map(|raw| merge(def, raw))
        .unwrap_or_else(|| {
            // 首次运行：确保默认游戏目录存在（对应 ensureDefaultGameFolder）
            let d = defaults();
            let _ = std::fs::create_dir_all(default_game_dir());
            d
        });
    // 对应 settings.ts:153 的默认值迁移：老落盘值里还写着公共 client_id 的，换成社区版自己的应用
    let mut merged = merged;
    if merged.get("msClientId").and_then(|v| v.as_str()) == Some(LEGACY_MS_CLIENT_ID) {
        merged["msClientId"] = Value::String(MS_CLIENT_ID.to_string());
    }
    *CACHE.lock().unwrap() = Some(merged.clone());
    merged
}

/// 浅合并 + 指定字段深合并（对应 getSettings() 的合并顺序）
fn merge(def: Value, raw: Value) -> Value {
    let mut out = def.as_object().cloned().unwrap_or_default();
    let raw_obj = raw.as_object().cloned().unwrap_or_default();
    for (k, v) in raw_obj {
        match k.as_str() {
            "resolution" | "custom" | "background" | "launchThumbnail" | "skinEditorPalette" => {
                let base = out.get(&k).cloned().unwrap_or(Value::Null);
                out.insert(k, deep_merge(base, v));
            }
            "homeLayout" => {
                // main/side 必须是数组，否则回落默认
                let base = out.get(&k).cloned().unwrap_or(json!({}));
                out.insert(k, deep_merge(base, v));
            }
            "folders" => {
                if !v.is_array() || v.as_array().map(|a| a.is_empty()).unwrap_or(true) {
                    // 保留默认
                } else {
                    out.insert(k, v);
                }
            }
            "activeFolder" => {
                let fallback = out.get("gameDir").cloned().unwrap_or(Value::Null);
                let v = if v.is_null() { fallback } else { v };
                out.insert(k, v);
            }
            _ => {
                out.insert(k, v);
            }
        }
    }
    Value::Object(out)
}

fn deep_merge(base: Value, overlay: Value) -> Value {
    match (base, overlay) {
        (Value::Object(mut b), Value::Object(o)) => {
            for (k, v) in o {
                let bv = b.remove(&k).unwrap_or(Value::Null);
                b.insert(k, deep_merge(bv, v));
            }
            Value::Object(b)
        }
        (_, o) => o,
    }
}

/// 保存设置补丁（settings:set）：合并后落盘，返回完整设置
pub fn save(app: &tauri::AppHandle, patch: Value) -> Result<Value, String> {
    // 校验合并后的整份设置（原版 settings.ts:187 validateDownloadLimits({ ...cur, ...patch })），
    // 只改主题时补丁里没有下载字段，拿补丁校验会误判成限额非法。
    let current = merge_and_validate(&get(app), patch)?;
    let file = settings_file(app);
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let bytes = serde_json::to_vec_pretty(&current).map_err(|e| e.to_string())?;
    std::fs::write(&file, bytes).map_err(|e| e.to_string())?;
    *CACHE.lock().unwrap() = Some(current.clone());
    crate::download::configure_limiter_from_settings(app);
    Ok(current)
}

/// 合并补丁并校验下载限额，返回可直接落盘的完整设置。
fn merge_and_validate(current: &Value, patch: Value) -> Result<Value, String> {
    let mut merged = current.clone();
    let obj = merged.as_object_mut().ok_or("设置损坏")?;
    for (k, v) in patch.as_object().cloned().unwrap_or_default() {
        obj.insert(k, v);
    }
    crate::download::validate_limits(&merged)?;
    Ok(merged)
}

/// 受管图片根目录（kamucl-asset 协议的授权范围）
pub fn managed_dir(app: &tauri::AppHandle) -> PathBuf {
    user_data_dir(app).join("launcher-managed")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Value {
        json!({ "appearance": { "theme": "default" }, "downloadThreads": 16, "downloadSpeedKBps": 0 })
    }

    #[test]
    fn theme_only_patch_is_not_blocked_by_download_limits() {
        let merged = merge_and_validate(&base(), json!({ "appearance": { "theme": "whiteBlue" } })).unwrap();
        assert_eq!(merged["appearance"]["theme"], "whiteBlue");
        assert_eq!(merged["downloadThreads"], 16);
    }

    #[test]
    fn out_of_range_threads_still_rejected() {
        let e = merge_and_validate(&base(), json!({ "downloadThreads": 999 })).unwrap_err();
        assert!(e.contains("1–64"), "实际 {}", e);
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
