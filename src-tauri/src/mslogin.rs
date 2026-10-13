//! 账号网络子系统（对应原版 core/accounts.ts 的微软 device code / 刷新链路、
//! core/yggdrasil.ts、core/yggdrasilProvider.ts；凭据加密对应 electron safeStorage，
//! Rust 端以 DPAPI CryptProtectData/CryptUnprotectData 等价实现）。
//!
//! 通道：accounts:msBegin / msCancel / refresh / yggProviders / yggProbe /
//!       yggSaveProvider / yggRemoveProvider / yggLogin / yggSelectProfile / yggRuntime
//!
//! 存储：与 crate::accounts 共用 userData/accounts.json（{accounts, selected}）；
//! 敏感字段（accessToken/refreshToken/clientToken/loginIdentifier/userProperties）
//! 写入 `secure` 子对象（DPAPI 密文 base64），对外列表一律剥离。

use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

const MS_SCOPE: &str = "XboxLive.signin offline_access";
/// client_id 统一走 consumers 租户（accounts.ts msAuthBase）
fn ms_auth_base() -> &'static str {
    "https://login.microsoftonline.com/consumers/oauth2/v2.0"
}
const PROVIDER_TIMEOUT_MS: u64 = 30_000;
const MAX_METADATA_BYTES: usize = 2 * 1024 * 1024;
const MAX_ERROR_BYTES: usize = 4096;
const MAX_AGENT_BYTES: usize = 32 * 1024 * 1024;
const SECRET_KEYS: &[&str] = &["accessToken", "refreshToken", "clientToken", "loginIdentifier", "userProperties"];

// ---------------- DPAPI（对应 electron safeStorage.encryptString/decryptString） ----------------

#[repr(C)]
struct CryptBuffer {
    cb: u32,
    pb: *mut u8,
}

#[link(name = "crypt32")]
extern "system" {
    fn CryptProtectData(
        pdata_in: *const CryptBuffer,
        sz_data_descr: *const u16,
        p_optional_entropy: *const CryptBuffer,
        pv_reserved: *const core::ffi::c_void,
        p_prompt_struct: *const core::ffi::c_void,
        dw_flags: u32,
        pdata_out: *mut CryptBuffer,
    ) -> i32;
    fn CryptUnprotectData(
        pdata_in: *const CryptBuffer,
        ppsz_data_descr: *mut *mut u16,
        p_optional_entropy: *const CryptBuffer,
        pv_reserved: *const core::ffi::c_void,
        p_prompt_struct: *const core::ffi::c_void,
        dw_flags: u32,
        pdata_out: *mut CryptBuffer,
    ) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn LocalFree(hmem: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
}

const CRYPTPROTECT_UI_FORBIDDEN: u32 = 0x1;

/// DPAPI 可用性探针（对应 Electron safeStorage.isAvailable）：
/// sysinfo 的 credentialStorage 字段用它，声明统一留在这里，避免同符号在别处重复声明。
pub fn dpapi_available() -> bool {
    let plain = b"wuhuicl";
    let mut out = CryptBuffer { cb: 0, pb: core::ptr::null_mut() };
    unsafe {
        let ok = CryptProtectData(
            &CryptBuffer { cb: plain.len() as u32, pb: plain.as_ptr() as *mut u8 },
            core::ptr::null(),
            core::ptr::null(),
            core::ptr::null(),
            core::ptr::null(),
            0,
            &mut out,
        );
        if ok != 0 && !out.pb.is_null() {
            LocalFree(out.pb as *mut core::ffi::c_void);
        }
        ok != 0
    }
}

/// 固定应用熵（WUHUICL 自有密文域，不与原版 KAMUCL 互解）
fn entropy() -> Vec<u16> {
    "WUHUICL-account-secrets".encode_utf16().collect()
}

fn dpapi_protect(plain: &[u8]) -> Result<Vec<u8>, String> {
    unsafe {
        let ent = entropy();
        let mut input = CryptBuffer { cb: plain.len() as u32, pb: plain.as_ptr() as *mut u8 };
        let mut entropy_buf = CryptBuffer { cb: (ent.len() * 2) as u32, pb: ent.as_ptr() as *mut u8 };
        let mut out = CryptBuffer { cb: 0, pb: std::ptr::null_mut() };
        if CryptProtectData(
            &mut input,
            std::ptr::null(),
            &mut entropy_buf,
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out,
        ) == 0
        {
            return Err("系统安全存储当前不可用，无法安全保存登录令牌".into());
        }
        let slice = std::slice::from_raw_parts(out.pb, out.cb as usize);
        let data = slice.to_vec();
        LocalFree(out.pb as *mut _);
        Ok(data)
    }
}

fn dpapi_unprotect(blob: &[u8]) -> Option<Vec<u8>> {
    unsafe {
        let ent = entropy();
        let mut input = CryptBuffer { cb: blob.len() as u32, pb: blob.as_ptr() as *mut u8 };
        let mut entropy_buf = CryptBuffer { cb: (ent.len() * 2) as u32, pb: ent.as_ptr() as *mut u8 };
        let mut out = CryptBuffer { cb: 0, pb: std::ptr::null_mut() };
        if CryptUnprotectData(
            &mut input,
            std::ptr::null_mut(),
            &mut entropy_buf,
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out,
        ) == 0
        {
            return None;
        }
        let slice = std::slice::from_raw_parts(out.pb, out.cb as usize);
        let data = slice.to_vec();
        LocalFree(out.pb as *mut _);
        Some(data)
    }
}

fn encrypt_secret(value: &Value) -> Result<String, String> {
    let plain = if value.is_string() {
        value.as_str().unwrap_or("").to_string()
    } else {
        value.to_string()
    };
    let blob = dpapi_protect(plain.as_bytes())?;
    Ok(base64::engine::general_purpose::STANDARD.encode(blob))
}

fn decrypt_secret(value: &str) -> Option<String> {
    let blob = base64::engine::general_purpose::STANDARD.decode(value).ok()?;
    let plain = dpapi_unprotect(&blob)?;
    String::from_utf8(plain).ok()
}

// ---------------- 账号存储（与 crate::accounts 共用 accounts.json） ----------------

fn accounts_file(app: &AppHandle) -> PathBuf {
    crate::settings::user_data_dir(app).join("accounts.json")
}

/// 读盘并解开 secure 密文（每次读盘，避免与 crate::accounts 的写入互相覆盖缓存）
fn load_store(app: &AppHandle) -> Value {
    std::fs::read(accounts_file(app))
        .ok()
        .and_then(|raw| serde_json::from_slice::<Value>(&raw).ok())
        .unwrap_or_else(|| json!({ "accounts": [], "selected": null }))
}

/// 渲染进程只拿展示字段（publicAccount）
pub fn public_account(account: &Value) -> Value {
    let mut o = account.as_object().cloned().unwrap_or_default();
    for k in SECRET_KEYS {
        o.remove(*k);
    }
    Value::Object(o)
}

/// 解密后的全量账号（含凭据）
fn deserialize_account(stored: &Value) -> Value {
    let mut account = stored.as_object().cloned().unwrap_or_default();
    account.remove("secure");
    if let Some(secure) = stored.get("secure").and_then(|s| s.as_object()) {
        for key in SECRET_KEYS {
            if let Some(encrypted) = secure.get(*key).and_then(|v| v.as_str()) {
                if let Some(plain) = decrypt_secret(encrypted) {
                    if *key == "userProperties" {
                        account.insert(
                            key.to_string(),
                            serde_json::from_str::<Value>(&plain).unwrap_or_else(|_| json!([])),
                        );
                    } else {
                        account.insert(key.to_string(), json!(plain));
                    }
                }
            }
        }
    }
    Value::Object(account)
}

/// 写盘：敏感字段加密进 secure
fn persist_store(app: &AppHandle, accounts: &[Value], selected: &Value) -> Result<(), String> {
    let mut stored_accounts: Vec<Value> = Vec::new();
    for account in accounts {
        let mut stored = account.as_object().cloned().unwrap_or_default();
        let mut secure = serde_json::Map::new();
        for key in SECRET_KEYS {
            if let Some(v) = stored.remove(*key) {
                if !v.is_null() && v != json!("") {
                    secure.insert(key.to_string(), json!(encrypt_secret(&v)?));
                }
            }
        }
        if !secure.is_empty() {
            stored.insert("secure".into(), Value::Object(secure));
        }
        stored_accounts.push(Value::Object(stored));
    }
    let file = accounts_file(app);
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let payload = json!({ "accounts": stored_accounts, "selected": selected });
    let bytes = serde_json::to_vec_pretty(&payload).map_err(|e| e.to_string())?;
    std::fs::write(file, bytes).map_err(|e| e.to_string())
}

/// 全量账号列表（解密后）
pub fn all_accounts_full(app: &AppHandle) -> Vec<Value> {
    load_store(app)["accounts"]
        .as_array()
        .map(|a| a.iter().map(deserialize_account).collect())
        .unwrap_or_default()
}

/// 当前选中账号（全量，launch.rs 用）
pub fn selected_full(app: &AppHandle) -> Option<Value> {
    let data = load_store(app);
    let sel = data["selected"].as_str().unwrap_or("");
    data["accounts"]
        .as_array()?
        .iter()
        .find(|a| a["id"].as_str() == Some(sel))
        .map(deserialize_account)
}

fn account_full_by_id(app: &AppHandle, id: &str) -> Option<Value> {
    load_store(app)["accounts"]
        .as_array()?
        .iter()
        .find(|a| a["id"].as_str() == Some(id))
        .map(deserialize_account)
}

/// 插入或更新账号（按 uuid 去重，非 yggdrasil 同 uuid 合并），并设为当前选中（accounts.ts upsert）
fn upsert(app: &AppHandle, account: Value, select: bool) -> Result<Value, String> {
    let mut data = load_store(app);
    let mut accounts: Vec<Value> = data["accounts"].as_array().cloned().unwrap_or_default();
    let uuid = account["uuid"].as_str().unwrap_or("");
    let acc_type = account["type"].as_str().unwrap_or("");
    let id = account["id"].as_str().unwrap_or("");
    let idx = accounts.iter().position(|existing| {
        existing["id"].as_str() == Some(id)
            || (acc_type != "yggdrasil"
                && existing["type"].as_str() != Some("yggdrasil")
                && existing["uuid"].as_str() == Some(uuid)
                && !uuid.is_empty())
    });
    match idx {
        Some(i) => {
            let merged = {
                let mut base = accounts[i].as_object().cloned().unwrap_or_default();
                for (k, v) in account.as_object().cloned().unwrap_or_default() {
                    if !v.is_null() {
                        base.insert(k, v);
                    }
                }
                Value::Object(base)
            };
            accounts[i] = merged;
        }
        None => accounts.push(account.clone()),
    }
    if select {
        data["selected"] = json!(account["id"]);
    }
    let selected = data["selected"].clone();
    persist_store(app, &accounts, &selected)?;
    Ok(account)
}

fn has_provider_accounts(app: &AppHandle, provider_id: &str) -> bool {
    all_accounts_full(app).iter().any(|a| {
        a["type"].as_str() == Some("yggdrasil") && a["providerId"].as_str() == Some(provider_id)
    })
}

// ---------------- invoke 入口 ----------------

/// 返回 None 表示本模块不认识该通道（由路由继续分发）；Some(result) 即最终结果。
pub fn invoke(app: &AppHandle, channel: &str, args: &[Value]) -> Option<Result<Value, String>> {
    match channel {
        "accounts:msBegin" => Some(ms_begin(app)),
        "accounts:msCancel" => Some(Ok(cancel_ms_login())),
        "accounts:refresh" => Some(
            arg_str(args, 0)
                .ok_or_else(|| "缺少 id".to_string())
                .and_then(|id| refresh_account_by_id(app, &id)),
        ),
        "accounts:yggProviders" => Some(Ok(json!(list_providers(app)))),
        "accounts:yggProbe" => Some(
            args.first()
                .cloned()
                .ok_or_else(|| "缺少参数".to_string())
                .and_then(|input| probe_provider(app, &input, args.get(1).and_then(|v| v.as_bool()).unwrap_or(false))),
        ),
        "accounts:yggSaveProvider" => Some(
            args.first()
                .cloned()
                .ok_or_else(|| "缺少参数".to_string())
                .and_then(|candidate| save_provider(app, &candidate, args.get(1).and_then(|v| v.as_bool()).unwrap_or(false))),
        ),
        "accounts:yggRemoveProvider" => Some(
            arg_str(args, 0)
                .ok_or_else(|| "缺少 id".to_string())
                .and_then(|id| remove_provider(app, &id)),
        ),
        "accounts:yggLogin" => Some(ygg_login(app, args)),
        "accounts:yggSelectProfile" => Some(
            (|| {
                let challenge_id = arg_str(args, 0).ok_or("缺少 challengeId")?;
                let profile_id = arg_str(args, 1).ok_or("缺少 profileId")?;
                complete_profile_selection(app, &challenge_id, &profile_id)
            })(),
        ),
        "accounts:yggRuntime" => Some(runtime_info(app)),
        _ => None,
    }
}

fn arg_str(args: &[Value], i: usize) -> Option<String> {
    args.get(i)
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .filter(|s| !s.is_empty())
}

// ---------------- HTTP 帮助（accounts.ts postForm/postJson/getJson） ----------------

fn http_client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_millis(PROVIDER_TIMEOUT_MS))
        .connect_timeout(std::time::Duration::from_millis(15_000))
        .build()
        .unwrap_or_default()
}

