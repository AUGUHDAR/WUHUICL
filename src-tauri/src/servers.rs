//! 服务器管理（对应原版 core/servers.ts 主线）：列表持久化 + SLP 状态探测 +
//! servers.dat 同步 + 启动准备。servers:list/add/remove/edit/favorite/ping/
//! bind/syncFromDat/prepareLaunch。

use crate::nbt::{self, NbtValue};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn servers_file(app: &tauri::AppHandle) -> PathBuf {
    crate::settings::user_data_dir(app).join("servers.json")
}

fn load(app: &tauri::AppHandle) -> Vec<Value> {
    std::fs::read(servers_file(app))
        .ok()
        .and_then(|raw| serde_json::from_slice::<Value>(&raw).ok())
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
}

fn save(app: &tauri::AppHandle, list: &[Value]) {
    let file = servers_file(app);
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(list) {
        let _ = std::fs::write(file, bytes);
    }
}

/// 规范化地址：host:port（IPv6 host 带方括号），默认端口省略（对应 normalizeAddress）
fn normalize_address(address: &str) -> (String, String, Option<u16>) {
    let (host, port, explicit) = if address.starts_with('[') {
        match address.find(']') {
            Some(end) => {
                let host = address[1..end].to_string();
                let port = address[end + 1..].trim_start_matches(':').parse::<u16>().ok();
                (host, port, port.is_some())
            }
            None => (address.trim().to_string(), None, false),
        }
    } else if address.matches(':').count() == 1 {
        let (h, p) = address.split_once(':').unwrap();
        (h.to_string(), p.parse::<u16>().ok(), true)
    } else {
        (address.trim().to_string(), None, false)
    };
    let port = port.unwrap_or(25565);
    let normalized = if explicit && port != 25565 {
        if host.contains(':') { format!("[{host}]:{port}") } else { format!("{host}:{port}") }
    } else {
        host.clone()
    };
    (normalized, host, Some(port))
}

// ---------------- SLP 状态探测（pingServer 1:1） ----------------

fn write_var_int(out: &mut Vec<u8>, value: i32) {
    // 按 MC 协议把 i32 当 u32 编码(最多 5 字节)。旧实现对负数(如 handshake 的 protocol=-1)
    // 算术右移永远非 0 → 无限 push → 缓冲翻倍到 4GB → OOM 崩溃(实测)。
    let mut v = value as u32;
    loop {
        let mut byte = (v & 0x7f) as u8;
        v >>= 7;
        if v != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if v == 0 {
            break;
        }
    }
}

fn packet(id: i32, payload: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    write_var_int(&mut body, id);
    body.extend_from_slice(payload);
    let mut out = Vec::new();
    write_var_int(&mut out, body.len() as i32);
    out.extend_from_slice(&body);
    out
}

fn read_var_int(stream: &mut impl Read, cap: usize) -> Result<i32, String> {
    // i64 累加：旧实现 `<< (shift*7)` 在 shift=28 时于 i32 上环绕成负数，
    // 长度检查被绕过 → take(天文数字) → read_to_end 缓冲翻倍到 4GB → OOM 崩溃
    let mut len: i64 = 0;
    let mut byte = [0u8; 1];
    for shift in 0..5 {
        stream.read_exact(&mut byte).map_err(|_| "连接中断".to_string())?;
        len |= ((byte[0] & 0x7f) as i64) << (shift * 7);
        if byte[0] & 0x80 == 0 {
            if len < 0 || len > cap as i64 {
                return Err("响应过大".into());
            }
            return Ok(len as i32);
        }
    }
    Err("响应格式无效".into())
}

/// MOTD 去 JSON 化（描述可为 string / 数组 / 组件树）
fn motd_text(desc: &Value) -> String {
    match desc {
        Value::String(s) => strip_codes(s),
        Value::Array(arr) => arr.iter().map(motd_text).collect::<Vec<_>>().join(""),
        Value::Object(o) => {
            let mut out = String::new();
            if let Some(t) = o["text"].as_str() {
                out.push_str(&strip_codes(t));
            }
            if let Some(extra) = o["extra"].as_array() {
                for e in extra {
                    out.push_str(&motd_text(e));
                }
            }
            out
        }
        _ => String::new(),
    }
}

