//! Accounts: local persistence and offline account support.

use md5::{Digest, Md5};
use serde_json::{json, Value};
use std::path::PathBuf;

fn accounts_file(app: &tauri::AppHandle) -> PathBuf {
    crate::settings::user_data_dir(app).join("accounts.json")
}

fn load(app: &tauri::AppHandle) -> Value {
    std::fs::read(accounts_file(app))
        .ok()
        .and_then(|raw| serde_json::from_slice::<Value>(&raw).ok())
        .unwrap_or_else(|| json!({ "accounts": [], "selected": null }))
}

fn save(app: &tauri::AppHandle, data: &Value) {
    let file = accounts_file(app);
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(data) {
        let _ = std::fs::write(file, bytes);
    }
}

/// Strip credentials before exposing accounts to the frontend.
fn public(account: &Value) -> Value {
    let mut output = account.as_object().cloned().unwrap_or_default();
    for key in [
        "accessToken",
        "refreshToken",
        "clientToken",
        "loginIdentifier",
        "userProperties",
    ] {
        output.remove(key);
    }
    Value::Object(output)
}

pub fn list(app: &tauri::AppHandle) -> Value {
    let data = load(app);
    let list = data["accounts"].as_array().cloned().unwrap_or_default();
    Value::Array(list.iter().map(public).collect())
}

/// Java-compatible deterministic offline UUID for OfflinePlayer:name.
fn offline_uuid(name: &str) -> String {
    let mut input = b"OfflinePlayer:".to_vec();
    input.extend_from_slice(name.as_bytes());
    let mut hasher = Md5::new();
    hasher.update(&input);
    let b: [u8; 16] = hasher.finalize().into();
    let mut b = b;
    b[6] = (b[6] & 0x0f) | 0x30;
    b[8] = (b[8] & 0x3f) | 0x80;
    format!(
        "{}-{}-{}-{}-{}",
        hex::encode(&b[0..4]),
        hex::encode(&b[4..6]),
        hex::encode(&b[6..8]),
        hex::encode(&b[8..10]),
        hex::encode(&b[10..16])
    )
}

pub fn add_offline(app: &tauri::AppHandle, username: &str) -> Result<Value, String> {
    let name = username.trim();
    if name.is_empty() || name.len() > 16 {
        return Err("用户名必须为 1-16 个字符".into());
    }
    let mut data = load(app);
    let accounts = data["accounts"].as_array_mut().ok_or("账号数据损坏")?;
    if accounts
        .iter()
        .any(|account| account["type"] == "offline" && account["username"] == name)
    {
        return Err("该离线账号已存在".into());
    }
    let account = json!({
        "id": uuid_v4(),
        "type": "offline",
        "username": name,
        "uuid": offline_uuid(name)
    });
    accounts.push(account.clone());
    save(app, &data);
    Ok(account)
}

pub fn remove(app: &tauri::AppHandle, id: &str) -> Value {
    let mut data = load(app);
    if let Some(list) = data["accounts"].as_array_mut() {
        list.retain(|account| account["id"] != id);
    }
    if data["selected"] == id {
        data["selected"] = Value::Null;
    }
    save(app, &data);
    list(app)
}

pub fn select(app: &tauri::AppHandle, id: &str) -> Value {
    let mut data = load(app);
    let found = data["accounts"]
        .as_array()
        .and_then(|list| list.iter().find(|account| account["id"] == id))
        .cloned();
    data["selected"] = found
        .as_ref()
        .map(|account| account["id"].clone())
        .unwrap_or(Value::Null);
    save(app, &data);
    found.map(|account| public(&account)).unwrap_or(Value::Null)
}

pub fn selected(app: &tauri::AppHandle) -> Value {
    let data = load(app);
    let selected = data["selected"].as_str().unwrap_or("");
    data["accounts"]
        .as_array()
        .and_then(|list| {
            list.iter()
                .find(|account| account["id"].as_str() == Some(selected))
        })
        .map(public)
        .unwrap_or(Value::Null)
}

pub fn invoke(
    app: &tauri::AppHandle,
    channel: &str,
    args: &[Value],
) -> Option<Result<Value, String>> {
    match channel {
        "accounts:list" => Some(Ok(list(app))),
        "accounts:addOffline" => Some((|| {
            let name = args
                .first()
                .and_then(|value| value.as_str())
                .ok_or("缺少用户名")?;
            add_offline(app, name)
        })()),
        "accounts:remove" => Some((|| {
            let id = args
                .first()
                .and_then(|value| value.as_str())
                .ok_or("缺少 id")?;
            Ok(remove(app, id))
        })()),
        "accounts:select" => Some((|| {
            let id = args
                .first()
                .and_then(|value| value.as_str())
                .ok_or("缺少 id")?;
            Ok(select(app, id))
        })()),
        "accounts:selected" => Some(Ok(selected(app))),
        _ => None,
    }
}

/// UUID v4 for local identifiers; uses OS randomness and a time fallback.
pub fn uuid_v4() -> String {
    let mut bytes = [0u8; 16];
    if !fill_random(&mut bytes) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        for (index, chunk) in bytes.chunks_mut(8).enumerate() {
            let value = (now >> (index * 32)) as u64;
            for (offset, byte) in chunk.iter_mut().enumerate() {
                *byte = ((value >> (offset * 8)) & 0xff) as u8;
            }
        }
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{}-{}-{}-{}-{}",
        hex::encode(&bytes[0..4]),
        hex::encode(&bytes[4..6]),
        hex::encode(&bytes[6..8]),
        hex::encode(&bytes[8..10]),
        hex::encode(&bytes[10..16])
    )
}

fn fill_random(buffer: &mut [u8]) -> bool {
    use rand::RngCore;
    rand::rng().fill_bytes(buffer);
    true
}
