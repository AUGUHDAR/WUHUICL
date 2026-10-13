//! 账号：本地持久化 + 离线账号（对应原版 core/accounts.ts 的本地部分）。
//! 微软设备码 / Yggdrasil 全链路在子系统移植阶段补齐（网络登录）。

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

/// 对外列表剥离凭据（对应原版 listAccounts 的脱敏口径）
fn public(a: &Value) -> Value {
    let mut o = a.as_object().cloned().unwrap_or_default();
    for k in ["accessToken", "refreshToken", "clientToken", "loginIdentifier", "userProperties"] {
        o.remove(k);
    }
    Value::Object(o)
}

pub fn list(app: &tauri::AppHandle) -> Value {
    let data = load(app);
    let list = data["accounts"].as_array().cloned().unwrap_or_default();
    Value::Array(list.iter().map(public).collect())
}

/// 离线账号：uuid 由用户名确定性派生（Java 离线惯例 UUID v3 "OfflinePlayer:name"）
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
        .any(|a| a["type"] == "offline" && a["username"] == name)
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
        list.retain(|a| a["id"] != id);
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
        .and_then(|l| l.iter().find(|a| a["id"] == id))
        .cloned();
    data["selected"] = found
        .as_ref()
        .map(|a| a["id"].clone())
        .unwrap_or(Value::Null);
    save(app, &data);
    found.map(|a| public(&a)).unwrap_or(Value::Null)
}

pub fn selected(app: &tauri::AppHandle) -> Value {
    let data = load(app);
    let sel = data["selected"].as_str().unwrap_or("");
    data["accounts"]
        .as_array()
        .and_then(|l| l.iter().find(|a| a["id"] == sel))
        .map(public)
        .unwrap_or(Value::Null)
}

/// uuid v4（ProcessPrng 优先，失败时间兜底；仅作本地 id）
pub fn uuid_v4() -> String {
    let mut b = [0u8; 16];
    if !fill_random(&mut b) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        for (i, chunk) in b.chunks_mut(8).enumerate() {
            let v = (now >> (i * 32)) as u64;
            for (j, byte) in chunk.iter_mut().enumerate() {
                *byte = ((v >> (j * 8)) & 0xff) as u8;
            }
        }
    }
    b[6] = (b[6] & 0x0f) | 0x40;
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

fn fill_random(buf: &mut [u8]) -> bool {
    use rand::RngCore;
    rand::rng().fill_bytes(buf);
    true
}
