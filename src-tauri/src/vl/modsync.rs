//! 模组一致性清单（对应原版 voxlink/modsync.ts 127 行，LGPL-3.0-only）
//! 上游 AUGUHDAR/VoxLink modsync, revision 721c7fae；KAMUCL 侧改动是"按实例目录取真实哈希 + 不覆盖提交"。
//!
//! 与原版一一对应的四道硬闸：目录不能是链接、单实例 1024 个模组上限、清单 256 条 / 128KiB 上限、
//! 未知模组最多带 64 个。数值一字不改。

use crate::vl::rudp::now_ms;
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// modsync.ts:27 单实例模组上限
pub const MOD_SYNC_LIMIT: usize = 1024;
/// modsync.ts:109 清单条数与字节上限
pub const MOD_SYNC_MANIFEST_MODS: usize = 256;
pub const MOD_SYNC_MANIFEST_BYTES: usize = 128 * 1024;
/// modsync.ts:105/109 未知模组最多带 64 个
pub const MOD_SYNC_UNKNOWN_LIMIT: usize = 64;
/// modsync.ts:73/82 每批 64 个查询
pub const MOD_SYNC_BATCH: usize = 64;
/// modsync.ts:67 单个模组文件大小上限
pub const MOD_SYNC_MAX_SIZE: i64 = 512 * 1024 * 1024;
/// modsync.ts:64 文件名长度上限
pub const MOD_SYNC_NAME_LIMIT: usize = 180;

pub const PROTOCOL_VERSION: &str = "modSync.v1";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalMod {
    pub file_name: String,
    pub sha1: String,
    pub disabled: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModSyncEntry {
    pub project_id: String,
    pub slug: String,
    pub title: String,
    pub version_number: String,
    pub file_name: String,
    pub url: String,
    pub sha1: String,
    pub sha512: String,
    pub size: i64,
    pub loaders: Vec<String>,
    pub game_versions: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModSyncManifest {
    pub protocol_version: String,
    pub loader: String,
    pub mc_version: String,
    pub mods: Vec<ModSyncEntry>,
    pub unknown_mods: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModSyncStatus {
    Installed,
    Disabled,
    Conflict,
    Unresolved,
    Missing,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModSyncRow {
    pub entry: ModSyncEntry,
    pub status: ModSyncStatus,
    pub reason: String,
}

fn ends_with_ignore_case(name: &str, suffix: &str) -> bool {
    name.len() >= suffix.len() && name[name.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
}

/// Windows 上取 Node `fstat/lstat` 同款的 ino/size/mtime/ctime：
/// libuv 在 Windows 也是走 GetFileInformationByHandle，字段一一对应。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct FileStamp {
    ino: u128,
    size: i64,
    mtime_ns: i128,
    ctime_ns: i128,
}

#[cfg(windows)]
fn stamp_of(file: &File) -> Result<FileStamp, String> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::Storage::FileSystem::{GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION};
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle() as *mut _, &mut info) };
    if ok == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    // FILETIME 是 1601-01-01 起的 100ns 计数，Node 换算成 ns 后同样只做相等比较
    let to_i64 = |time: &FILETIME| ((time.dwHighDateTime as i64) << 32) | time.dwLowDateTime as i64;
    Ok(FileStamp {
        ino: ((info.nFileIndexHigh as u128) << 32) | info.nFileIndexLow as u128,
        size: ((info.nFileSizeHigh as i64) << 32) | info.nFileSizeLow as i64,
        mtime_ns: to_i64(&info.ftLastWriteTime) as i128 * 100,
        ctime_ns: to_i64(&info.ftCreationTime) as i128 * 100,
    })
}

#[cfg(not(windows))]
fn stamp_of(file: &File) -> Result<FileStamp, String> {
    use std::os::unix::fs::MetadataExt;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    let ns = |sec: u64, nsec: u32| (sec as i128) * 1_000_000_000 + nsec as i128;
    Ok(FileStamp { ino: meta.ino() as u128, size: meta.size() as i64, mtime_ns: ns(meta.mtime(), meta.mtime_nsec()), ctime_ns: ns(meta.ctime(), meta.ctime_nsec()) })
}

/// 对应 scanModHashes：先 lstat 目录，逐个 .jar 算 SHA-1，哈希前后再比对文件身份
pub fn scan_mod_hashes(dir: &Path, cancelled: &AtomicBool, include_disabled: bool) -> Result<Vec<LocalMod>, String> {
    let entries = match dir.symlink_metadata() {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.to_string()),
        Ok(meta) => {
            if !meta.is_dir() || meta.file_type().is_symlink() {
                return Err("模组目录不能是链接".into());
            }
            match std::fs::read_dir(dir) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
                Err(error) => return Err(error.to_string()),
                Ok(list) => list,
            }
        }
    };
    let mut result: Vec<LocalMod> = Vec::new();
    for entry in entries {
        if cancelled.load(Ordering::SeqCst) {
            return Err("模组扫描已取消".into());
        }
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().to_string();
        let file_type = entry.file_type().map_err(|e| e.to_string())?;
        if !file_type.is_file() || !(ends_with_ignore_case(&name, ".jar") || (include_disabled && ends_with_ignore_case(&name, ".jar.disabled"))) {
            continue;
        }
        if result.len() >= MOD_SYNC_LIMIT {
            return Err("模组数量超过同步上限（1024），请先整理实例".into());
        }
        let full = dir.join(&name);
        let mut file = File::open(&full).map_err(|e| e.to_string())?;
        let before = stamp_of(&file)?;
        let mut hash = Sha1::new();
        let mut buf = vec![0u8; 65536];
        loop {
            if cancelled.load(Ordering::SeqCst) {
                return Err("模组扫描已取消".into());
            }
            let size = file.read(&mut buf).map_err(|e| e.to_string())?;
            if size == 0 {
                break;
            }
            hash.update(&buf[..size]);
        }
        drop(file);
        // 原版用 lstat 收尾：中途被换成链接也要报错
        let link = full.symlink_metadata().map_err(|e| e.to_string())?;
        let after = match File::open(&full) {
            Ok(handle) => stamp_of(&handle),
            Err(error) => return Err(error.to_string()),
        }?;
        if link.file_type().is_symlink() || before != after {
            return Err(format!("模组正在变化，请稍后重试：{}", name));
        }
        result.push(LocalMod { file_name: name.clone(), sha1: hex::encode(hash.finalize()), disabled: ends_with_ignore_case(&name, ".disabled") });
    }
    Ok(result)
}