fn post_form(url: &str, body: &[(&str, &str)]) -> Result<Value, String> {
    let params: Vec<(String, String)> = body.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    let resp = http_client()
        .post(url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(&params)
        .send()
        .map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    let data: Value = resp.json().unwrap_or_else(|_| json!({}));
    // OAuth 端点错误体在 200/400 都可能返回 error 字段，由调用方解读
    if status >= 400 && data.get("error").is_none() {
        return Err(format!("请求失败 HTTP {}: {}", status, url));
    }
    Ok(data)
}

fn post_json(url: &str, body: &Value) -> Result<Value, String> {
    let resp = http_client()
        .post(url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .json(body)
        .send()
        .map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    let data: Value = resp.json().unwrap_or_else(|_| json!({}));
    if status >= 400 {
        let xerr = if data.get("XErr").is_some() {
            format!(" (XErr {})", data["XErr"])
        } else {
            String::new()
        };
        return Err(format!("请求失败 HTTP {}{}: {}", status, xerr, url));
    }
    Ok(data)
}

fn get_json(url: &str, bearer: &str) -> Result<Value, String> {
    let resp = http_client()
        .get(url)
        .header("Authorization", format!("Bearer {}", bearer))
        .send()
        .map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    let data: Value = resp.json().unwrap_or_else(|_| json!({}));
    if status >= 400 {
        return Err(format!("请求失败 HTTP {}: {}", status, url));
    }
    Ok(data)
}

// ---------------- 微软登录（accounts.ts completeMsLogin） ----------------

/// 登录链路某一步失败时抛出带步骤标签的错误（accounts.ts step）
fn step(name: &str, result: Result<Value, String>) -> Result<Value, String> {
    result.map_err(|message| format!("{}：{}", name, message))
}

fn uuid_with_hyphens(id: &str) -> String {
    let mut hex: String = id.replace('-', "").to_lowercase();
    while hex.len() < 32 {
        hex.push('0');
    }
    format!("{}-{}-{}-{}-{}", &hex[0..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..32])
}

fn complete_ms_login(app: &AppHandle, ms_access_token: &str, refresh_token: &str, expires_in: u64, select: bool) -> Result<Value, String> {
    // 1. XBL 认证
    let xbl = step(
        "Xbox Live 认证",
        post_json(
            "https://user.auth.xboxlive.com/user/authenticate",
            &json!({
                "Properties": {
                    "AuthMethod": "RPS",
                    "SiteName": "user.auth.xboxlive.com",
                    "RpsTicket": format!("d={}", ms_access_token)
                },
                "RelyingParty": "http://auth.xboxlive.com",
                "TokenType": "JWT"
            }),
        ),
    )?;
    let xbl_token = xbl["Token"].as_str().unwrap_or("");
    if xbl_token.is_empty() {
        return Err("Xbox Live 认证失败：响应缺少 Token".into());
    }

    // 2. XSTS 授权
    let xsts = step(
        "XSTS 授权",
        post_json(
            "https://xsts.auth.xboxlive.com/xsts/authorize",
            &json!({
                "Properties": { "SandboxId": "RETAIL", "UserTokens": [xbl_token] },
                "RelyingParty": "rp://api.minecraftservices.com/",
                "TokenType": "JWT"
            }),
        ),
    )?;
    let xsts_token = xsts["Token"].as_str().unwrap_or("");
    let uhs = xsts
        .get("DisplayClaims")
        .and_then(|d| d.get("xui"))
        .and_then(|x| x.as_array())
        .and_then(|a| a.first())
        .and_then(|c| c.get("uhs"))
        .and_then(|u| u.as_str())
        .unwrap_or("");
    if xsts_token.is_empty() || uhs.is_empty() {
        return Err("XSTS 授权失败（该账号可能尚未创建 Xbox 档案，请先在 xbox.com 登录一次）".into());
    }

    // 3. MC 登录
    let mc = step(
        "Minecraft 登录",
        post_json(
            "https://api.minecraftservices.com/authentication/login_with_xbox",
            &json!({ "identityToken": format!("XBL3.0 x={};{}", uhs, xsts_token) }),
        ),
    )?;
    let mc_token = mc["access_token"].as_str().unwrap_or("").to_string();
    let mc_expires = mc["expires_in"].as_u64().unwrap_or(expires_in);
    if mc_token.is_empty() {
        return Err("Minecraft 登录失败：响应缺少 access_token".into());
    }

    // 4. 拥有权检查
    let entitlements = step(
        "Minecraft 拥有权检查",
        get_json("https://api.minecraftservices.com/entitlements/mcstore", &mc_token),
    )?;
    let items = entitlements["items"].as_array().cloned().unwrap_or_default();
    if !items.iter().any(|i| i["name"].as_str() == Some("product_minecraft")) {
        return Err("Minecraft 拥有权检查：该账号未拥有 Minecraft".into());
    }

    // 5. 档案
    let profile = step(
        "获取 Minecraft 档案",
        get_json("https://api.minecraftservices.com/minecraft/profile", &mc_token),
    )?;
    let pid = profile["id"].as_str().unwrap_or("");
    let pname = profile["name"].as_str().unwrap_or("");
    if pid.is_empty() || pname.is_empty() {
        return Err("获取 Minecraft 档案失败".into());
    }

    let uuid = uuid_with_hyphens(pid);
    let account = json!({
        "id": uuid,
        "type": "microsoft",
        "username": pname,
        "uuid": uuid,
        "accessToken": mc_token,
        "refreshToken": refresh_token,
        "expiresAt": chrono::Utc::now().timestamp() as u64 + mc_expires
    });
    upsert(app, account, select)
}

static POLL_CANCEL: AtomicBool = AtomicBool::new(false);

/// 可取消 sleep（100ms 切片检查取消标记）
fn sleep_cancellable(ms: u64) -> Result<(), String> {
    let mut remaining = ms;
    while remaining > 0 {
        if POLL_CANCEL.load(Ordering::SeqCst) {
            return Err("已取消".into());
        }
        let slice = remaining.min(100);
        std::thread::sleep(std::time::Duration::from_millis(slice));
        remaining -= slice;
    }
    if POLL_CANCEL.load(Ordering::SeqCst) {
        return Err("已取消".into());
    }
    Ok(())
}

/// 后台轮询 token 端点直到成功/过期/取消（accounts.ts pollDeviceCode）
fn poll_device_code(app: &AppHandle, device_code: &str, interval_sec: u64, expires_in_sec: u64) -> Result<Value, String> {
    let client_id = crate::settings::get(app)["msClientId"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let mut interval = interval_sec.max(1) * 1000;
    let deadline = chrono::Utc::now().timestamp_millis() as u64 + expires_in_sec * 1000;
    while (chrono::Utc::now().timestamp_millis() as u64) < deadline {
        sleep_cancellable(interval)?;
        let t = post_form(
            &format!("{}/token", ms_auth_base()),
            &[
                ("client_id", client_id.as_str()),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("device_code", device_code),
            ],
        )?;
        if t["access_token"].is_string() {
            return complete_ms_login(
                app,
                t["access_token"].as_str().unwrap_or(""),
                t["refresh_token"].as_str().unwrap_or(""),
                t["expires_in"].as_u64().unwrap_or(86400),
                true,
            );
        }
        match t["error"].as_str() {
            Some("authorization_pending") => continue,
            Some("slow_down") => {
                interval += 5000;
                continue;
            }
            Some(err) => {
                return Err(t["error_description"]
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| err.to_string()))
            }
            None => return Err("登录失败".into()),
        }
    }
    Err("登录超时，设备码已过期".into())
}

/// accounts:msBegin（accounts.ts beginMsDeviceCode）：返回展示信息，后台轮询，event:msLoginDone 收尾
fn ms_begin(app: &AppHandle) -> Result<Value, String> {
    // 取消上一次未完成的轮询
    let _ = cancel_ms_login();
    let client_id = crate::settings::get(app)["msClientId"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let dc = post_form(
        &format!("{}/devicecode", ms_auth_base()),
        &[("client_id", client_id.as_str()), ("scope", MS_SCOPE)],
    )
    .map_err(|e| format!("获取设备码：{}", e))?;
    let Some(device_code) = dc["device_code"].as_str().map(str::to_string) else {
        let message = dc["error_description"].as_str().unwrap_or("获取设备码失败").to_string();
        return Err(format!("获取设备码：{}", message));
    };

    POLL_CANCEL.store(false, Ordering::SeqCst);
    let app2 = app.clone();
    let device_code2 = device_code.clone();
    let interval = dc["interval"].as_u64().unwrap_or(5);
    let expires_in = dc["expires_in"].as_u64().unwrap_or(900);
    std::thread::spawn(move || {
        let result = poll_device_code(&app2, &device_code2, interval, expires_in);
        let payload = match result {
            Ok(account) => json!({ "account": public_account(&account) }),
            Err(e) => {
                if e == "已取消" {
                    json!({ "account": null })
                } else {
                    json!({ "account": null, "error": e })
                }
            }
        };
        if let Some(win) = app2.get_webview_window("main") {
            let _ = win.emit_to("main", "event:msLoginDone", payload);
        }
    });

    Ok(json!({
        "userCode": dc["user_code"].as_str().unwrap_or(""),
        "verificationUri": dc["verification_uri"].as_str().unwrap_or(""),
        "message": dc["message"].as_str().unwrap_or("")
    }))
}

/// accounts:msCancel
pub fn cancel_ms_login() -> Value {
    POLL_CANCEL.store(true, Ordering::SeqCst);
    Value::Null
}

/// accounts.ts refreshMicrosoft：refresh_token 重走 XBL→XSTS→MC 链
fn refresh_microsoft(app: &AppHandle, account: &Value) -> Result<Value, String> {
    let refresh_token = account["refreshToken"].as_str().unwrap_or("");
    if refresh_token.is_empty() {
        return Err("缺少 refresh_token，请重新登录".into());
    }
    let client_id = crate::settings::get(app)["msClientId"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let t = post_form(
        &format!("{}/token", ms_auth_base()),
        &[
            ("client_id", client_id.as_str()),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("scope", MS_SCOPE),
        ],
    )?;
    if !t["access_token"].is_string() {
        return Err(t["error_description"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| "微软令牌刷新失败，请重新登录".into()));
    }
    complete_ms_login(
        app,
        t["access_token"].as_str().unwrap_or(""),
        t["refresh_token"].as_str().unwrap_or(refresh_token),
        t["expires_in"].as_u64().unwrap_or(86400),
        false,
    )
}

/// accounts.ts getValidAccount：离线直返；微软临期（<5 分钟）自动刷新；yggdrasil 走 refreshAccount
pub fn get_valid_account(app: &AppHandle, account: Value) -> Result<Value, String> {
    match account["type"].as_str() {
        Some("offline") => Ok(account),
        Some("yggdrasil") => {
            let refreshed = refresh_yggdrasil(app, &account)?;
            let _ = upsert(app, refreshed.clone(), false);
            Ok(refreshed)
        }
        _ => {
            let now = chrono::Utc::now().timestamp() as u64;
            let has_token = !account["accessToken"].as_str().unwrap_or("").is_empty();
            let expires_at = account["expiresAt"].as_u64().unwrap_or(0);
            if !has_token || expires_at == 0 || expires_at.saturating_sub(now) < 300 {
                refresh_microsoft(app, &account)
            } else {
                Ok(account)
            }
        }
    }
}

/// accounts:refresh（accounts.ts refreshAccountById）
fn refresh_account_by_id(app: &AppHandle, id: &str) -> Result<Value, String> {
    let account = account_full_by_id(app, id).ok_or("账号不存在")?;
    let refreshed = get_valid_account(app, account)?;
    let _ = upsert(app, refreshed.clone(), false);
    Ok(public_account(&refreshed))
}

// ---------------- Yggdrasil 提供商（yggdrasilProvider.ts） ----------------

fn providers_file(app: &AppHandle) -> PathBuf {
    crate::settings::user_data_dir(app).join("yggdrasil-providers.json")
}

fn metadata_directory(app: &AppHandle) -> PathBuf {
    crate::settings::user_data_dir(app).join("yggdrasil-metadata")
}

fn injector_directory(app: &AppHandle) -> PathBuf {
    crate::settings::user_data_dir(app).join("authlib-injector")
}

static PROVIDERS_CACHE: Mutex<Option<Vec<Value>>> = Mutex::new(None);

/// yggdrasilProvider.ts normalizeYggdrasilUrl（手写 URL 归一化：协议校验 + 尾斜杠）
fn normalize_yggdrasil_url(input: &str) -> Result<String, String> {
    let mut value = input.trim().to_string();
    if value.is_empty() {
        return Err("API Root 不能为空".into());
    }
    if !regex::Regex::new(r"^[a-z][a-z\d+.-]*://")
        .unwrap()
        .is_match(&value.to_lowercase())
    {
        value = format!("https://{}", value);
    }
    // scheme://host[:port]/path?query#fragment
    let (scheme, rest) = match value.split_once("://") {
        Some((s, r)) => (s.to_lowercase(), r.to_string()),
        None => return Err("API Root 不是有效 URL".into()),
    };
    if scheme != "https" && scheme != "http" {
        return Err("认证服务仅支持 HTTPS 或 HTTP URL".into());
    }
    let (authority, path_and_more) = match rest.find(['/', '?', '#']) {
        Some(i) => (rest[..i].to_string(), rest[i..].to_string()),
        None => (rest.clone(), String::new()),
    };
    if authority.contains('@') {
        return Err("认证服务 URL 不得包含用户名或密码".into());
    }
    if path_and_more.contains('?') || path_and_more.contains('#') {
        return Err("认证服务 URL 不得包含查询参数或片段".into());
    }
    let path = path_and_more.trim_end_matches('/').to_string();
    Ok(format!("{}://{}{}/", scheme, authority, path))
}

/// yggdrasilProvider.ts providerId
fn provider_id(api_root: &str) -> Result<String, String> {
    let normalized = normalize_yggdrasil_url(api_root)?;
    let hash = Sha256::digest(normalized.as_bytes());
    Ok(format!("ygg-{}", hex::encode(&hash[..12])))
}

/// yggdrasilProvider.ts normalizeSkinDomains
fn normalize_skin_domains(values: Vec<String>) -> Vec<String> {
    let mut result: Vec<String> = Vec::new();
    for value in values {
        let mut item = value
            .trim()
            .to_lowercase()
            .trim_start_matches("https://")
            .to_string();
        let item = item.trim_start_matches("http://");
        let mut item = item.trim_end_matches('/').to_string();
        if item.starts_with("*.") {
            item = format!(".{}", &item[2..]);
        }
        if item.is_empty() || item.chars().any(|c| " /?#@".contains(c)) {
            continue;
        }
        if !result.contains(&item) {
            result.push(item);
        }
    }
    result
}

/// 提供商描述符（yggdrasilProvider.ts ParsedProviderDescriptor）
#[derive(Default, Clone)]
struct ProviderDescriptor {
    source_label: String,
    name: String,
    api_root: String,
    auth_server: String,
    account_server: String,
    session_server: String,
    services_url: String,
    skin_domains: Vec<String>,
}

/// 手写 URL 归一化（协议校验 + 尾斜杠），等价 normalizeYggdrasilUrl 的实现口径
fn normalize_url_simple(input: &str) -> Result<String, String> {
    normalize_yggdrasil_url(input)
}

/// yggdrasilProvider.ts resolveProviderEndpoints
fn resolve_provider_endpoints(descriptor: &ProviderDescriptor, resolved_api_root: &str) -> Result<Value, String> {
    let api_root = normalize_url_simple(resolved_api_root)?;
    let resolve = |value: &str, fallback: &str| -> Result<String, String> {
        let raw = if value.is_empty() { fallback } else { value };
        // 相对路径基于 apiRoot 解析
        if regex::Regex::new(r"^[a-z][a-z\d+.-]*://")
            .unwrap()
            .is_match(&raw.to_lowercase())
        {
            normalize_url_simple(raw)
        } else {
            normalize_url_simple(&format!("{}{}", api_root, raw.trim_start_matches('/')))
        }
    };
    let mut insecure_urls = vec![api_root.clone()];
    for v in [&descriptor.auth_server, &descriptor.account_server, &descriptor.session_server] {
        if !v.is_empty() {
            insecure_urls.push(v.clone());
        }
    }
    let insecure = insecure_urls.iter().any(|u| u.starts_with("http://"));
    let mut result = json!({
        "apiRoot": api_root,
        "authServer": resolve(&descriptor.auth_server, "authserver/")?,
        "accountServer": resolve(&descriptor.account_server, "api/")?,
        "sessionServer": resolve(&descriptor.session_server, "sessionserver/")?,
        "insecure": insecure
    });
    if !descriptor.services_url.is_empty() {
        result["servicesUrl"] = json!(resolve(&descriptor.services_url, "")?);
    }
    Ok(result)
}

/// yggdrasilProvider.ts endpointUrl
fn endpoint_url(base: &str, operation: &str) -> String {
    let normalized = normalize_url_simple(base).unwrap_or_else(|_| base.to_string());
    format!("{}{}", normalized, operation.trim_start_matches('/'))
}

/// yggdrasilProvider.ts parseProviderInput（kind: text | file）
fn parse_provider_input(input: &Value) -> Result<ProviderDescriptor, String> {
    let kind = input["kind"].as_str().unwrap_or("text");
    let value = input["value"].as_str().unwrap_or("");
    match kind {
        "file" => {
            let file = PathBuf::from(value);
            let stat = std::fs::metadata(&file).map_err(|_| "提供商配置文件不存在".to_string())?;
            if !stat.is_file() {
                return Err("提供商配置必须是文件".into());
            }
            if stat.len() > 1024 * 1024 {
                return Err("提供商配置超过 1 MB 限制".into());
            }
            let ext = file
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| format!(".{}", e.to_lowercase()))
                .unwrap_or_default();
            if ![".json", ".txt", ".url", ".yggdrasil"].contains(&ext.as_str()) {
                return Err("仅支持 JSON、TXT、URL 或 .yggdrasil 提供商配置".into());
            }
            let text = std::fs::read_to_string(&file).map_err(|e| e.to_string())?;
            descriptor_from_text(&text, &file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default())
        }
        _ => descriptor_from_text(value, "拖入或粘贴的文本"),
    }
}

/// 递归找第一个匹配别名的字符串（yggdrasilProvider.ts findString）
fn find_string(record: &Value, aliases: &[&str]) -> Option<String> {
    let wanted: Vec<String> = aliases
        .iter()
        .map(|a| a.to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect())
        .collect();
    if let Some(obj) = record.as_object() {
        for (key, value) in obj {
            let key_form: String = key.to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect();
            if wanted.contains(&key_form) && value.is_string() && !value.as_str().unwrap_or("").trim().is_empty() {
                return Some(value.as_str().unwrap_or("").trim().to_string());
            }
        }
        for value in obj.values() {
            if value.is_object() {
                if let Some(nested) = find_string(value, aliases) {
                    return Some(nested);
                }
            }
        }
    }
    None
}

fn find_strings(record: &Value, aliases: &[&str]) -> Vec<String> {
    let wanted: Vec<String> = aliases
        .iter()
        .map(|a| a.to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect())
        .collect();
    if let Some(obj) = record.as_object() {
        for (key, value) in obj {
            let key_form: String = key.to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect();
            if !wanted.contains(&key_form) {
                continue;
            }
            if let Some(arr) = value.as_array() {
                return arr.iter().filter_map(|v| v.as_str().map(str::to_string)).collect();
            }
            if let Some(s) = value.as_str() {
                return s
                    .split(|c: char| c == ',' || c.is_whitespace())
                    .filter(|p| !p.is_empty())
                    .map(str::to_string)
                    .collect();
            }
        }
        for value in obj.values() {
            if value.is_object() {
                let nested = find_strings(value, aliases);
                if !nested.is_empty() {
                    return nested;
                }
            }
        }
    }
    Vec::new()
}

/// yggdrasilProvider.ts descriptorFromText
fn descriptor_from_text(text: &str, source_label: &str) -> Result<ProviderDescriptor, String> {
    let trimmed = text.trim().trim_start_matches('\u{feff}').trim().to_string();
    if trimmed.is_empty() {
        return Err("拖入内容为空".into());
    }
    if trimmed.len() > 1024 * 1024 {
        return Err("提供商配置超过 1 MB 限制".into());
    }
    let dnd_prefix = "authlib-injector:yggdrasil-server:";
    if trimmed.to_lowercase().starts_with(dnd_prefix) {
        let encoded = &trimmed[dnd_prefix.len()..];
        let decoded = percent_decode(encoded).ok_or("authlib-injector 拖拽 URI 编码无效")?;
        return Ok(ProviderDescriptor {
            source_label: source_label.to_string(),
            api_root: normalize_yggdrasil_url(&decoded)?,
            ..Default::default()
        });
    }
    if trimmed.starts_with('{') {
        let data: Value = serde_json::from_str(&trimmed).map_err(|_| "提供商 JSON 格式无效".to_string())?;
        if !data.is_object() {
            return Err("提供商 JSON 顶层必须是对象".into());
        }
        let api_root = find_string(&data, &["apiRoot", "yggdrasilApiRoot", "yggdrasilServer", "apiUrl", "serverUrl"])
            .or_else(|| data["url"].as_str().map(str::to_string))
            .ok_or("提供商 JSON 中缺少 API Root")?;
        return Ok(ProviderDescriptor {
            source_label: source_label.to_string(),
            name: find_string(&data, &["providerName", "serverName", "name"]).unwrap_or_default(),
            api_root: normalize_yggdrasil_url(&api_root)?,
            auth_server: find_string(&data, &["authServer", "authenticationServer"]).unwrap_or_default(),
            account_server: find_string(&data, &["accountServer"]).unwrap_or_default(),
            session_server: find_string(&data, &["sessionServer"]).unwrap_or_default(),
            services_url: find_string(&data, &["services", "servicesUrl", "metadataUrl"]).unwrap_or_default(),
            skin_domains: normalize_skin_domains(find_strings(&data, &["skinDomains", "textureDomains"])),
        });
    }
    let shortcut = regex::Regex::new(r"(?im)^URL\s*=\s*(.+)$")
        .unwrap()
        .captures(&trimmed)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string());
    let embedded_url = regex::Regex::new(r#"https?://[^\s"'<>]+"#)
        .unwrap()
        .find(&trimmed)
        .map(|m| m.as_str().to_string());
    let labeled_root = regex::Regex::new(r"(?:api\s*root|yggdrasil(?:\s*server)?)\s*[:=]\s*([^\s]+)")
        .unwrap()
        .captures(&trimmed)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string());
    let value = shortcut
        .or(embedded_url)
        .or(labeled_root)
        .unwrap_or_else(|| trimmed.split_whitespace().next().unwrap_or("").to_string());
    Ok(ProviderDescriptor {
        source_label: source_label.to_string(),
        api_root: normalize_yggdrasil_url(&value)?,
        ..Default::default()
    })
}

fn percent_decode(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16)?;
            let lo = (bytes[i + 2] as char).to_digit(16)?;
            out.push(((hi << 4) | lo) as u8);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// yggdrasil.ts cleanProvider
fn clean_provider(value: &Value) -> Option<Value> {
    if !value.is_object() {
        return None;
    }
    let api_root = match normalize_yggdrasil_url(value["apiRoot"].as_str().unwrap_or("")) {
        Ok(u) => u,
        Err(_) => return None,
    };
    let mut descriptor = ProviderDescriptor {
        source_label: "已保存配置".into(),
        api_root: api_root.clone(),
        auth_server: value["authServer"].as_str().unwrap_or("").to_string(),
        account_server: value["accountServer"].as_str().unwrap_or("").to_string(),
        session_server: value["sessionServer"].as_str().unwrap_or("").to_string(),
        services_url: value["servicesUrl"].as_str().unwrap_or("").to_string(),
        skin_domains: normalize_skin_domains(
            value["skinDomains"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default(),
        ),
        name: String::new(),
    };
    descriptor.name = value["name"].as_str().unwrap_or("").trim().to_string();
    let endpoints = resolve_provider_endpoints(&descriptor, &api_root).ok()?;
    let name = if descriptor.name.is_empty() {
        hostname_of(&api_root)
    } else {
        descriptor.name.clone()
    };
    let mut out = endpoints;
    out["id"] = json!(provider_id(&api_root).ok()?);
    out["name"] = json!(name);
    out["skinDomains"] = json!(descriptor.skin_domains);
    out["metadataFetchedAt"] = json!(value["metadataFetchedAt"].as_str().map(str::to_string).unwrap_or_else(|| "1970-01-01T00:00:00Z".into()));
    Some(out)
}

fn hostname_of(url: &str) -> String {
    let re = regex::Regex::new(r"^[a-z][a-z\d+.-]*://([^/:?#]+)").unwrap();
    re.captures(url)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
        .unwrap_or_else(|| url.to_string())
}

/// yggdrasil.ts assertSecure
fn assert_secure(urls: &[&str], allow_insecure: bool) -> Result<(), String> {
    let insecure = urls
        .iter()
        .any(|u| normalize_yggdrasil_url(u).map(|n| n.starts_with("http:")).unwrap_or(false));
    if insecure && !allow_insecure {
        return Err("INSECURE_YGGDRASIL:该认证服务使用明文 HTTP，账号和密码可能被窃听。勾选风险确认后才能继续。".into());
    }
    Ok(())
}

/// 限量读取响应体（yggdrasil.ts responseBufferLimited）
fn read_limited(mut resp: reqwest::blocking::Response, max_bytes: usize) -> Result<Vec<u8>, String> {
    if let Some(len) = resp.content_length() {
        if len as usize > max_bytes {
            return Err(format!("认证服务响应超过 {} KB 限制", max_bytes / 1024));
        }
    }
    use std::io::Read;
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 16384];
    loop {
        match resp.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if buf.len() > max_bytes {
                    return Err(format!("认证服务响应超过 {} KB 限制", max_bytes / 1024));
                }
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(buf)
}

fn read_text_limited(resp: reqwest::blocking::Response, max_bytes: usize) -> Result<String, String> {
    let bytes = read_limited(resp, max_bytes)?;
    Ok(String::from_utf8_lossy(&bytes).trim_start_matches('\u{feff}').to_string())
}

fn parse_metadata(raw: &str) -> Result<Value, String> {
    let data: Value = serde_json::from_str(raw).map_err(|_| "认证服务返回的元数据不是有效 JSON".to_string())?;
    if !data.is_object() {
        return Err("认证服务元数据顶层必须是对象".into());
    }
    Ok(data)
}

/// yggdrasil.ts fetchMetadataAt：x-authlib-injector-api-location 跟随
fn fetch_metadata_at(input_url: &str, allow_insecure: bool) -> Result<(String, String, Value, bool), String> {
    let initial = normalize_yggdrasil_url(input_url)?;
    assert_secure(&[&initial], allow_insecure)?;
    let first = http_client()
        .get(&initial)
        .header("Accept", "application/json")
        .send()
        .map_err(|e| e.to_string())?;
    if !first.status().is_success() {
        return Err(format!("认证服务元数据请求失败（HTTP {}）", first.status().as_u16()));
    }
    let first_url = normalize_yggdrasil_url(first.url().as_str())?;
    assert_secure(&[&first_url], allow_insecure)?;
    let ali = first
        .headers()
        .get("x-authlib-injector-api-location")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let resolved = match &ali {
        Some(loc) => resolve_against(&first_url, loc)?,
        None => first_url.clone(),
    };
    assert_secure(&[&resolved], allow_insecure)?;
    let ali_redirected = resolved != first_url;
    let (final_root, raw) = if ali_redirected {
        let response = http_client()
            .get(&resolved)
            .header("Accept", "application/json")
            .send()
            .map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            return Err(format!("认证服务 API Root 请求失败（HTTP {}）", response.status().as_u16()));
        }
        let final_root = normalize_yggdrasil_url(response.url().as_str())?;
        assert_secure(&[&final_root], allow_insecure)?;
        let raw = read_text_limited(response, MAX_METADATA_BYTES)?;
        (final_root, raw)
    } else {
        let final_root = normalize_yggdrasil_url(first.url().as_str())?;
        assert_secure(&[&final_root], allow_insecure)?;
        let raw = read_text_limited(first, MAX_METADATA_BYTES)?;
        (final_root, raw)
    };
    let data = parse_metadata(&raw)?;
    Ok((final_root, raw, data, ali_redirected))
}

/// 相对地址基于 base 解析（authlib-injector ALI 头）
fn resolve_against(base: &str, location: &str) -> Result<String, String> {
    if regex::Regex::new(r"^[a-z][a-z\d+.-]*://")
        .unwrap()
        .is_match(&location.to_lowercase())
    {
        return normalize_yggdrasil_url(location);
    }
    if location.starts_with('/') {
        let host_end = base.find("://").map(|i| i + 3).unwrap_or(0);
        let path_start = base[host_end..].find('/').map(|i| host_end + i).unwrap_or(base.len());
        return normalize_yggdrasil_url(&format!("{}{}", &base[..path_start], location));
    }
    normalize_yggdrasil_url(&format!("{}{}", base, location))
}

fn metadata_name(data: &Value) -> Option<String> {
    data["meta"]["serverName"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn cache_metadata(app: &AppHandle, id: &str, raw: &str) {
    let dir = metadata_directory(app);
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join(format!("{}.json", id)), raw);
}

/// yggdrasil.ts probeProvider
fn probe_provider(app: &AppHandle, input: &Value, allow_insecure: bool) -> Result<Value, String> {
    let _ = app;
    let descriptor = parse_provider_input(input)?;
    {
        let mut urls: Vec<String> = vec![descriptor.api_root.clone()];
        for v in [&descriptor.auth_server, &descriptor.account_server, &descriptor.session_server, &descriptor.services_url] {
            if !v.is_empty() {
                urls.push(v.clone());
            }
        }
        let refs: Vec<&str> = urls.iter().map(String::as_str).collect();
        assert_secure(&refs, allow_insecure)?;
    }
    let (api_root, raw, data, ali_redirected) = fetch_metadata_at(&descriptor.api_root, allow_insecure)?;
    let endpoints = resolve_provider_endpoints(&descriptor, &api_root)?;
    {
        let check: Vec<String> = ["apiRoot", "authServer", "accountServer", "sessionServer"]
            .iter()
            .filter_map(|k| endpoints[k].as_str().map(str::to_string))
            .collect();
        let refs: Vec<&str> = check.iter().map(String::as_str).collect();
        assert_secure(&refs, allow_insecure)?;
    }
    let id = provider_id(endpoints["apiRoot"].as_str().unwrap_or(""))?;
    let mut skin_domains = descriptor.skin_domains.clone();
    skin_domains.extend(
        data["skinDomains"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect::<Vec<String>>()
            })
            .unwrap_or_default(),
    );
    let skin_domains = normalize_skin_domains(skin_domains);
    with_probed_metadata(|m| m.insert(id.clone(), raw));
    let mut candidate = endpoints;
    candidate["id"] = json!(id);
    candidate["name"] = json!(if descriptor.name.is_empty() {
        metadata_name(&data).unwrap_or_else(|| hostname_of(candidate["apiRoot"].as_str().unwrap_or("")))
    } else {
        descriptor.name.clone()
    });
    candidate["skinDomains"] = json!(skin_domains);
    candidate["metadataFetchedAt"] = json!(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
    candidate["sourceLabel"] = json!(descriptor.source_label);
    candidate["aliRedirected"] = json!(ali_redirected);
    Ok(candidate)
}

static PROBED_METADATA: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

fn with_probed_metadata<T>(f: impl FnOnce(&mut HashMap<String, String>) -> T) -> T {
    let mut guard = PROBED_METADATA.lock().unwrap();
    f(guard.get_or_insert_with(HashMap::new))
}

/// yggdrasil.ts listProviders
pub fn list_providers(app: &AppHandle) -> Vec<Value> {
    {
        let cache = PROVIDERS_CACHE.lock().unwrap();
        if let Some(list) = cache.as_ref() {
            return list.clone();
        }
    }
    let list: Vec<Value> = std::fs::read(providers_file(app))
        .ok()
        .and_then(|raw| serde_json::from_slice::<Value>(&raw).ok())
        .and_then(|raw| raw.as_array().cloned())
        .map(|arr| arr.iter().filter_map(clean_provider).collect())
        .unwrap_or_default();
    *PROVIDERS_CACHE.lock().unwrap() = Some(list.clone());
    list
}

fn persist_providers(app: &AppHandle, providers: &[Value]) {
    let file = providers_file(app);
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string_pretty(providers) {
        let _ = std::fs::write(file, text);
    }
    *PROVIDERS_CACHE.lock().unwrap() = Some(providers.to_vec());
}

fn find_provider(app: &AppHandle, id: &str) -> Result<Value, String> {
    list_providers(app)
        .iter()
        .find(|p| p["id"].as_str() == Some(id))
        .cloned()
        .ok_or("外置登录提供商已被删除，请重新添加".to_string())
}

/// yggdrasil.ts saveProvider
fn save_provider(app: &AppHandle, candidate: &Value, allow_insecure: bool) -> Result<Value, String> {
    let cleaned = clean_provider(candidate).ok_or("提供商配置无效，请重新探测")?;
    if cleaned["id"] != candidate["id"] {
        return Err("提供商 API Root 与探测结果不一致".into());
    }
    {
        let refs: Vec<String> = ["apiRoot", "authServer", "accountServer", "sessionServer"]
            .iter()
            .filter_map(|k| cleaned[k].as_str().map(str::to_string))
            .collect();
        let refs: Vec<&str> = refs.iter().map(String::as_str).collect();
        assert_secure(&refs, allow_insecure)?;
    }
    let mut list = list_providers(app);
    let id = cleaned["id"].as_str().unwrap_or("").to_string();
    match list.iter().position(|p| p["id"].as_str() == Some(id.as_str())) {
        Some(i) => list[i] = cleaned,
        None => list.push(cleaned),
    }
    persist_providers(app, &list);
    let metadata = with_probed_metadata(|m| m.get(&id).cloned());
    if let Some(metadata) = metadata {
        cache_metadata(app, &id, &metadata);
    }
    Ok(json!(list_providers(app)))
}

/// yggdrasil.ts removeProvider
fn remove_provider(app: &AppHandle, id: &str) -> Result<Value, String> {
    if has_provider_accounts(app, id) {
        return Err("该提供商仍有关联账号，请先退出并删除这些账号".into());
    }
    let list = list_providers(app);
    if !list.iter().any(|p| p["id"].as_str() == Some(id)) {
        return Err("提供商不存在".into());
    }
    let remaining: Vec<Value> = list.into_iter().filter(|p| p["id"].as_str() != Some(id)).collect();
    persist_providers(app, &remaining);
    let _ = std::fs::remove_file(metadata_directory(app).join(format!("{}.json", id)));
    Ok(json!(list_providers(app)))
}

// ---------------- Yggdrasil 登录（yggdrasil.ts authenticate 等） ----------------

fn read_ygg_error(status: u16, raw: &str, fallback: &str) -> String {
    let detail = serde_json::from_str::<Value>(raw)
        .map(|d| {
            d["errorMessage"]
                .as_str()
                .or_else(|| d["cause"].as_str())
                .or_else(|| d["error"].as_str())
                .unwrap_or("")
                .to_string()
        })
        .unwrap_or_else(|_| {
            regex::Regex::new(r"<[^>]+>")
                .unwrap()
                .replace_all(raw, " ")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        });
    if status == 403 {
        if detail.is_empty() {
            "账号或密码错误，或账号尚未激活".into()
        } else {
            detail
        }
    } else if status == 429 {
        "认证请求过于频繁，请稍后再试".into()
    } else if detail.is_empty() {
        format!("{}（HTTP {}）", fallback, status)
    } else {
        detail
    }
}

fn post_ygg(url: &str, body: &Value) -> Result<Value, String> {
    let response = http_client()
        .post(url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .json(body)
        .send()
        .map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    if !response.status().is_success() {
        let raw = read_text_limited(response, MAX_ERROR_BYTES).unwrap_or_default();
        return Err(read_ygg_error(status, &raw, "认证服务请求失败"));
    }
    if status == 204 {
        return Ok(json!({}));
    }
    let raw = read_text_limited(response, MAX_METADATA_BYTES)?;
    serde_json::from_str(&raw).map_err(|_| "认证服务返回了无效 JSON".to_string())
}

fn clean_profile_choice(value: &Value) -> Option<Value> {
    let id = value["id"].as_str().unwrap_or("").replace('-', "").to_lowercase();
    let name = value["name"].as_str().unwrap_or("").trim().to_string();
    if !regex::Regex::new(r"^[0-9a-f]{32}$").unwrap().is_match(&id) || name.is_empty() {
        return None;
    }
    Some(json!({ "id": id, "name": name }))
}

fn uuid_with_hyphens_strict(value: &str) -> Result<String, String> {
    let id = value.replace('-', "").to_lowercase();
    if !regex::Regex::new(r"^[0-9a-f]{32}$").unwrap().is_match(&id) {
        return Err("认证服务返回了无效角色 UUID".into());
    }
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &id[0..8],
        &id[8..12],
        &id[12..16],
        &id[16..20],
        &id[20..32]
    ))
}

fn clean_properties(value: Option<&Value>) -> Value {
    let mut out: Vec<Value> = Vec::new();
    if let Some(list) = value.and_then(|v| v.as_array()) {
        for item in list {
            let name = item["name"].as_str().unwrap_or("");
            let value = item["value"].as_str().unwrap_or("");
            if !name.is_empty() {
                out.push(json!({ "name": name, "value": value }));
            }
        }
    }
    json!(out)
}

fn account_from_response(provider: &Value, identifier: &str, response: &Value, fallback_client_token: &str) -> Result<Value, String> {
    let profile = response
        .get("selectedProfile")
        .and_then(clean_profile_choice)
        .ok_or("认证服务未返回已选择的角色")?;
    let access_token = response["accessToken"].as_str().unwrap_or("").to_string();
    let client_token = response["clientToken"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| fallback_client_token.to_string());
    if access_token.is_empty() || client_token.is_empty() {
        return Err("认证服务未返回完整令牌".into());
    }
    let uuid = uuid_with_hyphens_strict(profile["id"].as_str().unwrap_or(""))?;
    let id = format!(
        "ygg-{}",
        &hex::encode(Sha256::digest(
            format!(
                "{}\u{0}{}\u{0}{}",
                provider["id"].as_str().unwrap_or(""),
                identifier,
                profile["id"].as_str().unwrap_or("")
            )
            .as_bytes()
        ))[..16]
    );
    let mut account = json!({
        "id": id,
        "type": "yggdrasil",
        "username": profile["name"],
        "uuid": uuid,
        "accessToken": access_token,
        "clientToken": client_token,
        "providerId": provider["id"],
        "providerName": provider["name"],
        "apiRoot": provider["apiRoot"],
        "loginIdentifier": identifier,
        "userProperties": clean_properties(response.get("user"))
    });
    if let Some(user_id) = response["user"]["id"].as_str() {
        account["userId"] = json!(user_id);
    }
    Ok(account)
}

fn select_single_profile(provider: &Value, identifier: &str, access_token: &str, client_token: &str, profile: &Value, user: Option<&Value>) -> Result<Value, String> {
    let body = json!({
        "accessToken": access_token,
        "clientToken": client_token,
        "selectedProfile": profile,
        "requestUser": true
    });
    let mut response = post_ygg(&endpoint_url(provider["authServer"].as_str().unwrap_or(""), "refresh"), &body)?;
    if response.get("user").is_none() {
        if let Some(u) = user {
            response["user"] = u.clone();
        }
    }
    account_from_response(provider, identifier, &response, client_token)
}

/// 多角色挑战暂存（challengeId → pending，5 分钟过期）
struct PendingProfile {
    provider: Value,
    identifier: String,
    access_token: String,
    client_token: String,
    profiles: Vec<Value>,
    user: Option<Value>,
    expires_at: u64,
}

static PENDING_PROFILES: Mutex<Option<HashMap<String, PendingProfile>>> = Mutex::new(None);

fn with_pending_profiles<T>(f: impl FnOnce(&mut HashMap<String, PendingProfile>) -> T) -> T {
    let mut guard = PENDING_PROFILES.lock().unwrap();
    f(guard.get_or_insert_with(HashMap::new))
}

/// accounts:yggLogin（yggdrasil.ts authenticate）
fn ygg_login(app: &AppHandle, args: &[Value]) -> Result<Value, String> {
    let provider_id_value = arg_str(args, 0).ok_or("缺少 providerId")?;
    let identifier_value = arg_str(args, 1).unwrap_or_default();
    let password_value = args
        .get(2)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_default();
    let provider = find_provider(app, &provider_id_value)?;
    let identifier = identifier_value.trim().to_string();
    if identifier.is_empty() {
        return Err("账号或邮箱不能为空".into());
    }
    if password_value.is_empty() {
        return Err("密码不能为空".into());
    }
    let client_token = crate::accounts::uuid_v4().replace('-', "");
    let response = post_ygg(
        &endpoint_url(provider["authServer"].as_str().unwrap_or(""), "authenticate"),
        &json!({
            "agent": { "name": "Minecraft", "version": 1 },
            "username": identifier,
            "password": password_value,
            "clientToken": client_token,
            "requestUser": true
        }),
    )?;
    let access_token = response["accessToken"].as_str().unwrap_or("").to_string();
    let returned_client_token = response["clientToken"].as_str().unwrap_or(&client_token).to_string();
    if access_token.is_empty() {
        return Err("认证服务未返回 accessToken".into());
    }
    if response.get("selectedProfile").is_some() {
        let account = account_from_response(&provider, &identifier, &response, &returned_client_token)?;
        let saved = save_yggdrasil_account(app, account)?;
        return Ok(json!({ "status": "complete", "account": saved }));
    }
    let profiles: Vec<Value> = response["availableProfiles"]
        .as_array()
        .map(|a| a.iter().filter_map(clean_profile_choice).collect())
        .unwrap_or_default();
    if profiles.is_empty() {
        return Err("此账号没有可用角色，请先在皮肤站创建角色".into());
    }
    if profiles.len() == 1 {
        let account = select_single_profile(
            &provider,
            &identifier,
            &access_token,
            &returned_client_token,
            &profiles[0],
            response.get("user"),
        )?;
        let saved = save_yggdrasil_account(app, account)?;
        return Ok(json!({ "status": "complete", "account": saved }));
    }
    let challenge_id = crate::accounts::uuid_v4();
    let expires_at = chrono::Utc::now().timestamp_millis() as u64 + 5 * 60 * 1000;
    let provider_name = provider["name"].clone();
    with_pending_profiles(|m| {
        m.insert(
            challenge_id.clone(),
            PendingProfile {
                provider,
                identifier,
                access_token,
                client_token: returned_client_token,
                profiles: profiles.clone(),
                user: response.get("user").cloned(),
                expires_at,
            },
        );
    });
    Ok(json!({
        "status": "select-profile",
        "challengeId": challenge_id,
        "providerName": provider_name,
        "profiles": profiles,
        "expiresAt": expires_at
    }))
}

/// accounts:yggSelectProfile（yggdrasil.ts completeProfileSelection）
fn complete_profile_selection(app: &AppHandle, challenge_id: &str, profile_id_value: &str) -> Result<Value, String> {
    let pending = with_pending_profiles(|m| m.remove(challenge_id));
    let Some(pending) = pending else {
        return Err("角色选择已过期，请重新登录".into());
    };
    if pending.expires_at < chrono::Utc::now().timestamp_millis() as u64 {
        return Err("角色选择已过期，请重新登录".into());
    }
    let profile_id = profile_id_value.replace('-', "").to_lowercase();
    let profile = pending
        .profiles
        .iter()
        .find(|p| p["id"].as_str() == Some(profile_id.as_str()))
        .ok_or("所选角色不在本次登录响应中")?;
    let account = select_single_profile(
        &pending.provider,
        &pending.identifier,
        &pending.access_token,
        &pending.client_token,
        profile,
        pending.user.as_ref(),
    )?;
    save_yggdrasil_account(app, account)
}

fn validate_credentials(provider: &Value, access_token: &str, client_token: &str) -> bool {
    let response = http_client()
        .post(endpoint_url(provider["authServer"].as_str().unwrap_or(""), "validate"))
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .json(&json!({ "accessToken": access_token, "clientToken": client_token }))
        .send();
    matches!(response, Ok(r) if r.status().as_u16() == 204)
}

/// yggdrasil.ts refreshAccount（accounts:refresh 对 yggdrasil 账号的分支）
pub fn refresh_yggdrasil(app: &AppHandle, account: &Value) -> Result<Value, String> {
    if account["type"].as_str() != Some("yggdrasil") {
        return Err("不是外置登录账号".into());
    }
    let provider_id_value = account["providerId"].as_str().unwrap_or("").to_string();
    if provider_id_value.is_empty() {
        return Err("不是外置登录账号".into());
    }
    let provider = find_provider(app, &provider_id_value)?;
    let access_token = account["accessToken"].as_str().unwrap_or("");
    let client_token = account["clientToken"].as_str().unwrap_or("");
    if validate_credentials(&provider, access_token, client_token) {
        return Ok(account.clone());
    }
    if access_token.is_empty() || client_token.is_empty() {
        return Err("外置登录凭据缺失，请重新登录".into());
    }
    let response = post_ygg(
        &endpoint_url(provider["authServer"].as_str().unwrap_or(""), "refresh"),
        &json!({ "accessToken": access_token, "clientToken": client_token, "requestUser": true }),
    )
    .map_err(|e| format!("外置登录已失效，请重新输入密码：{}", e))?;
    let refreshed = account_from_response(
        &provider,
        account["loginIdentifier"].as_str().unwrap_or_else(|| account["username"].as_str().unwrap_or("")),
        &response,
        client_token,
    )?;
    if refreshed["uuid"] != account["uuid"] {
        return Err("刷新响应的角色与原账号不一致，请重新登录".into());
    }
    // 保留原 id
    let mut merged = refreshed;
    merged["id"] = account["id"].clone();
    Ok(merged)
}

/// yggdrasil.ts invalidateAccount（删除 yggdrasil 账号时远端失效）
pub fn invalidate_yggdrasil(app: &AppHandle, account: &Value) {
    if account["type"].as_str() != Some("yggdrasil") {
        return;
    }
    let Ok(provider) = find_provider(app, account["providerId"].as_str().unwrap_or("")) else {
        return;
    };
    let access_token = account["accessToken"].as_str().unwrap_or("");
    let client_token = account["clientToken"].as_str().unwrap_or("");
    if access_token.is_empty() || client_token.is_empty() {
        return;
    }
    let _ = post_ygg(
        &endpoint_url(provider["authServer"].as_str().unwrap_or(""), "invalidate"),
        &json!({ "accessToken": access_token, "clientToken": client_token }),
    );
}

// ---------------- authlib-injector 运行时（yggdrasil.ts downloadInjector 等） ----------------

fn sha256_file(path: &PathBuf) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    Ok(hex::encode(Sha256::digest(&bytes)))
}

fn clean_artifact(value: &Value) -> Option<Value> {
    if !value.is_object() {
        return None;
    }
    let sha256 = value["checksums"]["sha256"].as_str().unwrap_or("").to_lowercase();
    let download_url = value["download_url"].as_str().unwrap_or("");
    if !value["build_number"].is_number()
        || value["version"].as_str().unwrap_or("").trim().is_empty()
        || !regex::Regex::new(r"^[0-9a-f]{64}$").unwrap().is_match(&sha256)
        || !download_url.starts_with("https://")
    {
        return None;
    }
    let host = hostname_of(download_url);
    if !["authlib-injector.yushi.moe", "bmclapi2.bangbang93.com"].contains(&host.as_str()) {
        return None;
    }
    Some(json!({
        "build_number": value["build_number"],
        "version": value["version"],
        "download_url": download_url,
        "checksums": { "sha256": sha256 }
    }))
}

fn artifact_manifest_file(app: &AppHandle) -> PathBuf {
    injector_directory(app).join("current.json")
}

fn artifact_jar_path(app: &AppHandle, artifact: &Value) -> PathBuf {
    let version: String = artifact["version"]
        .as_str()
        .unwrap_or("")
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || "._-".contains(c) { c } else { '_' })
        .collect();
    injector_directory(app).join(format!("authlib-injector-{}-{}.jar", version, artifact["build_number"].as_i64().unwrap_or(0)))
}

