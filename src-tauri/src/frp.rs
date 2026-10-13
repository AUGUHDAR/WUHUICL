//! 内网穿透（樱花穿透 natfrp / frpc）：通道名、参数顺序、返回字段名、中文错误文案照抄原版
//! core/frp.ts + frpIpc.ts + frpManager.ts + frpNodes.ts + frpLog.ts + frpService.ts。

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tauri::{AppHandle, Emitter, Manager};

const API_BASE: &str = "https://api.natfrp.com/v4";
const CACHE_TTL_MS: i64 = 10 * 60 * 1000;
const API_TIMEOUT_SECS: u64 = 20;
const FRPC_ASSET_BASE: &str = "https://nya.globalslb.net/natfrp/client/frpc/0.51.0-sakura-14";
const MAX_LOGS: usize = 200;
const STOP_WAIT_MS: u64 = 3000;
const FLAG_CAN_CREATE: i64 = 1 << 2;
const FLAG_MAINLAND: i64 = 1 << 3;
const FLAG_NO_PROTECT: i64 = 1 << 4;
const FLAG_UDP: i64 = 1 << 5;
const FLAG_OFFLINE: i64 = 1 << 9;
const FLAG_BETA: i64 = 1 << 10;

// 对应原版 frp.ts 的 configFile()/frpcDir()/frpcPath() 与 frpService.ts 的 file()
fn config_file(app: &AppHandle) -> PathBuf {
    crate::settings::user_data_dir(app).join("frp-config.json")
}
fn frpc_dir(app: &AppHandle) -> PathBuf {
    crate::settings::user_data_dir(app).join("frp")
}
fn frpc_path(app: &AppHandle) -> PathBuf {
    frpc_dir(app).join(if cfg!(windows) { "frpc.exe" } else { "frpc" })
}
fn registry_file(app: &AppHandle) -> PathBuf {
    crate::settings::user_data_dir(app).join("frp-tunnels.json")
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn js_falsy(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::Bool(b) => !*b,
        Value::String(s) => s.is_empty(),
        Value::Number(n) => n.as_f64().map(|f| f == 0.0).unwrap_or(false),
        _ => false,
    }
}
fn or_val(v: &Value, fallback: Value) -> Value {
    if js_falsy(v) { fallback } else { v.clone() }
}
fn or_null(v: &Value) -> Value {
    or_val(v, Value::Null)
}
fn text_of(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_string()
}
fn js_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        other => other.to_string(),
    }
}
fn str_field(v: &Value) -> String {
    if v.is_null() { String::new() } else { js_string(v) }
}
fn as_i64(v: &Value) -> Option<i64> {
    v.as_i64().or_else(|| v.as_f64().filter(|f| f.fract() == 0.0).map(|f| f as i64))
}
fn num_or(v: &Value, def: i64) -> i64 {
    as_i64(v).unwrap_or(def)
}
fn is_safe_integer(v: &Value) -> bool {
    as_i64(v).map(|i| i.abs() <= 9_007_199_254_740_991).unwrap_or(false)
}
fn parse_int_loose(s: &str) -> i64 {
    let trimmed = s.trim_start();
    let digits = match trimmed.strip_prefix('-') {
        Some(rest) => format!("-{}", rest.chars().take_while(|c| c.is_ascii_digit()).collect::<String>()),
        None => trimmed.chars().take_while(|c| c.is_ascii_digit()).collect::<String>(),
    };
    digits.parse::<i64>().unwrap_or(0)
}

pub fn mask_key(value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= 4 {
        return "*".repeat(chars.len());
    }
    let head: String = chars[..2].iter().collect();
    let tail: String = chars[chars.len() - 2..].iter().collect();
    format!("{head}***{tail}")
}

fn encode_uri_component(value: &str) -> String {
    let mut out = String::new();
    for b in value.as_bytes() {
        let c = *b as char;
        if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '!' | '~' | '*' | '\'' | '(' | ')') {
            out.push(c);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn sanitize(raw: &str, key: &str) -> String {
    if raw.is_empty() {
        return raw.to_string();
    }
    let mut out = raw.to_string();
    if !key.is_empty() {
        let mask = mask_key(key);
        out = out.replace(key, &mask);
        let enc = encode_uri_component(key);
        if enc != key {
            out = out.replace(&enc, &mask);
        }
    }
    out
}

pub fn tunnel_identity(key: &str, id: &str) -> String {
    let mut h = Sha256::new();
    h.update(format!("{}\u{0}", key.trim()));
    h.update(id.as_bytes());
    hex::encode(h.finalize())
}

pub fn load_frp_config(app: &AppHandle) -> Option<Value> {
    let raw = std::fs::read_to_string(config_file(app)).ok()?;
    let parsed: Value = serde_json::from_str(&raw).ok()?;
    if !parsed.is_object() {
        return None;
    }
    let access_key = parsed.get("accessKey").and_then(|v| v.as_str())?;
    let tunnel_id = parsed.get("tunnelId").and_then(|v| v.as_str())?;
    let local_port = parsed.get("localPort").and_then(|v| as_i64(v)).unwrap_or(0);
    Some(json!({ "accessKey": access_key, "tunnelId": tunnel_id, "localPort": local_port }))
}

// 对应原版 frpManager.ts 的 readFrpRegistry()/writeFrpRegistry()
fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

fn read_registry(app: &AppHandle) -> Result<Value, String> {
    let file = registry_file(app);
    let legacy = load_frp_config(app);
    if !file.exists() {
        return Ok(match legacy {
            Some(l) => json!({
                "version": 2,
                "accessKey": or_val(&l["accessKey"], json!("")),
                "tunnels": [{
                    "id": tunnel_identity(&text_of(&l["accessKey"]), &text_of(&l["tunnelId"])),
                    "config": l,
                    "name": format!("隧道 {}", text_of(&l["tunnelId"])),
                    "nodeName": "",
                    "localIp": "127.0.0.1",
                    "desired": false
                }]
            }),
            None => json!({ "version": 2, "accessKey": "", "tunnels": [] }),
        });
    }
    let raw = std::fs::read_to_string(&file).map_err(|e| e.to_string())?;
    let parsed: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    if parsed["version"].as_i64() != Some(2) || !parsed["tunnels"].is_array() || !parsed["accessKey"].is_string() {
        return Err("隧道配置损坏，已保留原文件".to_string());
    }
    if let Some(ids) = parsed.get("deletedIds") {
        let bad = !ids.is_array()
            || ids
                .as_array()
                .map(|a| a.iter().any(|i| !i.as_str().map(is_hex64).unwrap_or(false)))
                .unwrap_or(false);
        if bad {
            return Err("隧道删除记录损坏，已保留原文件".to_string());
        }
    }
    let tunnels = parsed["tunnels"].as_array().cloned().unwrap_or_default();
    for r in &tunnels {
        let cfg = &r["config"];
        let digits_only = cfg["tunnelId"].as_str().map(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())).unwrap_or(false);
        let expected = tunnel_identity(&text_of(&cfg["accessKey"]), &text_of(&cfg["tunnelId"]));
        if js_falsy(&cfg["accessKey"]) || !digits_only || r["id"].as_str() != Some(expected.as_str()) || !r["desired"].is_boolean() {
            return Err("隧道配置无效，已保留原文件".to_string());
        }
    }
    Ok(parsed)
}

fn write_registry(app: &AppHandle, data: &Value) -> Result<(), String> {
    let file = registry_file(app);
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let temp = format!("{}.tmp", file.to_string_lossy());
    std::fs::write(&temp, serde_json::to_vec_pretty(data).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::rename(&temp, &file).map_err(|e| e.to_string())
}

struct NodesCache {
    access_key: String,
    result: Value,
    expires_at: i64,
}

static NODES_CACHE: OnceLock<Mutex<Option<NodesCache>>> = OnceLock::new();
static MANAGER: OnceLock<Mutex<ManagerState>> = OnceLock::new();

fn nodes_cache() -> std::sync::MutexGuard<'static, Option<NodesCache>> {
    NODES_CACHE.get_or_init(|| Mutex::new(None)).lock().unwrap_or_else(|e| e.into_inner())
}
fn manager() -> std::sync::MutexGuard<'static, ManagerState> {
    MANAGER.get_or_init(|| Mutex::new(ManagerState::default())).lock().unwrap_or_else(|e| e.into_inner())
}
fn clear_nodes_cache() {
    *nodes_cache() = None;
}

