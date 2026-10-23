//! 外观系统（对应原版 core/visualTheme / appearanceDraft / appearanceAssets /
//! appearanceAssetActions / assetsSettings）：主题草稿、受管背景/启动卡图、
//! kamucl-asset 协议的受管根。图片处理用 image crate 等价 imageAssetProcessor。

use base64::Engine;
use serde_json::{json, Value};
use std::path::PathBuf;

fn draft_file(app: &tauri::AppHandle) -> PathBuf {
    crate::settings::user_data_dir(app).join("appearance-draft.json")
}

fn managed_root(app: &tauri::AppHandle) -> PathBuf {
    crate::settings::user_data_dir(app).join("launcher-managed")
}

// ---------------- 主题（visualTheme.ts） ----------------

/// appearance:resetTheme：清空 settings.visualDesign
fn reset_theme(app: &tauri::AppHandle) -> Result<Value, String> {
    crate::settings::save(app, json!({ "visualDesign": Value::Null }))?;
    Ok(Value::Null)
}

/// appearance:exportTheme：导出主题代码（base64 JSON）
fn export_theme(app: &tauri::AppHandle) -> Result<Value, String> {
    let s = crate::settings::get(app);
    let design = s.get("visualDesign").cloned().unwrap_or(Value::Null);
    if design.is_null() {
        return Err("当前没有可导出的自定义外观".into());
    }
    let bytes = serde_json::to_vec(&design).map_err(|e| e.to_string())?;
    Ok(json!(format!(
        "WUHUICL-THEME:{}",
        base64::engine::general_purpose::URL_SAFE.encode(bytes)
    )))
}

/// appearance:importTheme：解析主题代码写入（preview=true 时不落盘，直接返回设计）
fn import_theme(app: &tauri::AppHandle, code: &str, preview: bool) -> Result<Value, String> {
    let raw = code
        .trim()
        .strip_prefix("WUHUICL-THEME:")
        .or_else(|| code.trim().strip_prefix("KAMUCL-THEME:"))
        .ok_or("无法识别的外观代码")?;
    let bytes = base64::engine::general_purpose::URL_SAFE
        .decode(raw.trim())
        .map_err(|_| "外观代码已损坏".to_string())?;
    let design: Value = serde_json::from_slice(&bytes).map_err(|_| "外观代码已损坏".to_string())?;
    if !preview {
        crate::settings::save(app, json!({ "visualDesign": design }))?;
    }
    Ok(design)
}

// ---------------- 草稿（appearanceDraft.ts） ----------------

fn draft_read(app: &tauri::AppHandle) -> Value {
    std::fs::read(draft_file(app))
        .ok()
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or(Value::Null)
}

fn draft_save(app: &tauri::AppHandle, value: &Value) -> Result<Value, String> {
    let file = draft_file(app);
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(&file, serde_json::to_vec(value).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    Ok(Value::Null)
}

fn draft_discard(app: &tauri::AppHandle) -> Result<Value, String> {
    let _ = std::fs::remove_file(draft_file(app));
    Ok(Value::Null)
}

fn draft_apply(app: &tauri::AppHandle, value: &Value) -> Result<Value, String> {
    crate::settings::save(app, json!({ "visualDesign": value }))?;
    draft_discard(app)?;
    Ok(crate::settings::get(app))
}

// ---------------- 受管图（appearanceAssets / appearanceAssetActions） ----------------

/// 图片校验 + 复制进受管根，返回受管绝对路径（importBackground 等共用）
fn import_managed_image(app: &tauri::AppHandle, src: &PathBuf, kind: &str) -> Result<String, String> {
    let bytes = std::fs::read(src).map_err(|_| "图片文件不存在".to_string())?;
    if bytes.len() > 32 * 1024 * 1024 {
        return Err("图片文件过大".into());
    }
    let format = image::guess_format(&bytes).map_err(|_| "图片无法解码（支持 PNG/JPG/WebP）".to_string())?;
    if !matches!(format, image::ImageFormat::Png | image::ImageFormat::Jpeg | image::ImageFormat::WebP) {
        return Err("图片无法解码（支持 PNG/JPG/WebP）".into());
    }
    let root = managed_root(app).join(kind);
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let ext = match format {
        image::ImageFormat::Png => "png",
        image::ImageFormat::Jpeg => "jpg",
        _ => "webp",
    };
    let name = format!("{}-{}.{}", chrono::Local::now().format("%Y%m%d%H%M%S"), &crate::accounts::uuid_v4()[..8], ext);
    let dest = root.join(&name);
    std::fs::write(&dest, bytes).map_err(|e| e.to_string())?;
    Ok(dest.to_string_lossy().to_string())
}

/// appearance:saveFaceImage(dataUrl, slot) → 受管路径。开屏头像/左上角图标：
/// 前端已把皮肤渲染成方块头 PNG，这里校验后落盘到受管 faces/ 目录。
fn save_face_image(app: &tauri::AppHandle, args: &[Value]) -> Result<Value, String> {
    let data_url = args.first().and_then(|v| v.as_str()).unwrap_or("");
    let slot = args.get(1).and_then(|v| v.as_str()).unwrap_or("");
    if !matches!(slot, "splash" | "logo") {
        return Err("未知的形象槽位".into());
    }
    let raw = data_url
        .strip_prefix("data:image/png;base64,")
        .ok_or("头像数据无效")?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(raw)
        .map_err(|_| "头像数据无效".to_string())?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err("头像图片过大".into());
    }
    if bytes.len() < 24 || bytes[0..4] != [0x89, 0x50, 0x4e, 0x47] {
        return Err("头像必须是 PNG 图片".into());
    }
    let root = managed_root(app).join("faces");
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let dest = root.join(format!("{slot}.png"));
    std::fs::write(&dest, &bytes).map_err(|e| e.to_string())?;
    Ok(json!(dest.to_string_lossy().to_string()))
}