fn valid_cached_artifact(app: &AppHandle) -> Option<String> {
    let manifest: Value = serde_json::from_slice(&std::fs::read(artifact_manifest_file(app)).ok()?).ok()?;
    let artifact = clean_artifact(&manifest)?;
    let jar = artifact_jar_path(app, &artifact);
    if !jar.exists() || sha256_file(&jar).ok()? != artifact["checksums"]["sha256"].as_str().unwrap_or("") {
        return None;
    }
    Some(jar.to_string_lossy().to_string())
}

fn fetch_injector_artifact(app: &AppHandle) -> Result<Value, String> {
    let official = "https://authlib-injector.yushi.moe/artifact/latest.json";
    let bmcl = "https://bmclapi2.bangbang93.com/mirrors/authlib-injector/artifact/latest.json";
    let sources: Vec<&str> = if crate::settings::get(app)["mirror"].as_str() == Some("bmclapi") {
        vec![bmcl, official]
    } else {
        vec![official, bmcl]
    };
    let mut errors: Vec<String> = Vec::new();
    for source in sources {
        let host = hostname_of(source);
        match http_client()
            .get(source)
            .header("Accept", "application/json")
            .send()
        {
            Ok(response) => {
                let status = response.status().as_u16();
                if !response.status().is_success() {
                    errors.push(format!("{}: HTTP {}", host, status));
                    continue;
                }
                let raw = read_text_limited(response, 128 * 1024)?;
                match serde_json::from_str::<Value>(&raw).map_err(|e| e.to_string()).and_then(|j| {
                    clean_artifact(&j).ok_or_else(|| "构件元数据缺少可信下载地址或 SHA-256".to_string())
                }) {
                    Ok(artifact) => return Ok(artifact),
                    Err(e) => errors.push(format!("{}: {}", host, e)),
                }
            }
            Err(e) => errors.push(format!("{}: {}", host, e)),
        }
    }
    Err(format!("无法获取 authlib-injector：{}", errors.join("；")))
}

