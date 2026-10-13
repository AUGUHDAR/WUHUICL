//! 皮肤/披风模块（对应原版 core/skins.ts 主线）：官方 Minecraft Services API +
//! 本地历史皮肤（userData/skins + skins.json，sha1 去重，上限 30）+ 离线账号本地皮肤 +
//! 方块头像（ACTIVE 皮肤纹理 dataURL）。

use base64::Engine;
use serde_json::{json, Value};
use std::path::PathBuf;

const API: &str = "https://api.minecraftservices.com";
const HISTORY_LIMIT: usize = 30;

fn skins_dir(app: &tauri::AppHandle) -> PathBuf {
    crate::settings::user_data_dir(app).join("skins")
}
fn history_file(app: &tauri::AppHandle) -> PathBuf {
    crate::settings::user_data_dir(app).join("skins.json")
}

fn load_history(app: &tauri::AppHandle) -> Vec<Value> {
    std::fs::read(history_file(app))
        .ok()
        .and_then(|raw| serde_json::from_slice::<Value>(&raw).ok())
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
}

fn persist_history(app: &tauri::AppHandle, list: &[Value]) {
    let _ = std::fs::create_dir_all(skins_dir(app));
    if let Ok(bytes) = serde_json::to_vec_pretty(list) {
        let _ = std::fs::write(history_file(app), bytes);
    }
}

// ---------------- 账号 / token ----------------

fn selected_account(app: &tauri::AppHandle) -> Result<Value, String> {
    let selected = crate::mslogin::selected_full(app).ok_or("请先选择账号")?;
    Ok(selected)
}

fn require_mc_token(app: &tauri::AppHandle) -> Result<String, String> {
    let acc = selected_account(app)?;
    if acc["type"].as_str() != Some("microsoft") {
        return Err("皮肤功能需要微软正版账号".into());
    }
    let valid = crate::mslogin::get_valid_account(app, acc)?;
    valid["accessToken"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| "登录状态已失效，请重新登录微软账号".into())
}

// ---------------- PNG 校验与纹理下载 ----------------

/// 64×64 PNG 校验（魔数 + IHDR 尺寸），返回字节
fn validate_skin_png(file_path: &str) -> Result<Vec<u8>, String> {
    if file_path.is_empty() {
        return Err("请选择皮肤文件".into());
    }
    let Ok(meta) = std::fs::metadata(file_path) else {
        return Err("皮肤文件不存在".into());
    };
    if !meta.is_file() {
        return Err("皮肤文件不存在".into());
    }
    let buf = std::fs::read(file_path).map_err(|_| "皮肤文件不存在".to_string())?;
    if buf.len() < 24 || buf[0..4] != [0x89, 0x50, 0x4e, 0x47] {
        return Err("皮肤必须是 64×64 的 PNG 图片".into());
    }
    let width = u32::from_be_bytes([buf[16], buf[17], buf[18], buf[19]]);
    let height = u32::from_be_bytes([buf[20], buf[21], buf[22], buf[23]]);
    if width != 64 || height != 64 {
        return Err("皮肤必须是 64×64 的 PNG 图片".into());
    }
    Ok(buf)
}

fn to_data_url(bytes: &[u8]) -> String {
    format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))
}

/// 纹理下载转 dataURL（对应 downloadTexture：8MB 上限 + PNG 校验）
fn fetch_texture(url: &str) -> Option<Value> {
    let resp = crate::net::client().get(url).timeout(std::time::Duration::from_secs(30)).send().ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let bytes = resp.bytes().ok()?;
    if bytes.len() > 8 * 1024 * 1024 || bytes.len() < 8 || bytes[0..4] != [0x89, 0x50, 0x4e, 0x47] {
        return None;
    }
    Some(json!({ "dataUrl": to_data_url(&bytes) }))
}

// ---------------- 档案 ----------------