/// 对应 modrinthRequest：429 按 Retry-After 退避，5xx 与网络错误最多三次，4xx 直接放弃
pub fn modrinth_request(client: &reqwest::blocking::Client, app_version: &str, route: &str, cancelled: &AtomicBool, body: Option<&serde_json::Value>) -> Result<serde_json::Value, String> {
    let mut attempt = 0u32;
    loop {
        if cancelled.load(Ordering::SeqCst) {
            return Err("Modrinth 请求已取消".into());
        }
        let mut retry: i64 = if attempt == 0 { 1000 } else { 3000 };
        let mut request = match body {
            None => client.get(format!("https://api.modrinth.com/v2{}", route)),
            Some(value) => client.post(format!("https://api.modrinth.com/v2{}", route)).json(value),
        };
        request = request
            .timeout(Duration::from_secs(15))
            .header("User-Agent", format!("KAMUCL-App/{}", app_version))
            .header("Content-Type", "application/json");
        match request.send() {
            Ok(response) => {
                let status = response.status().as_u16();
                if (200..300).contains(&status) {
                    return response.json::<serde_json::Value>().map_err(|e| e.to_string());
                }
                let header = response.headers().get("retry-after").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
                drop(response);
                if status == 429 {
                    retry = retry_after_delay(&header, now_ms() as i64, retry);
                } else if status < 500 {
                    return Err(format!("Modrinth HTTP {}", status));
                }
                if attempt >= 2 {
                    return Err(format!("Modrinth HTTP {}", status));
                }
            }
            Err(error) => {
                if cancelled.load(Ordering::SeqCst) {
                    return Err("Modrinth 请求已取消".into());
                }
                if attempt >= 2 {
                    return Err(error.to_string());
                }
            }
        }
        if !sleep_cancellable(Duration::from_millis(retry.max(1) as u64), cancelled) {
            return Err("Modrinth 请求已取消".into());
        }
        attempt += 1;
    }
}

/// JS `Math.max(1000, Number(v)*1000 | (Date.parse(v)-now || retry))` 的等价实现
fn retry_after_delay(header: &str, now_ms: i64, fallback: i64) -> i64 {
    if !header.is_empty() {
        if let Ok(value) = header.parse::<f64>() {
            if value.is_finite() {
                return (value * 1000.0).max(1000.0) as i64;
            }
        } else if let Ok(at) = chrono::DateTime::parse_from_rfc2822(header) {
            let delta = at.timestamp_millis() - now_ms;
            if delta != 0 {
                return delta.max(1000);
            }
        }
    }
    fallback.max(1000)
}

fn sleep_cancellable(total: Duration, cancelled: &AtomicBool) -> bool {
    let end = Instant::now() + total;
    loop {
        if cancelled.load(Ordering::SeqCst) {
            return false;
        }
        let left = end.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return true;
        }
        std::thread::sleep(left.min(Duration::from_millis(20)));
    }
}

/// modsync.ts:60 serverOnly
fn server_only(project: Option<&serde_json::Value>) -> bool {
    let Some(project) = project else { return false };
    project.get("client_side").and_then(|v| v.as_str()) == Some("unsupported") || project.get("environment").and_then(|v| v.as_str()) == Some("server_only")
}