fn strip_codes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '§' {
            chars.next();
            continue;
        }
        out.push(c);
    }
    out
}

/// servers:ping → ServerPingResult（永不 reject，离线返回固定文案，6s 超时）
pub fn ping_server(address: &str) -> Value {
    let offline = json!({
        "online": false, "players": "-",
        "motd": "无法连接（服务器离线或地址错误）", "version": "-", "latencyMs": 0
    });
    let (normalized, host, port) = normalize_address(address);
    let _ = normalized;
    let addr = format!("{host}:{}", port.unwrap_or(25565));
    let Ok(mut stream) = TcpStream::connect(&addr) else {
        return offline;
    };
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(6)));
    let start = Instant::now();
    // Handshake（next state=1）+ Status Request
    let mut hs = Vec::new();
    write_var_int(&mut hs, -1);
    let host_bytes = host.as_bytes();
    write_var_int(&mut hs, host_bytes.len() as i32);
    hs.extend_from_slice(host_bytes);
    hs.extend_from_slice(&port.unwrap_or(25565).to_be_bytes());
    write_var_int(&mut hs, 1);
    if stream.write_all(&packet(0x00, &hs)).is_err() || stream.write_all(&packet(0x00, &[])).is_err() {
        return offline;
    }
    // 响应：len(packet) + packet(id=0, varint(jsonLen) + json)
    if read_var_int(&mut stream, 1 << 22).is_err() {
        return offline;
    }
    let body_len = match read_var_int(&mut stream, 1 << 22) {
        Ok(l) => l as usize,
        Err(_) => return offline,
    };
    // 精确读 body_len 字节：read_to_end 会一直吃到连接关闭，异常服务器可喂出 GB 级缓冲
    let mut buf = vec![0u8; body_len];
    if stream.read_exact(&mut buf).is_err() {
        return offline;
    }
    let rtt = std::cmp::max(1, start.elapsed().as_millis() as i64);
    // body: varint id + varint jsonLen + json
    let mut i = 0usize;
    let mut json_len = 0i32;
    let mut shift = 0;
    while i < buf.len() && i < 6 {
        json_len |= ((buf[i] & 0x7f) as i32) << (shift * 7);
        shift += 1;
        i += 1;
        if buf[i - 1] & 0x80 == 0 {
            break;
        }
    }
    let json_end = (i + json_len as usize).min(buf.len());
    let Ok(status) = serde_json::from_slice::<Value>(&buf[i..json_end]) else {
        return offline;
    };
    {
        let motd = motd_text(&status["description"]);
        let motd = if motd.is_empty() { "这个服务器没有介绍".to_string() } else { motd };
        json!({
            "online": true,
            "players": format!("{}/{}",
                status["players"]["online"].as_i64().unwrap_or(0),
                status["players"]["max"].as_i64().unwrap_or(0)),
            "motd": motd,
            "version": status["version"]["name"].as_str().unwrap_or("未知"),
            "latencyMs": rtt
        })
    }
}

// ---------------- servers.dat 同步 ----------------

/// 一个候选 = (servers.dat 路径, 实例 id?, 实例 folder?)；
/// folder 根的 servers.dat → 共享（无实例归属）；versions/<id>/servers.dat → 归属该实例。
fn servers_dat_candidates(
    app: &tauri::AppHandle,
    version_id: Option<&str>,
    folder: Option<&str>,
) -> Vec<(PathBuf, Option<String>, Option<String>)> {
    let settings = crate::settings::get(app);
    let mut out = Vec::new();
    let folders: Vec<String> = match folder {
        Some(f) if !f.is_empty() => vec![f.to_string()],
        _ => settings["folders"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|f| f["path"].as_str().map(str::to_string))
            .collect(),
    };
    for folder_path in folders {
        let base = PathBuf::from(&folder_path);
        // 共享 servers.dat（folder 根）
        let shared = base.join("servers.dat");
        if shared.exists() {
            out.push((shared, version_id.map(str::to_string), Some(folder_path.clone())));
        }
        // 隔离实例的 servers.dat：全实例扫描（指定 version_id 时只看该实例）
        let versions = base.join("versions");
        let Ok(entries) = std::fs::read_dir(&versions) else { continue };
        let mut ids: Vec<String> = entries
            .flatten()
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().to_string_lossy().into_owned().into())
            .collect();
        if let Some(vid) = version_id {
            ids.retain(|id| id == vid);
        }
        for id in ids {
            let dat = versions.join(&id).join("servers.dat");
            if dat.exists() {
                out.push((dat, Some(id), Some(folder_path.clone())));
            }
        }
    }
    out
}