/// skin:profile → ProfileSkins（微软/Yggdrasil/离线三通道）
pub fn get_profile(app: &tauri::AppHandle, refresh: bool, account_id: Option<&str>) -> Result<Value, String> {
    let account = match account_id {
        Some(id) if !id.is_empty() => crate::mslogin::all_accounts_full(app)
            .into_iter()
            .find(|a| a["id"].as_str() == Some(id))
            .ok_or("请先选择账号")?,
        _ => selected_account(app)?,
    };
    fetch_profile(app, &account, refresh)
}

fn fetch_profile(app: &tauri::AppHandle, account: &Value, _refresh: bool) -> Result<Value, String> {
    match account["type"].as_str() {
        Some("offline") => {
            // 离线：本地皮肤库（userData/offline-skins/<accountId>.json）
            let store = crate::settings::user_data_dir(app).join("offline-skins");
            let file = store.join(format!("{}.json", account["id"].as_str().unwrap_or("")));
            let skin = std::fs::read(&file)
                .ok()
                .and_then(|raw| serde_json::from_slice::<Value>(&raw).ok());
            Ok(json!({
                "username": account["username"],
                "skins": skin.map(|s| Value::Array(vec![s])).unwrap_or_default(),
                "capes": []
            }))
        }
        Some("yggdrasil") => {
            let valid = crate::mslogin::get_valid_account(app, account.clone())?;
            crate::mslogin::external_profile_public(app, &valid)
        }
        _ => {
            let token = require_mc_token(app)?;
            let resp = crate::net::client()
                .get(format!("{API}/minecraft/profile"))
                .header("Authorization", format!("Bearer {token}"))
                .timeout(std::time::Duration::from_secs(30))
                .send()
                .map_err(|e| e.to_string())?;
            if !resp.status().is_success() {
                return Err(api_error(resp.status().as_u16(), "获取皮肤档案失败"));
            }
            let data: Value = resp.json().map_err(|e| e.to_string())?;
            let mut skins: Vec<Value> = data["skins"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|s| s["url"].as_str().map(|u| !u.is_empty()).unwrap_or(false))
                .map(|mut s| {
                    let variant = if s["variant"].as_str().unwrap_or("").eq_ignore_ascii_case("SLIM") {
                        "slim"
                    } else {
                        "classic"
                    };
                    s["variant"] = json!(variant);
                    s
                })
                .collect();
            // ACTIVE 排最前
            skins.sort_by_key(|s| std::cmp::Reverse(s["state"].as_str() == Some("ACTIVE")));
            let mut capes: Vec<Value> = data["capes"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|mut c| {
                    c["alias"] = json!(c["alias"].as_str().filter(|s| !s.is_empty()).unwrap_or("披风"));
                    c["active"] = json!(c["state"].as_str() == Some("ACTIVE"));
                    c
                })
                .collect();
            for s in skins.iter_mut() {
                if let Some(extra) = fetch_texture(s["url"].as_str().unwrap_or("")) {
                    if let (Some(a), Some(b)) = (s.as_object_mut(), extra.as_object()) {
                        for (k, v) in b {
                            a.insert(k.clone(), v.clone());
                        }
                    }
                }
            }
            for c in capes.iter_mut() {
                if let Some(url) = c["url"].as_str() {
                    if let Some(extra) = fetch_texture(url) {
                        if let (Some(a), Some(b)) = (c.as_object_mut(), extra.as_object()) {
                            for (k, v) in b {
                                a.insert(k.clone(), v.clone());
                            }
                        }
                    }
                }
            }
            Ok(json!({
                "username": data["name"].as_str().unwrap_or(""),
                "skins": skins,
                "capes": capes
            }))
        }
    }
}

/// 官方 API 错误 → 中文文案（apiError）
fn api_error(status: u16, fallback: &str) -> String {
    match status {
        401 | 403 => "登录状态已失效或无权限，请重新登录微软账号".into(),
        404 => "该账号未拥有 Minecraft 或尚未创建游戏档案".into(),
        429 => "操作过于频繁，请稍后再试".into(),
        _ => format!("{fallback}（HTTP {status}）"),
    }
}

// ---------------- 上传 / 披风 ----------------