// 对应原版 frpNodes.ts 的 apiGet()/fetchFrpNodes()
fn api_get(path: &str, access_key: &str, body: Option<Value>) -> Result<Value, String> {
    let url = format!("{API_BASE}{path}");
    let timeout = std::time::Duration::from_secs(API_TIMEOUT_SECS);
    let send = match body {
        None => crate::net::client()
            .get(url.as_str())
            .header("Authorization", format!("Bearer {access_key}"))
            .timeout(timeout)
            .send(),
        Some(b) => crate::net::client()
            .post(url.as_str())
            .header("Authorization", format!("Bearer {access_key}"))
            .header("Content-Type", "application/json")
            .timeout(timeout)
            .json(&b)
            .send(),
    };
    let resp = match send {
        Ok(r) => r,
        Err(_) => return Err("无法连接樱花穿透，请检查网络后重试".to_string()),
    };
    let status = resp.status().as_u16();
    let text = resp.text().unwrap_or_default();
    if !(200..300).contains(&status) {
        let mut detail: String = text.chars().take(200).collect();
        if let Ok(parsed) = serde_json::from_str::<Value>(&text) {
            if parsed["msg"].is_string() {
                let code = match parsed.get("code") {
                    Some(c) if !c.is_null() => js_string(c),
                    _ => status.to_string(),
                };
                detail = format!("{code}: {}", text_of(&parsed["msg"]));
            }
        }
        let safe_detail = if access_key.is_empty() { detail } else { detail.replace(access_key, "***") };
        return Err(format!("natfrp API {path} 请求失败（HTTP {status}）：{safe_detail}"));
    }
    serde_json::from_str::<Value>(&text).map_err(|_| format!("natfrp API {path} 返回了无法解析的内容"))
}

fn fetch_frp_nodes(access_key: &str, refresh: bool) -> Result<Value, String> {
    let key = access_key.trim().to_string();
    if key.is_empty() {
        return Err("请先填写访问密钥，再查询节点列表".to_string());
    }
    if !refresh {
        let hit = nodes_cache().as_ref().map(|c| (c.access_key.clone(), c.result.clone(), c.expires_at));
        if let Some((ck, res, exp)) = hit {
            if ck == key && now_ms() < exp {
                return Ok(res);
            }
        }
    }
    let (nodes_res, stats_res, tunnels_res) = std::thread::scope(|s| {
        let k: &str = key.as_str();
        let a = s.spawn(move || api_get("/nodes", k, None));
        let b = s.spawn(move || api_get("/node/stats", k, None));
        let c = s.spawn(move || api_get("/tunnels", k, None));
        (a.join(), b.join(), c.join())
    });
    let nodes_raw = match nodes_res {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => return Err(e),
        Err(_) => return Err("无法连接樱花穿透，请检查网络后重试".to_string()),
    };
    let stats: Vec<Value> = match stats_res {
        Ok(Ok(v)) => v["nodes"].as_array().cloned().unwrap_or_default(),
        _ => Vec::new(),
    };
    let mut stat_by_id: HashMap<i64, (bool, Option<i64>)> = HashMap::new();
    for st in &stats {
        let id = match as_i64(&st["id"]) {
            Some(i) => i,
            None => continue,
        };
        let online = match st.get("online").and_then(|v| v.as_i64()) {
            Some(o) => o >= 0,
            None => true,
        };
        stat_by_id.insert(id, (online, st.get("load").and_then(|v| v.as_i64())));
    }

    let mut nodes: Vec<Value> = Vec::new();
    if let Some(map) = nodes_raw.as_object() {
        for (id_str, raw) in map {
            let id = parse_int_loose(id_str);
            let flag = raw.get("flag").and_then(|v| v.as_i64()).unwrap_or(0);
            let vip = raw.get("vip").and_then(|v| v.as_i64()).unwrap_or(0);
            let (online, load) = match stat_by_id.get(&id) {
                Some((o, l)) => (*o, *l),
                None => (flag & FLAG_OFFLINE == 0, None),
            };
            nodes.push(json!({
                "id": id,
                "name": raw.get("name").filter(|v| v.is_string()).cloned().unwrap_or(json!(format!("节点 {id_str}"))),
                "host": raw.get("host").filter(|v| v.is_string()).cloned().unwrap_or(json!("")),
                "description": raw.get("description").filter(|v| v.is_string()).cloned().unwrap_or(json!("")),
                "vip": vip,
                "free": vip == 0,
                "online": online,
                "load": load.map(Value::from).unwrap_or(Value::Null),
                "udp": flag & FLAG_UDP != 0,
                "mainland": flag & FLAG_MAINLAND != 0,
                "canCreate": flag & FLAG_CAN_CREATE != 0,
                "noProtect": flag & FLAG_NO_PROTECT != 0,
                "beta": flag & FLAG_BETA != 0
            }));
        }
    }
    nodes.sort_by(|a, b| {
        let fa = a["free"].as_bool().unwrap_or(false);
        let fb = b["free"].as_bool().unwrap_or(false);
        if fa != fb {
            return if fa { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater };
        }
        let oa = a["online"].as_bool().unwrap_or(false);
        let ob = b["online"].as_bool().unwrap_or(false);
        if oa != ob {
            return if oa { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater };
        }
        if let (Some(la), Some(lb)) = (a["load"].as_i64(), b["load"].as_i64()) {
            if la != lb {
                return la.cmp(&lb);
            }
        }
        a["id"].as_i64().unwrap_or(0).cmp(&b["id"].as_i64().unwrap_or(0))
    });

    let mut tunnels: Option<Vec<Value>> = None;
    if let Ok(Ok(rows)) = &tunnels_res {
        if let Some(arr) = rows.as_array() {
            let name_by_id: HashMap<i64, String> = nodes
                .iter()
                .filter_map(|n| as_i64(&n["id"]).map(|i| (i, text_of(&n["name"]))))
                .collect();
            tunnels = Some(
                arr.iter()
                    .filter(|t| t["id"].is_number())
                    .map(|t| {
                        let has_node = t["node"].is_number();
                        let node = num_or(&t["node"], 0);
                        let node_name = if has_node { name_by_id.get(&node).map(|s| json!(s)).unwrap_or(Value::Null) } else { Value::Null };
                        json!({
                            "id": t["id"].clone(),
                            "name": t["name"].as_str().unwrap_or_default().to_string(),
                            "type": t["type"].as_str().unwrap_or_default().to_string(),
                            "node": node,
                            "nodeName": node_name,
                            "online": t["online"].as_bool().unwrap_or(false),
                            "status": num_or(&t["status"], 0),
                            "localIp": t["local_ip"].as_str().unwrap_or_default().to_string(),
                            "localPort": num_or(&t["local_port"], 0),
                            "remote": t["remote"].as_str().unwrap_or_default().to_string()
                        })
                    })
                    .collect(),
            );
        }
    }

    let result = json!({
        "fetchedAt": now_iso(),
        "nodes": nodes,
        "tunnels": tunnels.map(Value::from).unwrap_or(Value::Null)
    });
    *nodes_cache() = Some(NodesCache { access_key: key, result: result.clone(), expires_at: now_ms() + CACHE_TTL_MS });
    Ok(result)
}