fn download_injector(app: &AppHandle) -> Result<String, String> {
    if let Some(cached) = valid_cached_artifact(app) {
        return Ok(cached);
    }
    let artifact = fetch_injector_artifact(app)?;
    let dir = injector_directory(app);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let jar = artifact_jar_path(app, &artifact);
    if jar.exists() && sha256_file(&jar).ok().as_deref() == artifact["checksums"]["sha256"].as_str() {
        let _ = std::fs::write(artifact_manifest_file(app), serde_json::to_vec_pretty(&artifact).unwrap_or_default());
        return Ok(jar.to_string_lossy().to_string());
    }
    let response = http_client()
        .get(artifact["download_url"].as_str().unwrap_or(""))
        .send()
        .map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("authlib-injector 下载失败（HTTP {}）", response.status().as_u16()));
    }
    let final_url = response.url().as_str().to_string();
    if !final_url.starts_with("https://")
        || !["authlib-injector.yushi.moe", "bmclapi2.bangbang93.com"].contains(&hostname_of(&final_url).as_str())
    {
        return Err("authlib-injector 下载发生了不可信重定向".into());
    }
    let bytes = read_limited(response, MAX_AGENT_BYTES)?;
    if bytes.is_empty() {
        return Err("authlib-injector 文件大小异常".into());
    }
    let actual = hex::encode(Sha256::digest(&bytes));
    if actual != artifact["checksums"]["sha256"].as_str().unwrap_or("") {
        return Err("authlib-injector SHA-256 校验失败".into());
    }
    let temp = format!("{}.part", jar.to_string_lossy());
    std::fs::write(&temp, &bytes).map_err(|e| e.to_string())?;
    std::fs::copy(&temp, &jar).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&temp);
    let _ = std::fs::write(artifact_manifest_file(app), serde_json::to_vec_pretty(&artifact).unwrap_or_default());
    Ok(jar.to_string_lossy().to_string())
}

