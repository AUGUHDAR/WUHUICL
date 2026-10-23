//! 设置持久化：userData/settings.json（对应原版 core/settings.ts）。
//!
//! 策略与原版一致：默认值 JSON（镜像 defaults()）+ 文件内容浅合并 +
//! 少数字段深合并（resolution/custom/homeLayout/background/launchThumbnail）。

use serde_json::{json, Value};
use tauri::Manager;
use std::path::PathBuf;
use std::sync::Mutex;

static CACHE: Mutex<Option<Value>> = Mutex::new(None);

/// exe 所在目录（对应 PCL 的 Paths.Base）：便携化的根。
/// 取不到时回落 %APPDATA%\WUHUICL，保持可用。
fn exe_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?.to_path_buf();
    // current_exe 可能是临时目录里的短路径/符号链接，规范化成真实位置
    Some(dunce_canonical(&dir))
}

/// 规范化并去掉 `\\?\` 逐字前缀（dunce 的行为）：canonicalize 会把盘符路径
/// 变成 `\\?\D:\...`，直接入库会带进设置与 UI；UNC 前缀还原成 `\\server\share`。
fn dunce_canonical(path: &PathBuf) -> PathBuf {
    let canon = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
    let text = canon.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{}", rest));
    }
    if let Some(rest) = text.strip_prefix(r"\\?\") {
        return PathBuf::from(rest.to_string());
    }
    canon
}

/// WUHUICL 启动器数据根（对应 PCL 在 exe 旁建的 `PCL\` 文件夹）：
/// settings/账号/清单缓存/受管图片都落在这里，跟随 exe 移动。
pub fn user_data_dir<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> PathBuf {
    exe_dir()
        .map(|d| d.join(LAUNCHER_DATA_DIR_NAME))
        .unwrap_or_else(|| {
            app.path()
                .app_data_dir()
                .unwrap_or_else(|_| dirs::data_dir().unwrap_or_default().join("WUHUICL"))
        })
}

/// 启动器数据文件夹名（exe 旁）
pub const LAUNCHER_DATA_DIR_NAME: &str = "WUHUICL";

/// panic 日志落点：exe 旁 WUHUICL\logs\panic.log（panic hook 没有 AppHandle，
/// 单独走 exe_dir；hook 阶段不允许再 panic，全部静默失败）
pub fn user_data_dir_panic_log() -> std::path::PathBuf {
    let dir = exe_dir()
        .map(|d| d.join(LAUNCHER_DATA_DIR_NAME).join("logs"))
        .unwrap_or_else(std::env::temp_dir);
    let _ = std::fs::create_dir_all(&dir);
    dir.join("panic.log")
}

/// 旧版数据根（%APPDATA%\com.wuhuicl.app）：一次性迁移的来源。
fn legacy_user_data_dir<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Option<PathBuf> {
    let dir = app.path().app_data_dir().ok()?;
    let new_dir = user_data_dir(app);
    if same_dir(&dir, &new_dir) {
        None
    } else {
        Some(dir)
    }
}

fn same_dir(a: &PathBuf, b: &PathBuf) -> bool {
    let norm = |p: &PathBuf| {
        std::fs::canonicalize(p).unwrap_or_else(|_| p.clone()).to_string_lossy().to_lowercase()
    };
    norm(a) == norm(b)
}

/// 游戏根目录默认值：exe 旁 `.minecraft\`（对应 PCL 首启创建
/// `Paths.Base & ".minecraft\versions\"` 的逻辑，不再用 %APPDATA%）
pub fn default_game_dir() -> PathBuf {
    exe_dir()
        .unwrap_or_else(|| dirs::data_dir().unwrap_or_default())
        .join(".minecraft")
}

/// 旧默认游戏目录集合（%APPDATA%\.wuhuicl / %APPDATA%\.kamucl）：
/// 落盘设置还指向它们时会被改写到新的 exe 旁默认值
fn legacy_game_dirs() -> Vec<PathBuf> {
    let data = dirs::data_dir();
    let mut out = Vec::new();
    if let Some(d) = data {
        out.push(d.join(".wuhuicl"));
        out.push(d.join(".kamucl"));
    }
    out
}

/// 首启建目录 + 旧数据迁移（lib.rs setup 调用，先于任何 settings 读取）：
/// 1) PCL 式目录：exe 旁 `.minecraft\versions\` 与 `WUHUICL\`
/// 2) 旧 %APPDATA%\com.wuhuicl.app 的小文件与子目录一次性搬进 `WUHUICL\`
///    （条件：新目录还没有 settings.json；搬完不删旧目录，失败静默回退）
pub fn ensure_exe_side_dirs<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let new_dir = user_data_dir(app);
    let _ = std::fs::create_dir_all(&new_dir);
    let _ = std::fs::create_dir_all(default_game_dir().join("versions"));

    if let Some(old_dir) = legacy_user_data_dir(app) {
        let marker = new_dir.join("settings.json");
        let old_marker = old_dir.join("settings.json");
        if !marker.exists() && old_marker.exists() {
            for name in [
                "settings.json",
                "accounts.json",
                "yggdrasil-providers.json",
                "version_manifest.json",
                "running-game.json",
            ] {
                let from = old_dir.join(name);
                if from.is_file() {
                    let _ = std::fs::copy(&from, new_dir.join(name));
                }
            }
            for sub in ["yggdrasil-metadata", "authlib-injector", "launcher-managed"] {
                let from = old_dir.join(sub);
                if from.is_dir() {
                    let _ = copy_dir_recursive(&from, &new_dir.join(sub));
                }
            }
        }
    }
}