fn get_runnable_frp_tunnel(access_key: &str, id: &str) -> Result<Value, String> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
        return Err("请先选择已创建的隧道".to_string());
    }
    let result = fetch_frp_nodes(access_key, true)?;
    let tunnels: Vec<Value> = match &result["tunnels"] {
        Value::Array(arr) => arr.clone(),
        _ => return Err("隧道列表获取失败，请检查访问密钥后重试".to_string()),
    };
    let found = tunnels.into_iter().find(|t| js_string(&t["id"]) == id);
    let tunnel = match found {
        Some(t) if num_or(&t["status"], -1) == 0 => t,
        _ => return Err("隧道不存在或不可用，请刷新列表重新选择".to_string()),
    };
    if text_of(&tunnel["type"]) != "tcp" {
        return Err("Minecraft Java 版请选择 TCP 隧道".to_string());
    }
    let node_id = num_or(&tunnel["node"], -1);
    let node_online = result["nodes"]
        .as_array()
        .map(|arr| arr.iter().any(|n| num_or(&n["id"], -1) == node_id && n["online"].as_bool().unwrap_or(false)))
        .unwrap_or(false);
    if !node_online {
        return Err("隧道所在节点已离线，请选择其他隧道".to_string());
    }
    Ok(tunnel)
}

fn delete_frp_tunnel(access_key: &str, id: &str) -> Result<Value, String> {
    let key = access_key.trim().to_string();
    let shape_ok = !id.is_empty()
        && id.starts_with(|c: char| c.is_ascii_digit() && c != '0')
        && id.chars().all(|c| c.is_ascii_digit());
    let target = id.parse::<i64>().ok();
    if key.is_empty() || !shape_ok || target.map(|n| n > 9_007_199_254_740_991).unwrap_or(true) {
        return Err("隧道或账号无效，无法删除".to_string());
    }
    let target = target.unwrap_or(0);
    clear_nodes_cache();
    let exists = || -> Result<bool, String> {
        let rows = api_get("/tunnels", &key, None)?;
        let arr = match rows.as_array() {
            Some(a) => a,
            None => return Err("无法确认远端隧道列表，请刷新后重试删除".to_string()),
        };
        if arr.iter().any(|r| !is_safe_integer(&r["id"])) {
            return Err("无法确认远端隧道列表，请刷新后重试删除".to_string());
        }
        Ok(arr.iter().any(|r| num_or(&r["id"], -1) == target))
    };
    let outcome = (|| -> Result<Value, String> {
        if !exists()? {
            return Ok(json!({ "remoteDisconnectPending": false }));
        }
        let result = api_get("/tunnel/delete", &key, Some(json!({ "ids": id })))?;
        let deleted = match result["deleted"].as_array() {
            Some(a) => a.clone(),
            None => return Err("删除结果不完整，请重试确认远端状态".to_string()),
        };
        let failed = match result["failed"].as_array() {
            Some(a) => a.clone(),
            None => return Err("删除结果不完整，请重试确认远端状态".to_string()),
        };
        let hit = |arr: &[Value]| arr.iter().any(|v| as_i64(v) == Some(target));
        if !hit(&deleted) && !hit(&failed) {
            return Err("樱花穿透未确认删除该隧道，请检查隧道是否被锁定".to_string());
        }
        if exists()? {
            return Err("远端仍存在该隧道，尚未确认删除成功，请稍后重试".to_string());
        }
        Ok(json!({ "remoteDisconnectPending": hit(&failed) }))
    })();
    clear_nodes_cache();
    outcome
}

fn create_frp_tunnel(access_key: &str, input: &Value) -> Result<Value, String> {
    let name = str_field(&input["name"]).trim().to_string();
    if name.is_empty() || name.encode_utf16().count() > 64 {
        return Err("请填写 1–64 字符的隧道名称".to_string());
    }
    let local_port = match input.get("localPort").and_then(|v| as_i64(v)) {
        Some(p) if (1..=65535).contains(&p) => p,
        _ => return Err("请填写游戏中显示的局域网端口（1–65535）".to_string()),
    };
    let remote_supplied = !js_falsy(&input["remotePort"]);
    let remote_port = as_i64(&input["remotePort"]);
    if remote_supplied && !remote_port.map(|p| (1..=65535).contains(&p)).unwrap_or(false) {
        return Err("远程端口无效".to_string());
    }
    let result = fetch_frp_nodes(access_key, true)?;
    let node_id = num_or(&input["node"], -1);
    let node = result["nodes"]
        .as_array()
        .and_then(|arr| arr.iter().find(|n| num_or(&n["id"], -1) == node_id).cloned());
    let node = match node {
        Some(n) if n["online"].as_bool().unwrap_or(false) && n["canCreate"].as_bool().unwrap_or(false) => n,
        _ => return Err("所选节点离线或已满，请选择其他节点".to_string()),
    };
    let mut body = json!({
        "name": name,
        "type": "tcp",
        "node": num_or(&node["id"], node_id),
        "local_ip": "127.0.0.1",
        "local_port": local_port
    });
    if remote_supplied {
        body["remote"] = json!(remote_port.unwrap_or(0).to_string());
    }
    let created = api_get("/tunnels", access_key.trim(), Some(body))?;
    clear_nodes_cache();
    if !is_safe_integer(&created["id"]) || created["id"].as_i64().map(|i| i <= 0).unwrap_or(true) {
        return Err("创建结果不完整，请刷新隧道列表确认，勿重复创建".to_string());
    }
    Ok(created)
}

#[derive(Default)]
struct ManagerState {
    registry: Option<Value>,
    workers: HashMap<String, Arc<Mutex<WorkerState>>>,
    pending: HashMap<String, Arc<AtomicBool>>,
    deletions: HashSet<String>,
    generations: HashMap<String, u64>,
    failures: HashMap<String, String>,
    last_states: HashMap<String, Value>,
    closing: bool,
}

#[derive(Default)]
struct WorkerState {
    session: Option<Value>,
    child: Option<Arc<Mutex<Option<std::process::Child>>>>,
    epoch: u64,
    address_priority: i64,
    detached_by_user: bool,
}

// 对应原版 frpManager.ts 的 snapshot()/list()/register()/start()/stop()/remove()
fn registry_of(m: &mut ManagerState, app: &AppHandle) -> Result<Value, String> {
    if m.registry.is_none() {
        m.registry = Some(read_registry(app)?);
    }
    Ok(m.registry.clone().unwrap_or_else(|| json!({ "version": 2, "accessKey": "", "tunnels": [] })))
}