/// accounts:yggRuntime（yggdrasil.ts runtimeInfo）
fn runtime_info(app: &AppHandle) -> Result<Value, String> {
    let jar = download_injector(app)?;
    let manifest: Value = serde_json::from_slice(&std::fs::read(artifact_manifest_file(app)).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let artifact = clean_artifact(&manifest).ok_or("authlib-injector 本地构件清单损坏")?;
    let actual = sha256_file(&PathBuf::from(&jar))?;
    if actual != artifact["checksums"]["sha256"].as_str().unwrap_or("") {
        return Err("authlib-injector 本地 SHA-256 校验失败".into());
    }
    Ok(json!({
        "path": jar,
        "version": artifact["version"],
        "buildNumber": artifact["build_number"],
        "sha256": actual
    }))
}

/// yggdrasil.ts providerMetadata：带缓存回落（返回原文，prefetched base64 保持一致）
fn provider_metadata(app: &AppHandle, provider: &Value) -> Result<String, String> {
    let insecure = provider["insecure"].as_bool().unwrap_or(false);
    match fetch_metadata_at(provider["apiRoot"].as_str().unwrap_or(""), insecure) {
        Ok((_, raw, _, _)) => {
            cache_metadata(app, provider["id"].as_str().unwrap_or(""), &raw);
            Ok(raw)
        }
        Err(e) => {
            let cached = std::fs::read(metadata_directory(app).join(format!("{}.json", provider["id"].as_str().unwrap_or(""))))
                .ok()
                .and_then(|raw| String::from_utf8(raw).ok());
            match cached {
                Some(cached) if parse_metadata(&cached).is_ok() => Ok(cached),
                _ => Err(format!(
                    "无法获取 {} 元数据，且没有可用缓存：{}",
                    provider["name"].as_str().unwrap_or(""),
                    e
                )),
            }
        }
    }
}

/// yggdrasilProvider.ts buildAuthlibInjectorArguments
fn build_authlib_injector_arguments(jar_path: &str, api_root: &str, metadata: &str) -> Result<Vec<String>, String> {
    if jar_path.trim().is_empty() {
        return Err("authlib-injector 路径为空".into());
    }
    let root = normalize_yggdrasil_url(api_root)?;
    if serde_json::from_str::<Value>(metadata).map(|v| v.is_object()).unwrap_or(false) {
        // 元数据有效
    } else {
        return Err("authlib-injector 预取元数据无效".into());
    }
    Ok(vec![
        format!("-javaagent:{}={}", jar_path, root),
        format!(
            "-Dauthlibinjector.yggdrasil.prefetched={}",
            base64::engine::general_purpose::STANDARD.encode(metadata.as_bytes())
        ),
    ])
}

/// launch.rs 外置登录启动参数（yggdrasil.ts launchArguments）
pub fn launch_arguments(app: &AppHandle, account: &Value) -> Result<Vec<String>, String> {
    if account["type"].as_str() != Some("yggdrasil") {
        return Ok(Vec::new());
    }
    let provider_id_value = account["providerId"].as_str().unwrap_or("");
    if provider_id_value.is_empty() {
        return Ok(Vec::new());
    }
    let provider = find_provider(app, provider_id_value)?;
    let jar = download_injector(app)?;
    let metadata = provider_metadata(app, &provider)?;
    build_authlib_injector_arguments(&jar, provider["apiRoot"].as_str().unwrap_or(""), &metadata)
}

// ---------------- accounts.rs 协作入口 ----------------

/// yggdrasil 登录完成后入库（accounts.ts saveYggdrasilAccount：upsert + public）
pub fn save_yggdrasil_account(app: &AppHandle, account: Value) -> Result<Value, String> {
    if account["type"].as_str() != Some("yggdrasil") {
        return Err("无效的外置登录账号".into());
    }
    upsert(app, account, true).map(|a| public_account(&a))
}

// ---------------- skins.rs 用的外置档案（对应 yggdrasil.ts externalProfile） ----------------

/// 外置账号 → ProfileSkins（session server 纹理属性解析）
pub fn external_profile_public(app: &tauri::AppHandle, account: &Value) -> Result<Value, String> {
    if account["type"].as_str() != Some("yggdrasil") || account["providerId"].as_str().is_none() {
        return Err("不是外置登录账号".into());
    }
    let provider_id = account["providerId"].as_str().unwrap_or("");
    let providers = list_providers(app);
    let provider = providers
        .iter()
        .find(|p| p["id"].as_str() == Some(provider_id))
        .cloned()
        .ok_or("登录账户提供方不存在")?;
    let session_server = provider["sessionServer"].as_str().unwrap_or("").to_string();
    if session_server.is_empty() {
        return Err("提供方会话服务地址无效".into());
    }
    let base = session_server.trim_end_matches('/');
    let profile_id = account["uuid"].as_str().unwrap_or("").replace('-', "");
    let url = format!("{base}/session/minecraft/profile/{profile_id}?unsigned=false");
    let resp = crate::net::client()
        .get(&url)
        .header("Accept", "application/json")
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("获取外置角色材质失败（HTTP {}）", resp.status().as_u16()));
    }
    let data: Value = resp.json().map_err(|e| e.to_string())?;
    let texture_prop = data["properties"]
        .as_array()
        .and_then(|ps| ps.iter().find(|p| p["name"].as_str() == Some("textures")))
        .and_then(|p| p["value"].as_str())
        .map(str::to_string);
    let mut payload = json!({});
    if let Some(v) = texture_prop {
        use base64::Engine;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(v)
            .map_err(|_| "外置角色的 textures 属性无效".to_string())?;
        payload = serde_json::from_slice(&decoded).map_err(|_| "外置角色的 textures 属性无效".to_string())?;
    }
    let skin_url = payload["textures"]["SKIN"]["url"].as_str().map(str::to_string);
    let cape_url = payload["textures"]["CAPE"]["url"].as_str().map(str::to_string);
    let skins = skin_url.map(|u| {
        json!([{
            "url": u,
            "variant": if payload["textures"]["SKIN"]["metadata"]["model"].as_str() == Some("slim") { "slim" } else { "classic" },
            "state": "ACTIVE"
        }])
    }).unwrap_or(json!([]));
    let capes = cape_url.map(|u| json!([{ "id": "cape", "alias": "披风", "active": true, "url": u }])).unwrap_or(json!([]));
    Ok(json!({
        "username": data["name"].as_str().unwrap_or_else(|| account["username"].as_str().unwrap_or("")),
        "skins": skins,
        "capes": capes
    }))
}