/// servers:syncFromDat → ServerSyncResult（读 .dat 合并 launcher 记录）
pub fn sync_from_dat(app: &tauri::AppHandle, version_id: Option<&str>, folder: Option<&str>) -> Value {
    let mut list = load(app);
    let mut added = 0i64;
    let mut updated = 0i64;
    let mut errors: Vec<String> = Vec::new();
    for (dat, dat_version, dat_folder) in servers_dat_candidates(app, version_id, folder) {
        let Ok(raw) = std::fs::read(&dat) else { continue };
        let Ok(root) = nbt::parse_nbt(&raw) else {
            errors.push(format!("{}：servers.dat 无法解析", dat.display()));
            continue;
        };
        let Some(servers) = nbt::get(&root, "servers").and_then(|v| match v {
            NbtValue::List(l) => Some(&l.values),
            _ => None,
        }) else {
            continue;
        };
        for entry in servers {
            let Some(comp) = nbt::as_compound(entry) else { continue };
            let name = nbt::get(comp, "name").and_then(nbt::as_str).unwrap_or("").to_string();
            let ip = nbt::get(comp, "ip").and_then(nbt::as_str).unwrap_or("").to_string();
            if ip.is_empty() {
                continue;
            }
            let (normalized, host, port) = normalize_address(&ip);
            if let Some(existing) = list.iter_mut().find(|e| e["normalizedAddress"] == json!(normalized)) {
                existing["lastSeenAt"] = json!(chrono::Utc::now().to_rfc3339());
                // 实例级 servers.dat 里的记录归属该实例（未绑定过才补，不覆盖手动绑定）
                if let (Some(vid), true) = (&dat_version, existing["versionId"].as_str().is_none()) {
                    existing["versionId"] = json!(vid);
                    existing["folder"] = json!(dat_folder.clone().unwrap_or_default());
                }
                updated += 1;
            } else {
                let mut item = json!({
                    "id": crate::accounts::uuid_v4(),
                    "name": name,
                    "address": ip,
                    "normalizedAddress": normalized,
                    "host": host,
                    "port": port,
                    "source": "minecraft",
                    "lastSeenAt": chrono::Utc::now().to_rfc3339()
                });
                if let Some(vid) = &dat_version {
                    item["versionId"] = json!(vid);
                    item["folder"] = json!(dat_folder.clone().unwrap_or_default());
                }
                list.push(item);
                added += 1;
            }
        }
    }
    save(app, &list);
    let targets = crate::versions::list_installed(&crate::settings::get(app), false);
    json!({ "list": list, "targets": targets, "added": added, "updated": updated, "errors": errors })
}

/// servers:prepareLaunch → ServerLaunchPreparation
fn prepare_launch(app: &tauri::AppHandle, id: &str, version_id: Option<&str>, folder: Option<&str>) -> Result<Value, String> {
    let list = load(app);
    let entry = list
        .iter()
        .find(|e| e["id"].as_str() == Some(id))
        .ok_or("服务器条目不存在")?;
    let resolved_version = version_id
        .map(str::to_string)
        .filter(|s| !s.is_empty())
        .or_else(|| entry["versionId"].as_str().map(str::to_string))
        .unwrap_or_default();
    if resolved_version.is_empty() {
        return Err("请先在右侧选择要启动的实例".into());
    }
    Ok(json!({
        "serverId": id,
        "versionId": resolved_version,
        "folder": folder.unwrap_or(""),
        "address": entry["address"],
        "minecraftVersion": entry["minecraftVersion"],
        "loader": entry["loader"],
        "loaderVersion": entry["loaderVersion"],
        "directJoin": true
    }))
}