fn commit(m: &mut ManagerState, app: &AppHandle, next: Value) -> Result<(), String> {
    write_registry(app, &next)?;
    m.registry = Some(next);
    Ok(())
}

fn find_record(reg: &Value, id: &str) -> Option<Value> {
    reg["tunnels"]
        .as_array()
        .and_then(|arr| arr.iter().find(|t| t["id"].as_str() == Some(id)).cloned())
}

fn bump(m: &mut ManagerState, id: &str) -> u64 {
    let n = m.generations.get(id).copied().unwrap_or(0) + 1;
    m.generations.insert(id.to_string(), n);
    n
}

fn snapshot(m: &ManagerState, rec: &Value) -> Value {
    let id = text_of(&rec["id"]);
    let live = m.workers.get(&id).map(|w| worker_state_json(&w.lock().unwrap()));
    let chosen = match live {
        Some(v) if !v["config"].is_null() => Some(v),
        _ => m.last_states.get(&id).cloned(),
    };
    let pick = |field: &str| chosen.as_ref().map(|v| v[field].clone()).unwrap_or(Value::Null);
    let error = m.failures.get(&id);
    let status = match error {
        Some(_) => json!("error"),
        None => or_val(&pick("status"), json!("idle")),
    };
    let message = match error {
        Some(e) => json!(e),
        None => or_val(&pick("message"), json!("尚未启动")),
    };
    json!({
        "status": status,
        "config": rec["config"].clone(),
        "remoteAddress": or_null(&pick("remoteAddress")),
        "pid": or_null(&pick("pid")),
        "startedAt": or_null(&pick("startedAt")),
        "message": message,
        "logs": or_val(&pick("logs"), json!([])),
        "id": rec["id"].clone(),
        "name": rec["name"].clone(),
        "nodeName": rec["nodeName"].clone(),
        "localIp": rec["localIp"].clone(),
        "desired": rec["desired"].clone(),
        "busy": m.pending.contains_key(&id),
        "deleting": m.deletions.contains(&id)
    })
}

fn manager_list(app: &AppHandle) -> Result<Value, String> {
    let mut m = manager();
    let reg = registry_of(&mut m, app)?;
    let tunnels: Vec<Value> = reg["tunnels"]
        .as_array()
        .map(|arr| arr.iter().map(|rec| snapshot(&m, rec)).collect())
        .unwrap_or_default();
    Ok(json!({ "accessKey": reg["accessKey"].clone(), "tunnels": tunnels }))
}

fn emit_tunnel(app: &AppHandle, id: &str) {
    let mut m = manager();
    if m.registry.is_none() {
        m.registry = read_registry(app).ok();
    }
    let payload = m.registry.as_ref().and_then(|reg| find_record(reg, id)).map(|rec| snapshot(&m, &rec));
    drop(m);
    if let Some(tunnel) = payload {
        if let Some(win) = app.get_webview_window("main") {
            let _ = win.emit_to("main", "frp:event", json!({ "tunnel": tunnel }));
        }
    }
}