fn copy_dir_recursive(from: &PathBuf, to: &PathBuf) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// WUHUICL 自己的 Entra 应用注册：任何 Entra ID 租户 + 个人 Microsoft 账户，SPA 重定向 http://localhost
const MS_CLIENT_ID: &str = "ced46a35-61d5-434d-8bf7-9dbca71ef616";
/// 原版 shared/types.ts:978 的公共 client_id，社区版不再借用
const LEGACY_MS_CLIENT_ID: &str = "c36a9fb6-4f2a-41ff-90bd-ae7cc92031eb";

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
        "downloadThreads": 64,
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
        "splashFaceSource": "default",
        "splashFacePath": "",
        "logoFaceSource": "default",
        "logoFacePath": "",
        "background": { "mode": "none", "color": "#1b2437", "image": "", "opacity": 0.5, "blur": 0, "fit": "crop" },
        "launchThumbnail": { "image": "", "fit": "crop" },
        "skinEditorPalette": { "custom": [], "recent": [], "color": "#000000", "alpha": 1.0 },
        "closeAfterLaunch": false,
        "configVersion": 1
    })
}

/// 读取设置（内存缓存 → 文件 → 默认值），合并规则照抄 getSettings()
pub fn get<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Value {
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
    // 目录迁移：落盘设置还指向 %APPDATA% 旧默认（.wuhuicl/.kamucl）时，改写到 exe 旁 .minecraft；
    // 旧 C 盘目录保留为非默认条目，已有实例不丢。只生效一次（改完落盘）。
    if migrate_legacy_game_dir_entries(&mut merged) {
        let file = settings_file(app);
        if file.parent().map(|d| d.exists()).unwrap_or(false) {
            let bytes = serde_json::to_vec_pretty(&merged).unwrap_or_default();
            let _ = std::fs::write(&file, bytes);
        }
    }
    *CACHE.lock().unwrap() = Some(merged.clone());
    merged
}

/// 把 folders 里指向旧默认目录的条目改写到 exe 旁 .minecraft：
/// 默认条目原位替换；旧目录若是独立条目则保留（去默认化），旧的 gameDir/activeFolder 一并跟移。
fn migrate_legacy_game_dir_entries(merged: &mut Value) -> bool {
    let new_default = default_game_dir();
    let legacy = legacy_game_dirs();
    let is_legacy = |p: &str| -> bool {
        legacy
            .iter()
            .any(|l| same_dir(&PathBuf::from(p), l))
    };
    let mut changed = false;
    if let Some(folders) = merged.get_mut("folders").and_then(|f| f.as_array_mut()) {
        // 顺手归一化历史落盘的 \\?\ 逐字前缀路径
        for f in folders.iter_mut() {
            if let Some(p) = f["path"].as_str().map(str::to_string) {
                if let Some(stripped) = p.strip_prefix(r"\\?\UNC\").map(|r| format!(r"\\{}", r)) {
                    f["path"] = json!(stripped);
                    changed = true;
                } else if let Some(stripped) = p.strip_prefix(r"\\?\") {
                    f["path"] = json!(stripped);
                    changed = true;
                }
            }
        }
        let old_default_path = folders
            .iter()
            .find(|f| f["isDefault"] == true)
            .and_then(|f| f["path"].as_str())
            .map(str::to_string);
        let old_default_is_legacy = old_default_path.as_deref().map(is_legacy).unwrap_or(false);
        if old_default_is_legacy {
            if let Some(def) = folders.iter_mut().find(|f| f["isDefault"] == true) {
                def["path"] = json!(new_default.to_string_lossy());
                def["name"] = json!("默认文件夹");
                changed = true;
            }
        }
        // 旧 C 盘目录作为独立条目保留（去默认化），实例清单不丢
        if let Some(old_path) = &old_default_path {
            if old_default_is_legacy {
                let already_listed = folders.iter().any(|f| {
                    f["isDefault"] != true
                        && f["path"].as_str().map(|p| same_dir(&PathBuf::from(p), &PathBuf::from(old_path))).unwrap_or(false)
                });
                if !already_listed {
                    folders.push(json!({
                        "path": old_path,
                        "name": "旧目录（C 盘）",
                        "isDefault": false
                    }));
                }
            }
        }
    }
    if let Some(gd) = merged.get_mut("gameDir").and_then(|v| v.as_str()).map(str::to_string) {
        if is_legacy(&gd) {
            merged["gameDir"] = json!(new_default.to_string_lossy());
            changed = true;
        }
    }
    if let Some(af) = merged.get_mut("activeFolder").and_then(|v| v.as_str()).map(str::to_string) {
        if is_legacy(&af) {
            merged["activeFolder"] = json!(new_default.to_string_lossy());
            changed = true;
        }
    }
    changed
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