/// 对应 safeModEntry：只允许 Modrinth CDN 的 https 直链 + 完整校验信息
pub fn safe_mod_entry(entry: &ModSyncEntry) -> bool {
    let Ok(url) = url::Url::parse(&entry.url) else { return false };
    if entry.project_id.is_empty()
        || entry.file_name.chars().count() > MOD_SYNC_NAME_LIMIT
        || !ends_with_ignore_case(&entry.file_name, ".jar")
        || !is_safe_file_name(&entry.file_name)
        || !is_reserved_free(&entry.file_name)
    {
        return false;
    }
    if url.scheme() != "https" || url.host_str().map(|host| host.to_ascii_lowercase()) != Some("cdn.modrinth.com".to_string()) {
        return false;
    }
    if !url.username().is_empty() || url.password().is_some() || matches!(url.port(), Some(port) if port != 443) {
        return false;
    }
    is_hex(&entry.sha1, 40) && is_hex(&entry.sha512, 128) && entry.size > 0 && entry.size <= MOD_SYNC_MAX_SIZE
}

/// modsync.ts:65 Windows 非法字符
fn is_safe_file_name(name: &str) -> bool {
    !name.contains(['\\', '/', ':', '*', '?', '"', '<', '>', '|']) && !name.chars().any(|c| (c as u32) < 0x20)
}

/// modsync.ts:65 保留设备名：`^(con|prn|aux|nul|com[1-9]|lpt[1-9])\.`
fn is_reserved_free(name: &str) -> bool {
    let lower = name.to_lowercase();
    let Some(head) = lower.split('.').next() else { return true };
    let followed_by_dot = lower.len() > head.len() && lower.as_bytes()[head.len()] == b'.';
    if !followed_by_dot {
        return true;
    }
    if ["con", "prn", "aux", "nul"].contains(&head) {
        return false;
    }
    if head.len() == 4 && (head.starts_with("com") || head.starts_with("lpt")) {
        return !matches!(head[3..].parse::<u32>(), Ok(digit) if (1..=9).contains(&digit))
    }
    true
}