fn manager_register(app: &AppHandle, key: &str, tunnels: &[Value]) -> Result<(), String> {
    let mut m = manager();
    let mut next = registry_of(&mut m, app)?;
    let trimmed = key.trim().to_string();
    next["accessKey"] = json!(trimmed.clone());
    let deletions = m.deletions.clone();
    let deleted_ids: Vec<String> = next["deletedIds"]
        .as_array()
        .map(|a| a.iter().filter_map(|i| i.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    {
        let arr = next["tunnels"].as_array_mut().ok_or("隧道配置损坏，已保留原文件")?;
        for t in tunnels {
            let id_str = js_string(&t["id"]);
            let id = tunnel_identity(&trimmed, &id_str);
            if deleted_ids.iter().any(|s| s == &id) || deletions.contains(&id) {
                continue;
            }
            let name = or_val(&t["name"], json!(format!("隧道 {id_str}")));
            let node_name = or_val(&t["nodeName"], json!(""));
            let local_ip = t["localIp"].clone();
            let local_port = t["localPort"].clone();
            if let Some(old) = arr.iter_mut().find(|r| r["id"].as_str() == Some(id.as_str())) {
                old["name"] = name;
                old["nodeName"] = node_name;
                old["localIp"] = local_ip;
                old["config"]["localPort"] = local_port;
            } else {
                arr.push(json!({
                    "id": id,
                    "config": { "accessKey": trimmed.clone(), "tunnelId": id_str, "localPort": local_port },
                    "name": name,
                    "nodeName": node_name,
                    "localIp": local_ip,
                    "desired": false
                }));
            }
        }
    }
    commit(&mut m, app, next)
}

fn manager_start(app: &AppHandle, id: &str) -> Result<(), String> {
    let mut worker: Option<Arc<Mutex<WorkerState>>> = None;
    let mut flag: Option<Arc<AtomicBool>> = None;
    let mut generation = 0u64;
    let mut config = Value::Null;
    {
        let mut m = manager();
        if m.deletions.contains(id) {
            return Err("该隧道正在删除，请等待完成".to_string());
        }
        if m.closing {
            return Err("启动器正在关闭，请稍后重试".to_string());
        }
        let reg = registry_of(&mut m, app)?;
        let rec = match find_record(&reg, id) {
            Some(r) => r,
            None => return Err("未找到该隧道，请重新读取".to_string()),
        };
        if let Some(existing) = m.pending.get(id).cloned() {
            flag = Some(existing);
        } else {
            let active = m
                .workers
                .get(id)
                .map(|w| {
                    let g = w.lock().unwrap();
                    g.session.as_ref().map(|s| text_of(&s["status"])).unwrap_or_default()
                })
                .unwrap_or_default();
            if active == "running" || active == "starting" {
                return Ok(());
            }
            generation = bump(&mut m, id);
            let mut next = reg.clone();
            if let Some(arr) = next["tunnels"].as_array_mut() {
                if let Some(r) = arr.iter_mut().find(|r| r["id"].as_str() == Some(id)) {
                    r["desired"] = json!(true);
                }
            }
            commit(&mut m, app, next)?;
            m.failures.remove(id);
            let w = m
                .workers
                .entry(id.to_string())
                .or_insert_with(|| Arc::new(Mutex::new(WorkerState::default())))
                .clone();
            let new_flag = Arc::new(AtomicBool::new(false));
            m.pending.insert(id.to_string(), new_flag.clone());
            worker = Some(w);
            flag = Some(new_flag);
            config = rec["config"].clone();
        }
    }
    let flag = flag.unwrap_or_else(|| Arc::new(AtomicBool::new(true)));
    let worker = match worker {
        Some(w) => w,
        None => {
            while !flag.load(Ordering::SeqCst) {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            return Ok(());
        }
    };
    emit_tunnel(app, id);

    let access_key = text_of(&config["accessKey"]);
    let tunnel_id = text_of(&config["tunnelId"]);
    let result: Result<(), String> = (|| {
        let tunnel = get_runnable_frp_tunnel(&access_key, &tunnel_id)?;
        {
            let mut m = manager();
            if m.closing || m.generations.get(id) != Some(&generation) {
                return Ok(());
            }
            let mut next = registry_of(&mut m, app)?;
            if let Some(arr) = next["tunnels"].as_array_mut() {
                if let Some(r) = arr.iter_mut().find(|r| r["id"].as_str() == Some(id)) {
                    let port = tunnel["localPort"].clone();
                    let name = or_val(&tunnel["name"], r["name"].clone());
                    let node_name = or_val(&tunnel["nodeName"], r["nodeName"].clone());
                    let local_ip = or_val(&tunnel["localIp"], r["localIp"].clone());
                    r["config"]["localPort"] = port;
                    r["name"] = name;
                    r["nodeName"] = node_name;
                    r["localIp"] = local_ip;
                }
            }
            commit(&mut m, app, next)?;
        }
        worker_stop(&worker);
        {
            let m = manager();
            if m.closing || m.generations.get(id) != Some(&generation) {
                return Ok(());
            }
        }
        let mut start_config = config.clone();
        start_config["localPort"] = tunnel["localPort"].clone();
        worker_start(app, id, &worker, &start_config)
    })();

    let mut m = manager();
    if m.generations.get(id) == Some(&generation) && !m.closing {
        if let Err(e) = &result {
            let message = if access_key.is_empty() { e.clone() } else { e.replace(&access_key, "***") };
            m.failures.insert(id.to_string(), message);
        }
    }
    flag.store(true, Ordering::SeqCst);
    m.pending.remove(id);
    drop(m);
    emit_tunnel(app, id);
    result
}

fn manager_stop(app: &AppHandle, id: &str) -> Result<(), String> {
    let worker = {
        let mut m = manager();
        let reg = registry_of(&mut m, app)?;
        if find_record(&reg, id).is_none() {
            return Err("未找到该隧道，请重新读取".to_string());
        }
        let mut next = reg.clone();
        if let Some(arr) = next["tunnels"].as_array_mut() {
            if let Some(r) = arr.iter_mut().find(|r| r["id"].as_str() == Some(id)) {
                r["desired"] = json!(false);
            }
        }
        commit(&mut m, app, next)?;
        bump(&mut m, id);
        m.failures.remove(id);
        m.workers.get(id).cloned()
    };
    emit_tunnel(app, id);
    if let Some(w) = worker {
        worker_stop(&w);
    }
    emit_tunnel(app, id);
    Ok(())
}

fn manager_remove(app: &AppHandle, id: &str) -> Result<Value, String> {
    loop {
        let busy = manager().deletions.contains(id);
        if !busy {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let (config, worker) = {
        let mut m = manager();
        if m.closing {
            return Err("启动器正在关闭，请稍后重试".to_string());
        }
        let reg = registry_of(&mut m, app)?;
        let rec = match find_record(&reg, id) {
            Some(r) => r,
            None => return Err("未找到该隧道，请重新读取".to_string()),
        };
        m.deletions.insert(id.to_string());
        (rec["config"].clone(), m.workers.get(id).cloned())
    };
    let access_key = text_of(&config["accessKey"]);
    let tunnel_id = text_of(&config["tunnelId"]);
    let mut remote_deleted = false;
    let outcome: Result<Value, String> = (|| {
        manager_stop(app, id)?;
        loop {
            let busy = manager().pending.contains_key(id);
            if !busy {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        if let Some(w) = worker.as_ref() {
            let pid = {
                let g = w.lock().unwrap();
                or_null(&g.session.as_ref().map(|s| s["pid"].clone()).unwrap_or(Value::Null))
            };
            if !pid.is_null() {
                return Err("本地隧道尚未停止，未执行远端删除，请稍后重试".to_string());
            }
        }
        let result = delete_frp_tunnel(&access_key, &tunnel_id)?;
        remote_deleted = true;
        let mut m = manager();
        let mut next = registry_of(&mut m, app)?;
        if let Some(arr) = next["tunnels"].as_array_mut() {
            arr.retain(|t| t["id"].as_str() != Some(id));
        }
        let mut deleted_ids: Vec<Value> = next["deletedIds"].as_array().cloned().unwrap_or_default();
        if !deleted_ids.iter().any(|i| i.as_str() == Some(id)) {
            deleted_ids.push(json!(id));
        }
        next["deletedIds"] = Value::Array(deleted_ids);
        commit(&mut m, app, next)?;
        m.workers.remove(id);
        m.last_states.remove(id);
        m.failures.remove(id);
        Ok(result)
    })();
    let outcome = outcome.map_err(|e| {
        let detail = if access_key.is_empty() { e } else { e.replace(&access_key, "***") };
        if remote_deleted {
            format!("远端隧道已删除，但本地记录保存失败；请重试清理：{detail}")
        } else {
            format!("未能完成删除；隧道卡片已保留：{detail}")
        }
    });

    let mut m = manager();
    if let Err(msg) = &outcome {
        m.failures.insert(id.to_string(), msg.clone());
    }
    m.deletions.remove(id);
    let still_there = m.registry.as_ref().map(|r| find_record(r, id).is_some()).unwrap_or(false);
    drop(m);
    if still_there {
        emit_tunnel(app, id);
    }
    outcome
}

/// 对应原版 index.ts 的 frpManager.restore()/shutdown()（主控接线点）
pub fn restore(app: &AppHandle) {
    let ids: Vec<String> = {
        let mut m = manager();
        m.closing = false;
        match registry_of(&mut m, app) {
            Ok(reg) => reg["tunnels"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter(|t| t["desired"].as_bool().unwrap_or(false))
                        .filter_map(|t| t["id"].as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    };
    if ids.is_empty() {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let cursor = Arc::new(AtomicUsize::new(0));
        std::thread::scope(|s| {
            for _ in 0..ids.len().min(3) {
                let cursor = cursor.clone();
                let ids = ids.clone();
                let app = app.clone();
                s.spawn(move || loop {
                    let index = cursor.fetch_add(1, Ordering::SeqCst);
                    if index >= ids.len() || manager().closing {
                        break;
                    }
                    let _ = manager_start(&app, &ids[index]);
                });
            }
        });
    });
}

pub fn shutdown() {
    let workers: Vec<Arc<Mutex<WorkerState>>> = {
        let mut m = manager();
        m.closing = true;
        let ids: Vec<String> = m
            .registry
            .as_ref()
            .and_then(|r| r["tunnels"].as_array().cloned())
            .map(|a| a.iter().filter_map(|t| t["id"].as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        for id in ids {
            bump(&mut m, &id);
        }
        m.workers.values().cloned().collect()
    };
    for w in workers {
        worker_stop(&w);
    }
}

// 对应原版 frp.ts 的 FrpController.status()/start()/stop() 与 ensureFrpcInstalled()
fn worker_state_json(w: &WorkerState) -> Value {
    match &w.session {
        None => json!({
            "status": "idle",
            "config": Value::Null,
            "remoteAddress": Value::Null,
            "pid": Value::Null,
            "startedAt": Value::Null,
            "message": "尚未启动",
            "logs": []
        }),
        Some(s) => json!({
            "status": s["status"].clone(),
            "config": s["config"].clone(),
            "remoteAddress": s["remoteAddress"].clone(),
            "pid": s["pid"].clone(),
            "startedAt": s["startedAt"].clone(),
            "message": status_message(&text_of(&s["status"]), &or_null(&s["remoteAddress"])),
            "logs": s["logs"].clone()
        }),
    }
}

fn status_message(status: &str, remote_address: &Value) -> String {
    match status {
        "starting" => "正在连接 SakuraFrp…".to_string(),
        "running" => match remote_address {
            Value::Null => "已连接，等待远程地址".to_string(),
            other => format!("已连接，远程地址 {}", text_of(other)),
        },
        "auth_failed" => "访问密钥无效或已失效".to_string(),
        "tunnel_offline" => "隧道不存在或已被禁用".to_string(),
        "error" => "frpc 进程异常退出".to_string(),
        "stopped" => "已停止".to_string(),
        _ => "空闲".to_string(),
    }
}

fn frpc_asset() -> Result<(String, Option<String>), String> {
    let platform = match std::env::consts::OS {
        "windows" => "win32",
        "macos" => "darwin",
        other => other,
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => other,
    };
    if platform == "win32" {
        return Ok((format!("{FRPC_ASSET_BASE}/frpc_windows_amd64.exe"), None));
    }
    if platform == "linux" && (arch == "x64" || arch == "arm64") {
        let (bin, sha) = if arch == "x64" {
            ("amd64", "8d3fcf1e24537719c36c6270c122fcdc09954d22ae9dd74105241675e6dce4d0")
        } else {
            ("arm64", "7f1bf530eb6b46b47e9b1e4bb022d689b5f26f5132581c29c5f43d922a42e595")
        };
        return Ok((format!("{FRPC_ASSET_BASE}/frpc_linux_{bin}"), Some(sha.to_string())));
    }
    let (bin, sha) = match (platform, arch) {
        ("darwin", "x64") => ("amd64", "74ee362350314dd5ac8936fbe2299fc76671051e10beb46c8dd37c36a4503935"),
        ("darwin", "arm64") => ("arm64", "465db9daea0e14e3adaa89926640afa8b44737dadc1cf0b75f9b091850d2e331"),
        _ => return Err(format!("樱花穿透暂不支持 {platform}/{arch}")),
    };
    Ok((format!("{FRPC_ASSET_BASE}/frpc_darwin_{bin}"), Some(sha.to_string())))
}

fn ensure_frpc_installed(app: &AppHandle) -> Result<PathBuf, String> {
    let target = frpc_path(app);
    if cfg!(windows) && target.exists() {
        return Ok(target);
    }
    let (url, sha256) = frpc_asset()?;
    std::fs::create_dir_all(frpc_dir(app)).map_err(|e| e.to_string())?;
    let rec = ghost_task();
    let opts = crate::download::DownloadFileOpts {
        sha256: sha256.as_deref(),
        ..Default::default()
    };
    match crate::download::download_file(app, &rec, &url, &target, &opts) {
        Ok(()) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755));
            }
            Ok(target)
        }
        Err(msg) => Err(format!("frpc 下载失败：{msg}。请检查网络后重试，官方下载地址：{url}")),
    }
}

fn ghost_task() -> Arc<crate::tasks::TaskRecord> {
    Arc::new(crate::tasks::TaskRecord {
        id: format!("frpc-{}", now_ms()),
        title: String::new(),
        kind: "frpc",
        cancelled: Arc::new(AtomicBool::new(false)),
        paused: Arc::new(AtomicBool::new(false)),
        status: Mutex::new(crate::tasks::TaskStatus::Running),
    })
}

fn worker_start(app: &AppHandle, id: &str, worker: &Arc<Mutex<WorkerState>>, config: &Value) -> Result<(), String> {
    let epoch = {
        let mut w = worker.lock().unwrap();
        if w.session.is_some() {
            return Err("frpc 已在运行中，请先停止".to_string());
        }
        w.epoch += 1;
        w.detached_by_user = false;
        w.address_priority = 0;
        w.child = None;
        w.epoch
    };
    let access_key = text_of(&config["accessKey"]).trim().to_string();
    let tunnel_id = text_of(&config["tunnelId"]).trim().to_string();
    let local_port = config["localPort"].as_i64().unwrap_or(0);
    if access_key.is_empty() {
        return Err("请填写访问密钥".to_string());
    }
    if tunnel_id.is_empty() {
        return Err("请填写隧道 ID".to_string());
    }
    let target = ensure_frpc_installed(app)?;
    if worker.lock().unwrap().epoch != epoch {
        return Err("启动已取消".to_string());
    }
    let saved = json!({ "accessKey": access_key.clone(), "tunnelId": tunnel_id.clone(), "localPort": local_port });
    let started_at = now_iso();

    let mut cmd = std::process::Command::new(&target);
    cmd.arg("-f")
        .arg(format!("{access_key}:{tunnel_id}"))
        .arg("--disable_log_color")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let (mut child_opt, spawn_err) = match cmd.spawn() {
        Ok(c) => (Some(c), None),
        Err(e) => (None, Some(e.to_string())),
    };
    let pid = child_opt.as_mut().map(|c| c.id() as u64).unwrap_or(0);
    let child_arc = Arc::new(Mutex::new(child_opt));
    {
        let mut w = worker.lock().unwrap();
        w.session = Some(json!({
            "status": "starting",
            "config": saved,
            "remoteAddress": Value::Null,
            "pid": pid,
            "startedAt": started_at.clone(),
            "logs": []
        }));
        w.child = Some(child_arc.clone());
    }
    worker_event(app, id, worker, "status", Some("starting"), None);
    worker_event(app, id, worker, "log", None, None);

    if pid == 0 {
        let message = sanitize(spawn_err.as_deref().unwrap_or("启动失败"), &access_key);
        {
            let mut w = worker.lock().unwrap();
            if let Some(s) = w.session.as_mut() {
                push_log(s, json!({ "ts": now_iso(), "stream": "system", "text": format!("进程错误：{message}") }));
            }
        }
        worker_event(app, id, worker, "log", None, None);
        {
            let mut w = worker.lock().unwrap();
            if let Some(s) = w.session.as_mut() {
                s["status"] = json!("error");
            }
        }
        worker_event(app, id, worker, "error", Some("error"), Some(&message));
        worker_event(app, id, worker, "stopped", Some("error"), Some(&message));
        let mut w = worker.lock().unwrap();
        if w.session.as_ref().and_then(|s| s["startedAt"].as_str()) == Some(started_at.as_str()) {
            w.session = None;
        }
        return Ok(());
    }

    let pipes: Vec<(&'static str, Box<dyn Read + Send>)> = {
        let mut guard = child_arc.lock().unwrap();
        match guard.as_mut() {
            Some(child) => {
                let mut v: Vec<(&'static str, Box<dyn Read + Send>)> = Vec::new();
                if let Some(out) = child.stdout.take() {
                    v.push(("stdout", Box::new(out)));
                }
                if let Some(err) = child.stderr.take() {
                    v.push(("stderr", Box::new(err)));
                }
                v
            }
            None => Vec::new(),
        }
    };
    for (stream, reader) in pipes {
        let app = app.clone();
        let id = id.to_string();
        let worker = worker.clone();
        let key = access_key.clone();
        let stamp = started_at.clone();
        std::thread::spawn(move || {
            let mut reader = reader;
            let mut pending: Vec<u8> = Vec::new();
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        pending.extend_from_slice(&buf[..n]);
                        while let Some(pos) = pending.iter().position(|b| *b == b'\n') {
                            let line: Vec<u8> = pending.drain(..=pos).collect();
                            flush_line(&app, &id, &worker, stream, &line, &key, &stamp);
                        }
                    }
                    Err(_) => break,
                }
            }
            if !pending.is_empty() {
                flush_line(&app, &id, &worker, stream, &pending, &key, &stamp);
            }
        });
    }

    {
        let app = app.clone();
        let id = id.to_string();
        let worker = worker.clone();
        let stamp = started_at.clone();
        std::thread::spawn(move || {
            let mut status_opt: Option<std::process::ExitStatus> = None;
            loop {
                let mut guard = child_arc.lock().unwrap();
                let got = match guard.as_mut() {
                    Some(child) => child.try_wait().ok().flatten(),
                    None => None,
                };
                if got.is_some() {
                    *guard = None;
                    status_opt = got;
                    break;
                }
                if guard.is_none() {
                    break;
                }
                drop(guard);
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            let mine = {
                let w = worker.lock().unwrap();
                w.session.as_ref().and_then(|s| s["startedAt"].as_str()) == Some(stamp.as_str())
            };
            if !mine {
                return;
            }
            let code = status_opt.as_ref().and_then(|s| s.code());
            #[cfg(unix)]
            let signal = {
                use std::os::unix::process::ExitStatusExt;
                if code.is_none() { status_opt.as_ref().and_then(|s| s.signal()) } else { None }
            };
            #[cfg(not(unix))]
            let signal: Option<i32> = None;
            let text = format!(
                "frpc 已退出（code={}, signal={}）",
                code.map(|c| c.to_string()).unwrap_or_else(|| "null".to_string()),
                signal.map(|c| c.to_string()).unwrap_or_else(|| "null".to_string())
            );
            {
                let mut w = worker.lock().unwrap();
                match w.session.as_mut() {
                    Some(s) if s["startedAt"].as_str() == Some(stamp.as_str()) => {
                        push_log(s, json!({ "ts": now_iso(), "stream": "system", "text": text.clone() }));
                    }
                    _ => return,
                }
            }
            worker_event(&app, &id, &worker, "log", None, None);
            let detached = {
                let w = worker.lock().unwrap();
                w.detached_by_user
            };
            let final_status = {
                let mut w = worker.lock().unwrap();
                match w.session.as_mut() {
                    Some(s) if s["startedAt"].as_str() == Some(stamp.as_str()) => {
                        let cur = text_of(&s["status"]);
                        let next = if detached || code == Some(0) {
                            "stopped".to_string()
                        } else if cur != "auth_failed" && cur != "tunnel_offline" {
                            "error".to_string()
                        } else {
                            cur
                        };
                        s["status"] = json!(next.clone());
                        next
                    }
                    _ => return,
                }
            };
            worker_event(&app, &id, &worker, "stopped", Some(&final_status), Some(&text));
            let mut w = worker.lock().unwrap();
            if w.session.as_ref().and_then(|s| s["startedAt"].as_str()) == Some(stamp.as_str()) {
                w.session = None;
            }
        });
    }
    Ok(())
}

fn flush_line(app: &AppHandle, id: &str, worker: &Arc<Mutex<WorkerState>>, stream: &str, bytes: &[u8], key: &str, stamp: &str) {
    let mut line = bytes.to_vec();
    if line.last() == Some(&b'\n') {
        line.pop();
    }
    if line.last() == Some(&b'\r') {
        line.pop();
    }
    let text = String::from_utf8_lossy(&line).to_string();
    if text.trim().is_empty() {
        return;
    }
    handle_line(app, id, worker, stream, text, key, stamp);
}

fn push_log(session: &mut Value, entry: Value) {
    if let Some(arr) = session["logs"].as_array_mut() {
        arr.push(entry);
        if arr.len() > MAX_LOGS {
            let cut = arr.len() - MAX_LOGS;
            arr.splice(0..cut, []);
        }
    }
}

// 对应原版 frpLog.ts 的 parseFrpLine()/frpLineReader()
fn re_ansi() -> &'static regex::Regex {
    static R: OnceLock<regex::Regex> = OnceLock::new();
    R.get_or_init(|| regex::Regex::new(r"\x1b\[[0-9;]*[A-Za-z]").unwrap())
}
fn re_auth() -> &'static regex::Regex {
    static R: OnceLock<regex::Regex> = OnceLock::new();
    R.get_or_init(|| regex::Regex::new(r"(?i)invalid token|login to server failed|authorization failed|访问密钥.*(?:无效|失效)|认证失败").unwrap())
}
fn re_tunnel() -> &'static regex::Regex {
    static R: OnceLock<regex::Regex> = OnceLock::new();
    R.get_or_init(|| regex::Regex::new(r"(?i)tunnel not exists|tunnel offline|proxy not found|隧道不存在|隧道已被禁用").unwrap())
}
fn re_start_fail() -> &'static regex::Regex {
    static R: OnceLock<regex::Regex> = OnceLock::new();
    R.get_or_init(|| regex::Regex::new(r"(?i)start proxy (?:error|failed)|隧道启动失败").unwrap())
}
fn re_running() -> &'static regex::Regex {
    static R: OnceLock<regex::Regex> = OnceLock::new();
    R.get_or_init(|| regex::Regex::new(r"(?i)\b(?:start )?proxy success\b|隧道启动成功").unwrap())
}
fn re_contextual() -> &'static regex::Regex {
    static R: OnceLock<regex::Regex> = OnceLock::new();
    R.get_or_init(|| regex::Regex::new(r"(?i)需要输\s*IP\s*[:：]\s*端口|远程地址|remote address|(?:电信|联通|移动)\s*[:：]").unwrap())
}
fn re_priority() -> &'static regex::Regex {
    static R: OnceLock<regex::Regex> = OnceLock::new();
    R.get_or_init(|| regex::Regex::new(r"(?i)需要输\s*IP\s*[:：]\s*端口|远程地址|remote address").unwrap())
}
fn re_known() -> &'static regex::Regex {
    static R: OnceLock<regex::Regex> = OnceLock::new();
    R.get_or_init(|| regex::Regex::new(r"(?i)(?:[\w-]+\.)*(?:natfrp\.cloud|nyatwork\.cn|frp-hub\.com|frp\.com|frp\.net)").unwrap())
}
fn re_banned() -> &'static regex::Regex {
    static R: OnceLock<regex::Regex> = OnceLock::new();
    R.get_or_init(|| regex::Regex::new(r"^(?:127\.|0\.0\.0\.0$|localhost$)").unwrap())
}
fn re_hostport() -> &'static regex::Regex {
    static R: OnceLock<regex::Regex> = OnceLock::new();
    R.get_or_init(|| regex::Regex::new(r"(?i)((?:[a-z0-9-]+\.)+[a-z0-9-]+)\s*:\s*(\d{1,5})(\D|$)").unwrap())
}