/// skin:upload：multipart 上传 + 历史记录
pub fn upload_skin(app: &tauri::AppHandle, file_path: &str, variant: &str, account_id: Option<&str>) -> Result<Value, String> {
    if variant != "classic" && variant != "slim" {
        return Err("无效的皮肤模型".into());
    }
    let account = match account_id {
        Some(id) if !id.is_empty() => crate::mslogin::all_accounts_full(app)
            .into_iter()
            .find(|a| a["id"].as_str() == Some(id))
            .ok_or("请先选择账号")?,
        _ => selected_account(app)?,
    };
    let buf = validate_skin_png(file_path)?;
    let token = require_mc_token(app)?;
    let boundary = format!("----wuhuicl{}", crate::accounts::uuid_v4().replace('-', ""));
    let mut body: Vec<u8> = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"variant\"\r\n\r\n{variant}\r\n").as_bytes());
    let file_name = PathBuf::from(file_path)
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default()
        .to_string();
    body.extend_from_slice(
        format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{file_name}\"\r\nContent-Type: image/png\r\n\r\n").as_bytes(),
    );
    body.extend_from_slice(&buf);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let resp = crate::net::client()
        .post(format!("{API}/minecraft/profile/skins"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", format!("multipart/form-data; boundary={boundary}"))
        .timeout(std::time::Duration::from_secs(60))
        .body(body)
        .send()
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(api_error(resp.status().as_u16(), "皮肤上传失败"));
    }
    save_history(app, &buf, variant, &file_name);
    get_profile(app, true, account["id"].as_str())
}

/// skin:cape：PUT 激活 / DELETE 卸下
pub fn change_cape(app: &tauri::AppHandle, cape_id: Option<&str>) -> Result<Value, String> {
    let token = require_mc_token(app)?;
    let url = format!("{API}/minecraft/profile/capes/active");
    let req = match cape_id {
        None | Some("") => crate::net::client().delete(&url),
        Some(id) => crate::net::client()
            .put(&url)
            .header("Content-Type", "application/json")
            .body(json!({ "capeId": id }).to_string()),
    };
    let resp = req
        .header("Authorization", format!("Bearer {token}"))
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(api_error(resp.status().as_u16(), "披风更换失败"));
    }
    get_profile(app, true, None)
}

// ---------------- 历史皮肤 ----------------

fn sha1_hex(bytes: &[u8]) -> String {
    crate::net::net_sha1_hex(bytes)
}

/// sha1 去重保存，上限 30 条（saveHistory）
fn save_history(app: &tauri::AppHandle, buf: &[u8], variant: &str, source_name: &str) {
    let hash = sha1_hex(buf);
    let mut list = load_history(app);
    if let Some(existing_pos) = list.iter().position(|i| i["hash"].as_str() == Some(hash.as_str())) {
        let mut item = list.remove(existing_pos);
        item["time"] = json!(chrono::Utc::now().timestamp_millis());
        list.insert(0, item);
    } else {
        let id = crate::accounts::uuid_v4();
        let _ = std::fs::create_dir_all(skins_dir(app));
        let _ = std::fs::write(skins_dir(app).join(format!("{id}.png")), buf);
        list.insert(
            0,
            json!({
                "id": id,
                "variant": variant,
                "time": chrono::Utc::now().timestamp_millis(),
                "name": if source_name.is_empty() { Value::Null } else { json!(source_name) },
                "hash": hash
            }),
        );
    }
    list.truncate(HISTORY_LIMIT);
    persist_history(app, &list);
}

/// skin:history → SkinHistoryEntry[]（带 dataUrl，新→旧）
pub fn history(app: &tauri::AppHandle) -> Value {
    let entries: Vec<Value> = load_history(app)
        .iter()
        .filter_map(|item| {
            let id = item["id"].as_str()?;
            let bytes = std::fs::read(skins_dir(app).join(format!("{id}.png"))).ok()?;
            let mut entry = item.clone();
            entry["dataUrl"] = json!(to_data_url(&bytes));
            Some(entry)
        })
        .collect();
    Value::Array(entries)
}

pub fn history_delete(app: &tauri::AppHandle, id: &str) -> Value {
    let mut list = load_history(app);
    list.retain(|i| i["id"].as_str() != Some(id));
    persist_history(app, &list);
    let _ = std::fs::remove_file(skins_dir(app).join(format!("{id}.png")));
    history(app)
}