/// 统一入口
pub fn invoke(app: &tauri::AppHandle, channel: &str, args: &[Value]) -> Option<Result<Value, String>> {
    match channel {
        "servers:list" => Some(Ok(Value::Array(load(app)))),
        "servers:add" => Some((|| {
            let name = args.first().and_then(|v| v.as_str()).unwrap_or("").trim();
            let address = args.get(1).and_then(|v| v.as_str()).unwrap_or("").trim();
            if name.is_empty() || address.is_empty() {
                return Err("名称和地址不能为空".into());
            }
            let mut list = load(app);
            let (normalized, host, port) = normalize_address(address);
            list.push(json!({
                "id": crate::accounts::uuid_v4(),
                "name": name,
                "address": address,
                "normalizedAddress": normalized,
                "host": host,
                "port": port,
                "source": "launcher"
            }));
            save(app, &list);
            Ok(Value::Array(list))
        })()),
        "servers:remove" => Some((|| {
            let id = args.first().and_then(|v| v.as_str()).ok_or("缺少 id")?;
            let mut list = load(app);
            list.retain(|e| e["id"].as_str() != Some(id));
            save(app, &list);
            Ok(Value::Array(list))
        })()),
        "servers:edit" => Some((|| {
            let id = args.first().and_then(|v| v.as_str()).ok_or("缺少 id")?;
            let name = args.get(1).and_then(|v| v.as_str()).unwrap_or("").trim();
            let address = args.get(2).and_then(|v| v.as_str()).unwrap_or("").trim();
            let mut list = load(app);
            let Some(entry) = list.iter_mut().find(|e| e["id"].as_str() == Some(id)) else {
                return Err("服务器条目不存在".into());
            };
            if !name.is_empty() {
                entry["name"] = json!(name);
            }
            if !address.is_empty() {
                let (normalized, host, port) = normalize_address(address);
                entry["address"] = json!(address);
                entry["normalizedAddress"] = json!(normalized);
                entry["host"] = json!(host);
                entry["port"] = json!(port);
            }
            save(app, &list);
            Ok(Value::Array(list))
        })()),
        "servers:favorite" => Some((|| {
            let id = args.first().and_then(|v| v.as_str()).ok_or("缺少 id")?;
            let favorite = args.get(1).and_then(|v| v.as_bool()).unwrap_or(false);
            let mut list = load(app);
            if let Some(entry) = list.iter_mut().find(|e| e["id"].as_str() == Some(id)) {
                entry["favorite"] = json!(favorite);
            }
            save(app, &list);
            Ok(Value::Array(list))
        })()),
        "servers:ping" => Some(Ok(ping_server(
            args.first().and_then(|v| v.as_str()).unwrap_or(""),
        ))),
        "servers:bind" => Some((|| {
            let id = args.first().and_then(|v| v.as_str()).ok_or("缺少 id")?;
            let version_id = args.get(1).and_then(|v| v.as_str()).unwrap_or("");
            let folder = args.get(2).and_then(|v| v.as_str());
            let mut list = load(app);
            let Some(entry) = list.iter_mut().find(|e| e["id"].as_str() == Some(id)) else {
                return Err("服务器条目不存在".into());
            };
            if version_id.is_empty() {
                entry.as_object_mut().map(|o| {
                    o.remove("versionId");
                    o.remove("folder");
                });
            } else {
                entry["versionId"] = json!(version_id);
                entry["folder"] = json!(folder.unwrap_or(""));
            }
            save(app, &list);
            Ok(Value::Array(list))
        })()),
        "servers:syncFromDat" => Some(Ok(sync_from_dat(
            app,
            args.first().and_then(|v| v.as_str()),
            args.get(1).and_then(|v| v.as_str()),
        ))),
        "servers:prepareLaunch" => Some(prepare_launch(
            app,
            args.first().and_then(|v| v.as_str()).unwrap_or(""),
            args.get(1).and_then(|v| v.as_str()),
            args.get(2).and_then(|v| v.as_str()),
        )),
        _ => None,
    }
}