fn parse_frp_line(raw: &str) -> (Option<String>, Option<&'static str>) {
    let text = re_ansi().replace_all(raw, "").to_string();
    let mut status: Option<&'static str> = None;
    if re_auth().is_match(&text) {
        status = Some("auth_failed");
    } else if re_tunnel().is_match(&text) {
        status = Some("tunnel_offline");
    } else if re_start_fail().is_match(&text) {
        status = Some("error");
    } else if re_running().is_match(&text) {
        status = Some("running");
    }
    let contextual = re_contextual().is_match(&text);
    let mut address: Option<String> = None;
    for caps in re_hostport().captures_iter(&text) {
        let host = caps.get(1).map(|m| m.as_str()).unwrap_or_default();
        let port: i64 = caps.get(2).map(|m| m.as_str()).unwrap_or_default().parse().unwrap_or(0);
        let boundary_ok = caps.get(3).map(|m| m.as_str().is_empty() || !m.as_str().starts_with(|c: char| c.is_ascii_digit())).unwrap_or(true);
        if (contextual || re_known().is_match(host)) && !re_banned().is_match(host) && boundary_ok && port > 0 && port <= 65535 {
            address = Some(format!("{host}:{port}"));
            break;
        }
    }
    (address, status)
}

fn handle_line(app: &AppHandle, id: &str, worker: &Arc<Mutex<WorkerState>>, stream: &str, raw: String, key: &str, stamp: &str) {
    let cleaned = sanitize(&raw, key);
    let entry = json!({ "ts": now_iso(), "stream": stream, "text": cleaned.clone() });
    let (address, status) = parse_frp_line(&cleaned);
    let priority = if re_priority().is_match(&cleaned) { 2 } else { 1 };
    let mut announced = false;
    {
        let mut guard = worker.lock().unwrap();
        let st = &mut *guard;
        if let Some(s) = st.session.as_mut() {
            if s["startedAt"].as_str() == Some(stamp) {
                push_log(s, entry.clone());
                if let Some(addr) = &address {
                    if priority >= st.address_priority && s["remoteAddress"].as_str() != Some(addr.as_str()) {
                        s["remoteAddress"] = json!(addr);
                        st.address_priority = priority;
                        announced = true;
                    }
                }
            }
        }
    }
    worker_event(app, id, worker, "log", None, None);
    if announced {
        worker_event(app, id, worker, "ready", None, None);
    }
    if let Some(st) = status {
        let changed = {
            let mut w = worker.lock().unwrap();
            match w.session.as_mut() {
                Some(s) if s["startedAt"].as_str() == Some(stamp) && text_of(&s["status"]) != st => {
                    s["status"] = json!(st);
                    true
                }
                _ => false,
            }
        };
        if changed {
            let remote = {
                let w = worker.lock().unwrap();
                w.session.as_ref().map(|s| or_null(&s["remoteAddress"])).unwrap_or(Value::Null)
            };
            let message = status_message(st, &remote);
            worker_event(app, id, worker, "status", Some(st), Some(&message));
        }
    }
}