pub fn history_rename(app: &tauri::AppHandle, id: &str, name: &str) -> Value {
    let mut list = load_history(app);
    if let Some(item) = list.iter_mut().find(|i| i["id"].as_str() == Some(id)) {
        item["name"] = json!(name);
    }
    persist_history(app, &list);
    history(app)
}

/// skin:uploadHistory：把历史副本重新上传
pub fn upload_history(app: &tauri::AppHandle, id: &str) -> Result<Value, String> {
    let item = load_history(app)
        .into_iter()
        .find(|i| i["id"].as_str() == Some(id))
        .ok_or("历史记录不存在")?;
    let path = skins_dir(app).join(format!("{id}.png"));
    upload_skin(app, &path.to_string_lossy(), item["variant"].as_str().unwrap_or("classic"), None)
}

// ---------------- 离线账号本地皮肤 ----------------

/// skin:offlineApply：本地 PNG 应用到离线账号（不联网，游戏启动时经 offlineSkinLaunch 注入）
pub fn apply_offline_skin(app: &tauri::AppHandle, file_path: &str, variant: &str, account_id: &str) -> Result<Value, String> {
    if account_id.is_empty() {
        return Err("请重新选择离线账号并确认应用".into());
    }
    let accounts = crate::mslogin::all_accounts_full(app);
    let account = accounts
        .iter()
        .find(|a| a["id"].as_str() == Some(account_id))
        .ok_or("请重新选择离线账号并确认应用")?;
    if account["type"].as_str() != Some("offline") {
        return Err("请先选择离线账号".into());
    }
    let buf = validate_skin_png(file_path)?;
    let store = crate::settings::user_data_dir(app).join("offline-skins");
    let _ = std::fs::create_dir_all(&store);
    let snapshot = json!({
        "variant": variant,
        "dataUrl": to_data_url(&buf),
        "name": PathBuf::from(file_path).file_name().map(|n| n.to_string_lossy()).unwrap_or_default(),
        "appliedAt": chrono::Utc::now().timestamp_millis()
    });
    std::fs::write(store.join(format!("{account_id}.json")), serde_json::to_vec(&snapshot).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    fetch_profile(app, account, true)
}

/// skin:offlineReset
pub fn reset_offline_skin(app: &tauri::AppHandle, account_id: &str) -> Result<Value, String> {
    if account_id.is_empty() {
        return Err("请重新选择离线账号并确认应用".into());
    }
    let _ = std::fs::remove_file(
        crate::settings::user_data_dir(app)
            .join("offline-skins")
            .join(format!("{account_id}.json")),
    );
    let accounts = crate::mslogin::all_accounts_full(app);
    let account = accounts
        .iter()
        .find(|a| a["id"].as_str() == Some(account_id))
        .ok_or("请重新选择离线账号并确认应用")?;
    fetch_profile(app, account, true)
}

/// skin:avatar → dataURL | null（ACTIVE 皮肤，不抛错）
pub fn get_avatar(app: &tauri::AppHandle, account_id: Option<&str>) -> Value {
    get_profile(app, false, account_id)
        .ok()
        .and_then(|p| p["skins"].as_array().and_then(|s| s.first().cloned()))
        .and_then(|s| s["dataUrl"].as_str().map(str::to_string))
        .map(|u| json!(u))
        .unwrap_or(Value::Null)
}

/// skin:editorSave / editorUpload：64×64 dataURL 校验（编辑器导出）
fn validate_data_url_png(value: &str) -> Result<Vec<u8>, String> {
    let raw = value.strip_prefix("data:image/png;base64,").ok_or("皮肤数据无效")?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(raw)
        .map_err(|_| "皮肤数据无效".to_string())?;
    if bytes.len() < 24 || bytes[0..4] != [0x89, 0x50, 0x4e, 0x47] {
        return Err("皮肤必须是 64×64 的 PNG 图片".into());
    }
    let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    if width != 64 || height != 64 {
        return Err("皮肤必须是 64×64 的 PNG 图片".into());
    }
    Ok(bytes)
}

pub fn editor_save(app: &tauri::AppHandle, value: &str) -> Result<Value, String> {
    let bytes = validate_data_url_png(value)?;
    use tauri_plugin_dialog::DialogExt;
    let picked = app
        .dialog()
        .file()
        .add_filter("PNG 图片", &["png"])
        .set_file_name("skin.png")
        .blocking_save_file();
    let Some(path) = picked else { return Ok(json!(false)) };
    std::fs::write(path.to_string(), bytes).map_err(|e| e.to_string())?;
    Ok(json!(true))
}

pub fn editor_upload(app: &tauri::AppHandle, value: &str, variant: &str, account_id: &str) -> Result<Value, String> {
    let bytes = validate_data_url_png(value)?;
    let tmp = std::env::temp_dir().join(format!("wuhuicl-skin-{}.png", crate::accounts::uuid_v4()));
    std::fs::write(&tmp, &bytes).map_err(|e| e.to_string())?;
    let result = if account_id.is_empty() {
        Err("请先选择账号".into())
    } else {
        let accounts = crate::mslogin::all_accounts_full(app);
        match accounts.iter().find(|a| a["id"].as_str() == Some(account_id)).map(|a| a["type"].as_str().unwrap_or("").to_string()) {
            Some(t) if t == "offline" => apply_offline_skin(app, &tmp.to_string_lossy(), variant, account_id),
            Some(t) if t == "microsoft" => {
                let _ = account_id;
                upload_skin(app, &tmp.to_string_lossy(), variant, None)
            }
            _ => Err("请先选择账号".into()),
        }
    };
    let _ = std::fs::remove_file(&tmp);
    result
}

/// 统一入口
pub fn invoke(app: &tauri::AppHandle, channel: &str, args: &[Value]) -> Option<Result<Value, String>> {
    match channel {
        "skin:profile" => Some(get_profile(
            app,
            args.first().and_then(|v| v.as_bool()).unwrap_or(false),
            args.get(1).and_then(|v| v.as_str()),
        )),
        "skin:upload" => Some(upload_skin(
            app,
            args.first().and_then(|v| v.as_str()).unwrap_or(""),
            args.get(1).and_then(|v| v.as_str()).unwrap_or("classic"),
            args.get(2).and_then(|v| v.as_str()),
        )),
        "skin:cape" => Some(change_cape(app, args.first().and_then(|v| v.as_str()))),
        "skin:offlineApply" => Some(apply_offline_skin(
            app,
            args.first().and_then(|v| v.as_str()).unwrap_or(""),
            args.get(1).and_then(|v| v.as_str()).unwrap_or("classic"),
            args.get(2).and_then(|v| v.as_str()).unwrap_or(""),
        )),
        "skin:offlineReset" => Some(reset_offline_skin(
            app,
            args.first().and_then(|v| v.as_str()).unwrap_or(""),
        )),
        "skin:history" => Some(Ok(history(app))),
        "skin:historyDelete" => Some(Ok(history_delete(
            app,
            args.first().and_then(|v| v.as_str()).unwrap_or(""),
        ))),
        "skin:historyRename" => Some(Ok(history_rename(
            app,
            args.first().and_then(|v| v.as_str()).unwrap_or(""),
            args.get(1).and_then(|v| v.as_str()).unwrap_or(""),
        ))),
        "skin:uploadHistory" => Some(upload_history(
            app,
            args.first().and_then(|v| v.as_str()).unwrap_or(""),
        )),
        "skin:avatar" => Some(Ok(get_avatar(app, args.first().and_then(|v| v.as_str())))),
        "skin:editorSave" => Some(editor_save(app, args.first().and_then(|v| v.as_str()).unwrap_or(""))),
        "skin:editorUpload" => Some(editor_upload(
            app,
            args.first().and_then(|v| v.as_str()).unwrap_or(""),
            args.get(1).and_then(|v| v.as_str()).unwrap_or("classic"),
            args.get(2).and_then(|v| v.as_str()).unwrap_or(""),
        )),
        _ => None,
    }
}