/// appearance:resetFaceImage(slot)：删除自定义头图文件（设置字段由前端一并清空）
fn reset_face_image(app: &tauri::AppHandle, args: &[Value]) -> Result<Value, String> {
    let slot = args.first().and_then(|v| v.as_str()).unwrap_or("");
    if !matches!(slot, "splash" | "logo") {
        return Err("未知的形象槽位".into());
    }
    let _ = std::fs::remove_file(managed_root(app).join("faces").join(format!("{slot}.png")));
    Ok(Value::Null)
}

fn patch_background(app: &tauri::AppHandle, patch: Value) -> Result<Value, String> {
    crate::settings::save(app, json!({ "background": patch }))
}

fn patch_launch_thumbnail(app: &tauri::AppHandle, patch: Value) -> Result<Value, String> {
    crate::settings::save(app, json!({ "launchThumbnail": patch }))
}

fn pick_images(app: &tauri::AppHandle, multi: bool) -> Vec<String> {
    use tauri_plugin_dialog::DialogExt;
    let dialog = app.dialog().file().add_filter("图片", &["png", "jpg", "jpeg", "webp"]);
    if multi {
        dialog
            .blocking_pick_files()
            .map(|ps| ps.iter().map(|p| p.to_string()).collect())
            .unwrap_or_default()
    } else {
        dialog
            .blocking_pick_file()
            .map(|p| vec![p.to_string()])
            .unwrap_or_default()
    }
}

/// appearance:importBackground → Settings | null
fn import_background(app: &tauri::AppHandle, multi: bool) -> Result<Value, String> {
    let picked = pick_images(app, multi);
    if picked.is_empty() {
        return Ok(Value::Null);
    }
    let mut images = Vec::new();
    for p in &picked {
        images.push(import_managed_image(app, &PathBuf::from(p), "backgrounds")?);
    }
    let s = crate::settings::get(app);
    let mut bg = s["background"].clone();
    if let Some(obj) = bg.as_object_mut() {
        obj["mode"] = json!("image");
        let mut list: Vec<String> = s["background"]["images"].as_array().cloned().unwrap_or_default().iter().filter_map(|v| v.as_str().map(str::to_string)).collect();
        list.push(images.remove(0));
        list.extend(images);
        obj["images"] = json!(list);
        obj["image"] = json!(list[0]);
    }
    patch_background(app, bg)
}

/// appearance:resetBackground → Settings
fn reset_background(app: &tauri::AppHandle) -> Result<Value, String> {
    patch_background(app, json!({ "mode": "none", "color": "#1b2437", "image": "", "opacity": 0.5, "blur": 0, "fit": "crop" }))
}

fn import_launch_thumbnail(app: &tauri::AppHandle) -> Result<Value, String> {
    let picked = pick_images(app, true);
    if picked.is_empty() {
        return Ok(Value::Null);
    }
    let image = import_managed_image(app, &PathBuf::from(&picked[0]), "launch-thumbnails")?;
    patch_launch_thumbnail(app, json!({ "image": image, "fit": "crop" }))
}

fn reset_launch_thumbnail(app: &tauri::AppHandle) -> Result<Value, String> {
    patch_launch_thumbnail(app, json!({ "image": "", "fit": "crop" }))
}

/// 统一入口
pub fn invoke(app: &tauri::AppHandle, channel: &str, args: &[Value]) -> Option<Result<Value, String>> {
    match channel {
        "appearance:resetTheme" => Some(reset_theme(app)),
        "appearance:exportTheme" => Some(export_theme(app)),
        "appearance:importTheme" => Some(import_theme(
            app,
            args.first().and_then(|v| v.as_str()).unwrap_or(""),
            args.get(1).and_then(|v| v.as_bool()).unwrap_or(false),
        )),
        "appearance:draftRead" => Some(Ok(draft_read(app))),
        "appearance:draftSave" => Some(draft_save(app, &args.first().cloned().unwrap_or(Value::Null))),
        "appearance:draftDiscard" => Some(draft_discard(app)),
        "appearance:draftApply" => Some(draft_apply(app, &args.first().cloned().unwrap_or(Value::Null))),
        "appearance:importBackground" => Some(import_background(app, false)),
        "appearance:importBackgroundMulti" => Some(import_background(app, true)),
        "appearance:resetBackground" => Some(reset_background(app)),
        "appearance:saveFaceImage" => Some(save_face_image(app, args)),
        "appearance:resetFaceImage" => Some(reset_face_image(app, args)),
        "appearance:importLaunchThumbnail" => Some(import_launch_thumbnail(app)),
        "appearance:resetLaunchThumbnail" => Some(reset_launch_thumbnail(app)),
        _ => None,
    }
}