// 对应原版 frpManager.ts 的 worker.setSink() 与 frpIpc.ts 的 installFrpEventBridge()
fn worker_event(app: &AppHandle, id: &str, worker: &Arc<Mutex<WorkerState>>, kind: &str, status: Option<&str>, message: Option<&str>) {
    let mut m = manager();
    let same = m.workers.get(id).map(|w| Arc::ptr_eq(w, worker)).unwrap_or(false);
    if !same {
        return;
    }
    let state = worker_state_json(&worker.lock().unwrap());
    m.last_states.insert(id.to_string(), state);
    if kind == "error" || (kind == "stopped" && status != Some("stopped")) {
        m.failures.insert(id.to_string(), message.unwrap_or("隧道异常退出").to_string());
    }
    let payload = m.registry.as_ref().and_then(|reg| find_record(reg, id)).map(|rec| snapshot(&m, &rec));
    drop(m);
    if let Some(tunnel) = payload {
        if let Some(win) = app.get_webview_window("main") {
            let _ = win.emit_to("main", "frp:event", json!({ "tunnel": tunnel }));
        }
    }
}

fn worker_stop(worker: &Arc<Mutex<WorkerState>>) {
    let stamp = {
        let mut w = worker.lock().unwrap();
        w.epoch += 1;
        let stamp = w.session.as_ref().map(|s| text_of(&s["startedAt"]));
        if stamp.is_none() {
            return;
        }
        w.detached_by_user = true;
        stamp.unwrap_or_default()
    };
    let (child_arc, pid) = {
        let w = worker.lock().unwrap();
        let pid = w.session.as_ref().and_then(|s| s["pid"].as_u64()).unwrap_or(0);
        (w.child.clone(), pid)
    };
    if let Some(arc) = child_arc {
        if let Some(child) = arc.lock().unwrap().as_mut() {
            let _ = child.kill();
        }
    }
    if pid != 0 {
        kill_process_tree(pid);
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(STOP_WAIT_MS);
    while std::time::Instant::now() < deadline {
        let ended = {
            let w = worker.lock().unwrap();
            match w.session.as_ref() {
                None => true,
                Some(s) => s["startedAt"].as_str() != Some(stamp.as_str()),
            }
        };
        if ended {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

fn kill_process_tree(pid: u64) {
    if !cfg!(windows) {
        return;
    }
    let pid_text = pid.to_string();
    let mut cmd = std::process::Command::new("taskkill");
    cmd.args(["/T", "/F", "/PID", pid_text.as_str()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let _ = cmd.spawn();
}

// 对应原版 frpIpc.ts 的 registerFrpIpc()
pub fn invoke(app: &AppHandle, channel: &str, args: &[Value]) -> Option<Result<Value, String>> {
    let payload = args.first().cloned().unwrap_or(Value::Null);
    match channel {
        "frp:status" => Some(manager_list(app)),
        "frp:nodes" => Some((|| {
            let mut key = str_field(&payload["accessKey"]).trim().to_string();
            if key.is_empty() {
                key = text_of(&manager_list(app)?["accessKey"]);
            }
            if key.is_empty() {
                key = load_frp_config(app).map(|c| text_of(&c["accessKey"])).unwrap_or_default();
            }
            let refresh = !js_falsy(&payload["refresh"]);
            let result = fetch_frp_nodes(&key, refresh)?;
            if let Some(tunnels) = result["tunnels"].as_array() {
                let tunnels = tunnels.clone();
                manager_register(app, &key, &tunnels)?;
            }
            Ok(result)
        })()),
        "frp:start" => Some((|| {
            if !payload.is_object() {
                return Err("参数无效".to_string());
            }
            if !js_falsy(&payload["id"]) {
                manager_start(app, &js_string(&payload["id"]))?;
                return Ok(Value::Null);
            }
            let access_key = str_field(&payload["accessKey"]).trim().to_string();
            let tunnel_id = str_field(&payload["tunnelId"]).trim().to_string();
            if access_key.is_empty() {
                return Err("请填写访问密钥".to_string());
            }
            let tunnel = get_runnable_frp_tunnel(&access_key, &tunnel_id)?;
            manager_register(app, &access_key, std::slice::from_ref(&tunnel))?;
            manager_start(app, &tunnel_identity(&access_key, &tunnel_id))?;
            Ok(Value::Null)
        })()),
        "frp:stop" => Some((|| {
            let id = if js_falsy(&payload["id"]) { String::new() } else { js_string(&payload["id"]) };
            if id.is_empty() {
                return Err("请选择要停止的隧道".to_string());
            }
            manager_stop(app, &id)?;
            manager_list(app)
        })()),
        "frp:delete-tunnel" => Some((|| {
            let id = if js_falsy(&payload["id"]) { String::new() } else { js_string(&payload["id"]) };
            if id.is_empty() || payload["confirmed"].as_bool() != Some(true) {
                return Err("请先确认删除该隧道".to_string());
            }
            manager_remove(app, &id)
        })()),
        "frp:create-tunnel" => Some(create_frp_tunnel(&str_field(&payload["accessKey"]), &payload["tunnel"])),
        _ => None,
    }
}