fn is_hex(value: &str, length: usize) -> bool {
    value.len() == length && value.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Modrinth 查询通道：与原版 `MRRequest` 同形，测试可以注入假响应
pub type MRRequest<'a> = &'a dyn Fn(&str, Option<&serde_json::Value>) -> Result<serde_json::Value, String>;

/// JS Map 的语义：已有键覆盖值但保持原位置
fn map_put(list: &mut Vec<(String, serde_json::Value)>, key: String, value: serde_json::Value) {
    match list.iter().position(|(known, _)| *known == key) {
        Some(at) => list[at].1 = value,
        None => list.push((key, value)),
    }
}

fn push_unique(list: &mut Vec<String>, value: String) {
    if !list.iter().any(|seen| *seen == value) {
        list.push(value);
    }
}

/// 对应 buildModManifests：按本地 SHA-1 反查版本与项目，再补必需依赖闭包
pub fn build_mod_manifests(local: &[LocalMod], loader: &str, mc_version: &str, request: MRRequest, cancelled: &AtomicBool) -> Result<(ModSyncManifest, ModSyncManifest), String> {
    let mut versions: Vec<(String, serde_json::Value)> = Vec::new();
    let mut projects: Vec<(String, serde_json::Value)> = Vec::new();
    let mut unknown: Vec<String> = Vec::new();
    let mut start = 0usize;
    while start < local.len() {
        if cancelled.load(Ordering::SeqCst) {
            return Err("模组清单构建已取消".into());
        }
        let block = &local[start..(start + MOD_SYNC_BATCH).min(local.len())];
        start += MOD_SYNC_BATCH;
        let hashes: Vec<&str> = block.iter().map(|f| f.sha1.as_str()).collect();
        let payload = serde_json::json!({ "hashes": hashes, "algorithm": "sha1" });
        match request("/version_files", Some(&payload)) {
            Ok(data) => {
                for module in block {
                    let found = data.get(&module.sha1).filter(|v| !v.is_null());
                    let hit = found.filter(|value| value.get("project_id").and_then(|v| v.as_str()).unwrap_or("") != "" && value.get("files").map(|files| files.is_array()).unwrap_or(false));
                    match hit {
                        Some(value) => map_put(&mut versions, module.sha1.clone(), value.clone()),
                        None => push_unique(&mut unknown, module.file_name.clone()),
                    }
                }
            }
            Err(_) => {
                if cancelled.load(Ordering::SeqCst) {
                    return Err("模组清单构建已取消".into());
                }
                for module in block {
                    push_unique(&mut unknown, module.file_name.clone());
                }
            }
        }
    }
    // 项目列表：自身 + 被标为 required 的依赖
    let mut ids: Vec<String> = Vec::new();
    for (_, value) in &versions {
        if let Some(id) = value.get("project_id").and_then(|v| v.as_str()).filter(|id| !id.is_empty()) {
            push_unique(&mut ids, id.to_string());
        }
        for dep in value.get("dependencies").and_then(|v| v.as_array()).into_iter().flatten() {
            if dep.get("dependency_type").and_then(|v| v.as_str()) == Some("required") {
                if let Some(id) = dep.get("project_id").and_then(|v| v.as_str()).filter(|id| !id.is_empty()) {
                    push_unique(&mut ids, id.to_string());
                }
            }
        }
    }
    let mut start = 0usize;
    while start < ids.len() {
        if cancelled.load(Ordering::SeqCst) {
            return Err("模组清单构建已取消".into());
        }
        let chunk = &ids[start..(start + MOD_SYNC_BATCH).min(ids.len())];
        start += MOD_SYNC_BATCH;
        let route = format!("/projects?ids={}", percent_encode(&serde_json::to_string(chunk).unwrap_or_default()));
        match request(&route, None) {
            Ok(data) => {
                for project in data.as_array().into_iter().flatten() {
                    if let Some(id) = project.get("id").and_then(|v| v.as_str()) {
                        map_put(&mut projects, id.to_string(), project.clone());
                    }
                }
            }
            Err(_) => {
                if cancelled.load(Ordering::SeqCst) {
                    return Err("模组清单构建已取消".into());
                }
            }
        }
    }
    let project_of = |id: &str| projects.iter().find(|(known, _)| known == id).map(|(_, value)| value);
    let version_of = |sha: &str| versions.iter().find(|(known, _)| known == sha).map(|(_, value)| value);

    let mut entries: Vec<(String, ModSyncEntry)> = Vec::new();
    let mut by_project: Vec<(String, serde_json::Value)> = Vec::new();
    for module in local {
        let Some(v) = version_of(&module.sha1) else { continue };
        let project_id = v.get("project_id").and_then(|x| x.as_str()).unwrap_or("").to_string();
        let project = project_of(&project_id);
        if server_only(project) {
            continue;
        }
        let file = v
            .get("files")
            .and_then(|files| files.as_array())
            .and_then(|files| {
                files.iter().find(|f| {
                    f.get("hashes")
                        .and_then(|hashes| hashes.get("sha1"))
                        .and_then(|sha| sha.as_str())
                        .map(|sha| sha.to_lowercase() == module.sha1)
                        .unwrap_or(false)
                })
            })
            .cloned();
        let (Some(project), Some(file)) = (project, file) else {
            push_unique(&mut unknown, module.file_name.clone());
            continue;
        };
        let entry = ModSyncEntry {
            project_id: project_id.clone(),
            slug: project.get("slug").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            title: project.get("title").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            version_number: v.get("version_number").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            file_name: file.get("filename").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            url: file.get("url").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            sha1: module.sha1.clone(),
            sha512: file.get("hashes").and_then(|hashes| hashes.get("sha512")).and_then(|x| x.as_str()).unwrap_or("").to_string(),
            size: file.get("size").and_then(|x| x.as_i64()).unwrap_or(0),
            loaders: string_array(v.get("loaders")),
            game_versions: string_array(v.get("game_versions")),
        };
        if !safe_mod_entry(&entry) {
            push_unique(&mut unknown, module.file_name.clone());
            continue;
        }
        match entries.iter().position(|(id, _)| *id == project_id) {
            Some(at) => entries[at].1 = entry,
            None => entries.push((project_id.clone(), entry)),
        }
        map_put(&mut by_project, project_id, v.clone());
    }
    // 必装闭包：从本地已识别的项目出发，沿 required 依赖广度扩展
    let mut required: Vec<String> = Vec::new();
    let mut queue: Vec<String> = entries
        .iter()
        .filter(|(id, _)| !["optional", "unsupported"].contains(&project_of(id).and_then(|p| p.get("client_side")).and_then(|v| v.as_str()).unwrap_or("")))
        .map(|(id, _)| id.clone())
        .collect();
    let mut index = 0usize;
    while index < queue.len() {
        let id = queue[index].clone();
        index += 1;
        if required.contains(&id) || server_only(project_of(&id)) {
            continue;
        }
        required.push(id.clone());
        let deps = by_project.iter().find(|(known, _)| *known == id).map(|(_, value)| value).and_then(required_deps).unwrap_or_default();
        for dep in deps {
            if !required.contains(&dep) {
                push_unique(&mut queue, dep);
            }
        }
    }
    let manifest = |all: bool| -> ModSyncManifest {
        let mut result = ModSyncManifest {
            protocol_version: PROTOCOL_VERSION.into(),
            loader: loader.into(),
            mc_version: mc_version.into(),
            mods: Vec::new(),
            unknown_mods: unknown.iter().take(MOD_SYNC_UNKNOWN_LIMIT).cloned().collect(),
        };
        for (id, entry) in &entries {
            if !all && !required.contains(id) {
                continue;
            }
            result.mods.push(entry.clone());
            let bytes = serde_json::to_string(&result).map(|text| text.len()).unwrap_or(0);
            if result.mods.len() > MOD_SYNC_MANIFEST_MODS || bytes > MOD_SYNC_MANIFEST_BYTES {
                result.mods.pop();
                if result.unknown_mods.len() < MOD_SYNC_UNKNOWN_LIMIT {
                    result.unknown_mods.push(entry.file_name.clone());
                }
            }
        }
        result
    };
    Ok((manifest(false), manifest(true)))
}

fn string_array(value: Option<&serde_json::Value>) -> Vec<String> {
    value
        .and_then(|list| list.as_array())
        .map(|list| list.iter().filter_map(|item| item.as_str().map(|text| text.to_string())).collect())
        .unwrap_or_default()
}

fn required_deps(version: &serde_json::Value) -> Option<Vec<String>> {
    let deps = version.get("dependencies")?.as_array()?;
    Some(
        deps.iter()
            .filter(|dep| dep.get("dependency_type").and_then(|v| v.as_str()) == Some("required"))
            .filter_map(|dep| dep.get("project_id").and_then(|v| v.as_str()).filter(|id| !id.is_empty()).map(|id| id.to_string()))
            .collect(),
    )
}

/// 对应 normalizeModName
pub fn normalize_mod_name(name: &str) -> String {
    let lowered = name.to_lowercase();
    let stripped = lowered.strip_suffix(".disabled").unwrap_or(&lowered);
    let stripped = stripped.strip_suffix(".jar").unwrap_or(stripped);
    stripped
        .split(|c: char| !c.is_ascii_alphabetic())
        .filter(|word| !word.is_empty() && !["fabric", "forge", "neoforge", "quilt", "fml", "mc", "all", "for", "the", "with"].contains(&word.as_ref()))
        .collect::<Vec<_>>()
        .join("-")
}

/// 对应 diffMods：五种状态与文案一字不改
pub fn diff_mods(manifest: &ModSyncManifest, local: &[LocalMod], loader: &str, mc_version: &str) -> Vec<ModSyncRow> {
    manifest
        .mods
        .iter()
        .map(|entry| {
            let matches: Vec<&LocalMod> = local.iter().filter(|f| f.sha1.to_lowercase() == entry.sha1.to_lowercase()).collect();
            if matches.iter().any(|f| !f.disabled) {
                return ModSyncRow { entry: entry.clone(), status: ModSyncStatus::Installed, reason: "已安装".into() };
            }
            if !matches.is_empty() {
                return ModSyncRow { entry: entry.clone(), status: ModSyncStatus::Disabled, reason: "已安装但被禁用，请手动启用".into() };
            }
            let exists = local.iter().any(|f| {
                let lowered = f.file_name.to_lowercase();
                let stripped = lowered.strip_suffix(".disabled").unwrap_or(&lowered);
                stripped == entry.file_name.to_lowercase() || normalize_mod_name(&f.file_name) == normalize_mod_name(&entry.file_name)
            });
            if exists {
                return ModSyncRow { entry: entry.clone(), status: ModSyncStatus::Conflict, reason: "已存在其他版本，请手动处理；不会覆盖或移动原文件".into() };
            }
            if !safe_mod_entry(entry) || !entry.loaders.iter().any(|known| known == loader) || !entry.game_versions.iter().any(|known| known == mc_version) || manifest.loader != loader || manifest.mc_version != mc_version {
                return ModSyncRow { entry: entry.clone(), status: ModSyncStatus::Unresolved, reason: "与所选实例不兼容，或缺少可靠下载校验信息".into() };
            }
            ModSyncRow { entry: entry.clone(), status: ModSyncStatus::Missing, reason: "可下载".into() }
        })
        .collect()
}

/// `encodeURIComponent(JSON.stringify(ids))`
fn percent_encode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.as_bytes() {
        let c = *byte as char;
        if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '!' | '~' | '*' | '\'' | '(' | ')') {
            out.push(c);
        } else {
            out.push_str(&format!("%{:02X}", byte));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;

    const SHA_A: &str = "a9993e364706816aba3e25717850c26c9cd0d89d";
    const SHA_B: &str = "9a1fc0d519d6195f1d8b1f922d5223b4cd6f8d8e";

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wuhuicl-modsync-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn entry(index: u8, project: &str, name: &str) -> ModSyncEntry {
        ModSyncEntry {
            project_id: project.into(),
            slug: name.into(),
            title: name.into(),
            version_number: "1.0".into(),
            file_name: name.into(),
            url: format!("https://cdn.modrinth.com/data/{}/{}.jar", project, name),
            sha1: format!("{:02x}{:038x}", index, index),
            sha512: format!("{:0128x}", index),
            size: 1024,
            loaders: vec!["fabric".into()],
            game_versions: vec!["1.20.1".into()],
        }
    }

    fn local(sha: &str, name: &str, disabled: bool) -> LocalMod {
        LocalMod { file_name: name.into(), sha1: sha.into(), disabled }
    }
    #[test]
    fn scan_hashes_jars_and_skips_the_rest() {
        let dir = temp_dir("scan");
        std::fs::write(dir.join("a.jar"), b"abc").unwrap();
        std::fs::write(dir.join("notes.txt"), b"abc").unwrap();
        std::fs::write(dir.join("c.jar.disabled"), b"abc").unwrap();
        std::fs::write(dir.join("D.JAR"), b"abc").unwrap();
        let cancelled = AtomicBool::new(false);
        let list = scan_mod_hashes(&dir, &cancelled, false).unwrap();
        let names: Vec<&str> = list.iter().map(|m| m.file_name.as_str()).collect();
        assert!(names.contains(&"a.jar") && names.contains(&"D.JAR"), ".jar 与 .JAR 都要收");
        assert!(!names.contains(&"notes.txt") && !names.contains(&"c.jar.disabled"));
        let found = list.iter().find(|m| m.file_name == "a.jar").unwrap();
        assert_eq!(found.sha1, SHA_A, "SHA-1 必须和标准向量一致");
        assert!(!found.disabled);
        let with_disabled = scan_mod_hashes(&dir, &cancelled, true).unwrap();
        let item = with_disabled.iter().find(|m| m.file_name == "c.jar.disabled").unwrap();
        assert!(item.disabled, ".jar.disabled 要标记为已禁用");
        assert_eq!(item.sha1, SHA_A);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn missing_dir_reads_as_empty_but_a_file_is_rejected() {
        let cancelled = AtomicBool::new(false);
        assert!(scan_mod_hashes(Path::new("D:\\不存在的模组目录"), &cancelled, false).unwrap().is_empty());
        let dir = temp_dir("notdir");
        let file = dir.join("a.jar");
        std::fs::write(&file, b"abc").unwrap();
        assert_eq!(scan_mod_hashes(&file, &cancelled, false).unwrap_err(), "模组目录不能是链接");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn scan_stops_at_the_1024_module_cap() {
        let dir = temp_dir("cap");
        for index in 0..1025 {
            std::fs::write(dir.join(format!("m{:04}.jar", index)), b"abc").unwrap();
        }
        let cancelled = AtomicBool::new(false);
        assert_eq!(
            scan_mod_hashes(&dir, &cancelled, false).unwrap_err(),
            "模组数量超过同步上限（1024），请先整理实例"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn cancelled_scan_reports_before_any_hashing() {
        let dir = temp_dir("cancel");
        std::fs::write(dir.join("a.jar"), b"abc").unwrap();
        let cancelled = AtomicBool::new(true);
        assert_eq!(scan_mod_hashes(&dir, &cancelled, false).unwrap_err(), "模组扫描已取消");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn normalize_mod_name_drops_loaders_and_stopwords() {
        assert_eq!(normalize_mod_name("Fabric API-0.92.jar"), "api");
        assert_eq!(normalize_mod_name("Sodium.forge.1.20.jar.disabled"), "sodium");
        assert_eq!(normalize_mod_name("sodium-0.4.2.jar"), "sodium");
        assert_eq!(normalize_mod_name("mod for the with all mc thing.jar"), "mod-thing");
    }

    #[test]
    fn safe_entry_accepts_only_verified_modrinth_links() {
        assert!(safe_mod_entry(&entry(1, "p1", "a.jar")));
        let mut case = entry(1, "p1", "a.jar");
        case.url = "http://cdn.modrinth.com/data/p1/a.jar".into();
        assert!(!safe_mod_entry(&case), "必须是 https");
        case = entry(1, "p1", "a.jar");
        case.url = "https://cdn.modrinth.com.evil.com/a.jar".into();
        assert!(!safe_mod_entry(&case), "域名必须正好是 cdn.modrinth.com");
        case = entry(1, "p1", "a.jar");
        case.url = "https://cdn.modrinth.com:8443/data/p1/a.jar".into();
        assert!(!safe_mod_entry(&case), "只允许默认端口");
        case = entry(1, "p1", "a.jar");
        case.url = "https://user:pw@cdn.modrinth.com/data/p1/a.jar".into();
        assert!(!safe_mod_entry(&case), "URL 不能带凭证");
        case = entry(1, "p1", "a.jar");
        case.sha1 = "abc".into();
        assert!(!safe_mod_entry(&case), "SHA-1 必须是 40 位十六进制");
        case = entry(1, "p1", "a.jar");
        case.sha512 = case.sha512[..127].into();
        assert!(!safe_mod_entry(&case), "SHA-512 必须是 128 位");
        case = entry(1, "p1", "a.jar");
        case.size = 0;
        assert!(!safe_mod_entry(&case));
        case = entry(1, "p1", "a.jar");
        case.size = MOD_SYNC_MAX_SIZE + 1;
        assert!(!safe_mod_entry(&case), "超过 512MiB 不收");
        case = entry(1, "p1", "a.jar");
        case.project_id = String::new();
        assert!(!safe_mod_entry(&case));
        case = entry(1, "p1", &format!("{}.jar", "n".repeat(181)));
        assert!(!safe_mod_entry(&case), "文件名超过 180 不收");
    }

    #[test]
    fn safe_entry_rejects_windows_unsafe_names() {
        for name in ["con.jar", "PRN.txt.jar", "aux.jar", "nul.jar", "com1.jar", "LPT9.jar"] {
            let mut case = entry(1, "p1", name);
            case.file_name = name.into();
            assert!(!safe_mod_entry(&case), "{} 是保留设备名", name);
        }
        // 非保留名与没有点号的名字必须放行，不能误伤
        for name in ["com10.jar", "console.jar", "a:b.jar", "a\\b.jar", "a?b.jar"] {
            let mut case = entry(1, "p1", "a.jar");
            case.file_name = name.into();
            let colon_or_slash = name.contains([':', '\\', '?']);
            assert_eq!(safe_mod_entry(&case), !colon_or_slash, "{} 应按 Windows 字符规则判定", name);
        }
    }

    #[test]
    fn diff_maps_the_five_statuses_with_original_wording() {
        let wanted = entry(7, "p7", "seven.jar");
        let manifest = ModSyncManifest {
            protocol_version: PROTOCOL_VERSION.into(),
            loader: "fabric".into(),
            mc_version: "1.20.1".into(),
            mods: vec![wanted.clone()],
            unknown_mods: Vec::new(),
        };
        let rows = |local: Vec<LocalMod>| diff_mods(&manifest, &local, "fabric", "1.20.1").remove(0);
        assert_eq!(rows(vec![local(&wanted.sha1, "seven.jar", false)]).status, ModSyncStatus::Installed);
        assert_eq!(rows(vec![local(&wanted.sha1, "seven.jar.disabled", true)]).reason, "已安装但被禁用，请手动启用");
        let conflict = rows(vec![local(SHA_B, "seven-0.9.jar", false)]);
        assert_eq!(conflict.status, ModSyncStatus::Conflict);
        assert_eq!(conflict.reason, "已存在其他版本，请手动处理；不会覆盖或移动原文件");
        let missing = rows(Vec::new());
        assert_eq!(missing.status, ModSyncStatus::Missing);
        assert_eq!(missing.reason, "可下载");
        let mut foreign = wanted.clone();
        foreign.game_versions = vec!["1.21".into()];
        let manifest2 = ModSyncManifest { mods: vec![foreign], ..manifest.clone() };
        let unresolved = diff_mods(&manifest2, &[], "fabric", "1.20.1").remove(0);
        assert_eq!(unresolved.status, ModSyncStatus::Unresolved);
        assert_eq!(unresolved.reason, "与所选实例不兼容，或缺少可靠下载校验信息");
        // 清单自身的 loader/mcVersion 与所选实例不符也要判不兼容
        let mismatched = ModSyncManifest { loader: "forge".into(), ..manifest };
        assert_eq!(diff_mods(&mismatched, &[], "fabric", "1.20.1").remove(0).status, ModSyncStatus::Unresolved);
    }

    fn fake_modrinth(hits: Vec<(String, serde_json::Value)>, projects: Vec<serde_json::Value>) -> impl Fn(&str, Option<&serde_json::Value>) -> Result<serde_json::Value, String> {
        move |route, _| {
            if route.starts_with("/version_files") {
                let mut map = serde_json::Map::new();
                for (sha, value) in &hits {
                    map.insert(sha.clone(), value.clone());
                }
                return Ok(serde_json::Value::Object(map));
            }
            Ok(serde_json::Value::Array(projects.clone()))
        }
    }

    fn version(project: &str, sha: &str, file: &str, deps: Vec<(&str, &str)>) -> serde_json::Value {
        json!({
            "project_id": project,
            "version_number": "1.0",
            "loaders": ["fabric"],
            "game_versions": ["1.20.1"],
            "dependencies": deps.iter().map(|(id, kind)| json!({"project_id": id, "dependency_type": kind})).collect::<Vec<_>>(),
            "files": [{ "filename": file, "url": format!("https://cdn.modrinth.com/data/{}/{}", project, file), "size": 1024,
                         "hashes": { "sha1": sha, "sha512": format!("{:0128x}", 9u32) } }]
        })
    }

    fn project(id: &str, client_side: &str) -> serde_json::Value {
        json!({ "id": id, "slug": id, "title": id, "client_side": client_side })
    }

    #[test]
    fn required_dependency_closure_rescues_optional_mod() {
        let wanted = entry(3, "p3", "three.jar");
        let dependency = entry(4, "p4", "four.jar");
        let hits = vec![
            (wanted.sha1.clone(), version("p3", &wanted.sha1, "three.jar", vec![("p4", "required"), ("p9", "optional")])),
            (dependency.sha1.clone(), version("p4", &dependency.sha1, "four.jar", Vec::new())),
        ];
        // p4 自己标成 optional，只因为被 p3 依赖才进必装清单；p9 是可选依赖，不该被带上
        let projects = vec![project("p3", "required"), project("p4", "optional"), project("p9", "required")];
        let request = fake_modrinth(hits, projects);
        let cancelled = AtomicBool::new(false);
        let (required, all) = build_mod_manifests(
            &[local_clone(&wanted), local_clone(&dependency)],
            "fabric",
            "1.20.1",
            &request,
            &cancelled,
        )
        .unwrap();
        assert_eq!(required.protocol_version, "modSync.v1");
        assert_eq!(required.loader, "fabric");
        assert_eq!(required.mc_version, "1.20.1");
        assert_eq!(required.mods.iter().map(|m| m.project_id.as_str()).collect::<Vec<_>>(), vec!["p3", "p4"]);
        assert_eq!(all.mods.len(), 2);
        assert_eq!(required.unknown_mods, Vec::<String>::new());
    }

    #[test]
    fn optional_local_mod_is_only_shipped_in_the_full_manifest() {
        let wanted = entry(3, "p3", "three.jar");
        let other = entry(4, "p4", "four.jar");
        let hits = vec![
            (wanted.sha1.clone(), version("p3", &wanted.sha1, "three.jar", Vec::new())),
            (other.sha1.clone(), version("p4", &other.sha1, "four.jar", Vec::new())),
        ];
        let projects = vec![project("p3", "optional"), project("p4", "required")];
        let request = fake_modrinth(hits, projects);
        let cancelled = AtomicBool::new(false);
        let (required, all) = build_mod_manifests(&[local_clone(&wanted), local_clone(&other)], "fabric", "1.20.1", &request, &cancelled).unwrap();
        assert_eq!(required.mods.iter().map(|m| m.project_id.as_str()).collect::<Vec<_>>(), vec!["p4"]);
        assert_eq!(all.mods.len(), 2, "全部清单要带上可选模组");
    }

    fn local_clone(entry: &ModSyncEntry) -> LocalMod {
        local(&entry.sha1, &entry.file_name, false)
    }

    #[test]
    fn server_only_and_unmatched_projects_are_not_shipped() {
        let wanted = entry(5, "p5", "five.jar");
        let hits = vec![(wanted.sha1.clone(), version("p5", &wanted.sha1, "five.jar", Vec::new()))];
        let projects = vec![project("p5", "required")];
        let request = fake_modrinth(hits.clone(), projects);
        let cancelled = AtomicBool::new(false);
        let (required, _) = build_mod_manifests(&[local_clone(&wanted)], "fabric", "1.20.1", &request, &cancelled).unwrap();
        assert_eq!(required.mods.len(), 1);
        // 项目被标成 server_only：整条不进清单，也不进 unknown
        let projects = vec![project("p5", "unsupported")];
        let request = fake_modrinth(hits, projects);
        let (required, all) = build_mod_manifests(&[local_clone(&wanted)], "fabric", "1.20.1", &request, &cancelled).unwrap();
        assert!(required.mods.is_empty() && all.mods.is_empty());
        assert!(required.unknown_mods.is_empty(), "服务端专用模组不算未知");
    }

    #[test]
    fn failed_lookup_lands_in_unknown_mods() {
        let wanted = entry(6, "p6", "six.jar");
        let request = |_: &str, _: Option<&serde_json::Value>| Err("Modrinth HTTP 500".to_string());
        let cancelled = AtomicBool::new(false);
        let (required, _) = build_mod_manifests(&[local_clone(&wanted), local(SHA_B, "other.jar", false)], "fabric", "1.20.1", &request, &cancelled).unwrap();
        assert_eq!(required.unknown_mods, vec!["six.jar", "other.jar"]);
        // 查到了但没有 project_id 也算未知
        let hits = vec![("nope".to_string(), json!({ "files": [] }))];
        let request = fake_modrinth(hits, Vec::new());
        let (required, _) = build_mod_manifests(&[local_clone(&wanted)], "fabric", "1.20.1", &request, &cancelled).unwrap();
        assert_eq!(required.unknown_mods, vec!["six.jar"]);
    }

    #[test]
    fn manifest_is_capped_at_256_entries_and_128_kib() {
        let locals: Vec<LocalMod> = (1..=258usize).map(|index| LocalMod { file_name: format!("m{:03}.jar", index), sha1: format!("{:040x}", index), disabled: false }).collect();
        let hits: Vec<(String, serde_json::Value)> = locals
            .iter()
            .enumerate()
            .map(|(position, module)| (module.sha1.clone(), version(&format!("p{}", position), &module.sha1, &module.file_name, Vec::new())))
            .collect();
        let projects: Vec<serde_json::Value> = (0..258).map(|position| project(&format!("p{}", position), "required")).collect();
        let request = fake_modrinth(hits, projects);
        let cancelled = AtomicBool::new(false);
        let (_, all) = build_mod_manifests(&locals, "fabric", "1.20.1", &request, &cancelled).unwrap();
        assert_eq!(all.mods.len(), MOD_SYNC_MANIFEST_MODS, "超过 256 条要截断");
        assert_eq!(all.unknown_mods.len(), 2, "截掉的两条进 unknownMods");
        let bytes = serde_json::to_string(&all).unwrap().len();
        assert!(bytes <= MOD_SYNC_MANIFEST_BYTES, "清单序列化后不能超 128KiB，实际 {}", bytes);
    }

    #[test]
    fn project_query_encodes_ids_like_encodeuricomponent() {
        let seen = std::cell::RefCell::new(Vec::<String>::new());
        let request = |route: &str, _: Option<&serde_json::Value>| -> Result<serde_json::Value, String> {
            seen.borrow_mut().push(route.to_string());
            if route.starts_with("/version_files") {
                let mut map = serde_json::Map::new();
                map.insert(SHA_A.to_string(), version("pX", SHA_A, "a.jar", Vec::new()));
                return Ok(serde_json::Value::Object(map));
            }
            Ok(json!([project("pX", "required")]))
        };
        let cancelled = AtomicBool::new(false);
        build_mod_manifests(&[local(SHA_A, "a.jar", false)], "fabric", "1.20.1", &request, &cancelled).unwrap();
        let seen = seen.borrow();
        assert_eq!(seen[0], "/version_files");
        assert_eq!(seen[1], "/projects?ids=%5B%22pX%22%5D", "ids 要按 encodeURIComponent(JSON) 编码");
    }
}
