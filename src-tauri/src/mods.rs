//! 模组管理子系统（对应原版 KAMUCL-1.1.21/src/main/core/ 的 modinfo.ts、modMetadata.ts、
//! modScan.ts/modScanWorker.ts、modState.ts、modTransaction.ts、modUpdates.ts、
//! modManagement.ts、modPlanStore.ts、modInstallPlan.ts、modTargets.ts、modIcons.ts、
//! modIconIdentity.ts、modFavorites.ts（shared/modFavorites.ts）、recordingMods.ts、
//! modMigration.ts、supplementalMods.ts、fileHash.ts）。
//!
//! 通道：mods:parse / duplicates / crossDuplicates / icons / migrationPlan / migrationApply /
//!       checkUpdates / applyUpdates / catalog / setEnabled / setLocked / versionChoices /
//!       versionPlan / versionApply / versionDiscard / targets / prepare / commit / discard /
//!       install / favorites / favorite / favoriteRemove / favoriteLink / favoriteVersions /
//!       favoriteVersionsCancel / favoriteLocal / supplementalList / supplementalKeep /
//!       supplementalRetry
//!
//! 与原版的差异（语义保持一致的前提下）：
//! 1. Worker 线程扫描（modScanWorker）→ 直接在调用线程解析（zip crate；长任务已在
//!    spawned 线程上运行）；共享扫描缓存未保留（每次重扫，行为等价）。
//! 2. mods.toml 解析：依赖清单无 TOML crate → 手写子集解析器（见 parse_mods_toml）。
//! 3. AbortSignal → tasks.rs TaskRecord.cancelled 轮询；ModPlanStore 的租约/过期语义
//!    精确保留（users 计数 + 单次提交），仅定时器轮换为惰性过期。
//! 4. 批量下载（downloadAll）→ crate::download::download_file 逐文件顺序传输
//!    （modInstallPlan 的 prepare/commit 使用显式 mirror='official'，replaceModFiles 用
//!    settings.mirror，与原版一致）；并发差异不影响落盘结果与错误文案。
//! 5. launch.getRunningVersionIds → [`set_running_version_ids_provider`] 接线点（launch.rs
//!    移植后注册）；未注册时视为无运行实例。externalGameUsesDirectory（外部启动器占用检测）
//!    未移植。
//! 6. changeProtection（BackupStore 备份）→ [`set_mod_backup_hook`] 接线点；未注册时跳过
//!    备份（备份库归 instance-center/backup 子系统所有）。
//! 7. modMigration 的目标实例安装（versions.installVersion）→ [`set_migration_install_hook`]
//!    接线点（installer 的完整安装流程未 pub，由集成方注册）。
//! 8. 整合包安装完成回调见 community.rs [`crate::community::set_modpack_install_hook`]。
//! 9. mods:favoriteVersions 的订阅按 ticket 全局登记（Electron 按 sender 隔离；Tauri 单主窗口
//!    等价）；单飞共享队列（4 并发、无完成缓存）语义保留。

use crate::community::{
    self, community_exact_file, community_favorite_candidates, community_file_matches_instance,
    community_files, community_search_items, dependency_range, matches_version_range,
    mod_matches_instance, mod_mismatch_reasons, mr_post, normalize_loader,
};
use crate::download::{self, DownloadFileOpts, SmoothedSpeedEstimator};
use crate::installer::{self, ProgressGuard};
use crate::net::{self, Sha1};
use crate::tasks::{self, TaskRecord};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const UA_MGMT: &str = "WUHUICL (github.com/AUGUHDAR/WUHUICL)";
const UA_UPDATES: &str = "WUHUICL-Launcher (github.com/AUGUHDAR/WUHUICL)";

fn arg(args: &[Value], i: usize) -> Option<&Value> {
    args.get(i)
}

fn arg_str(args: &[Value], i: usize) -> Option<&str> {
    args.get(i).and_then(|v| v.as_str())
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn check_cancel(rec: &TaskRecord) -> Result<(), String> {
    if rec.cancelled.load(Ordering::SeqCst) {
        Err("已取消".into())
    } else {
        Ok(())
    }
}

fn is_cancel(msg: &str) -> bool {
    download::is_cancel_error(msg)
}

/// 路径同一性（pathIdentity：win32 小写化解析）
fn same_fs_path(a: &Path, b: &Path) -> bool {
    installer::path_identity(&a.to_string_lossy()) == installer::path_identity(&b.to_string_lossy())
}

fn strip_bom(text: &str) -> &str {
    text.strip_prefix('\u{FEFF}').unwrap_or(text)
}

// ---------------- HTTP（modManagement json / modIcons request / modMigration post） ----------------

fn post_json(url: &str, body: &Value, ua: &str, timeout_ms: u64, headers: &[(&str, &str)]) -> Result<Value, String> {
    let mut req = net::client()
        .post(url)
        .timeout(Duration::from_millis(timeout_ms))
        .header("User-Agent", ua)
        .header("Content-Type", "application/json")
        .body(body.to_string());
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let resp = req.send().map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(format!("HTTP {status}"));
    }
    resp.json::<Value>().map_err(|e| e.to_string())
}

fn get_json(url: &str, ua: &str, timeout_ms: u64, headers: &[(&str, &str)]) -> Result<Value, String> {
    let mut req = net::client().get(url).timeout(Duration::from_millis(timeout_ms)).header("User-Agent", ua);
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let resp = req.send().map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(format!("HTTP {status}"));
    }
    resp.json::<Value>().map_err(|e| e.to_string())
}

/// modManagement json()：错误文案「社区请求失败 HTTP {status}」
fn mgmt_json(url: &str, body: Option<&Value>, headers: &[(&str, &str)]) -> Result<Value, String> {
    let result = match body {
        Some(b) => post_json(url, b, UA_MGMT, 15_000, headers),
        None => get_json(url, UA_MGMT, 15_000, headers),
    };
    result.map_err(|e| if e.starts_with("HTTP ") { format!("社区请求失败 {e}") } else { e })
}

// ---------------- modState.ts（目录级锁定/身份记录，userData/mod-state/<sha256>.json） ----------------

#[derive(Clone, Default)]
struct DirectoryState {
    aliases: HashMap<String, String>,
    locks: HashMap<String, bool>,
}

fn state_file(app: &tauri::AppHandle, dir: &Path) -> PathBuf {
    let real = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    let mut key_src = real.to_string_lossy().into_owned();
    if cfg!(windows) {
        key_src = key_src.to_lowercase();
    }
    let digest = hex::encode(Sha256::digest(key_src.as_bytes()));
    crate::settings::user_data_dir(app).join("mod-state").join(format!("{digest}.json"))
}

fn read_state(app: &tauri::AppHandle, dir: &Path) -> Result<DirectoryState, String> {
    let path = state_file(app, dir);
    let raw = match std::fs::read(&path) {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(DirectoryState::default()),
        Err(_) => return Err("模组锁定记录无法读取，请先恢复记录后重试".into()),
    };
    let value: Value = serde_json::from_slice(&raw)
        .map_err(|_| "模组锁定记录无法读取，请先恢复记录后重试".to_string())?;
    let mut state = DirectoryState::default();
    if let Some(map) = value.get("aliases").and_then(|v| v.as_object()) {
        for (k, v) in map {
            if let Some(s) = v.as_str() {
                state.aliases.insert(k.clone(), s.to_string());
            }
        }
    }
    if let Some(map) = value.get("locks").and_then(|v| v.as_object()) {
        for (k, v) in map {
            state.locks.insert(k.clone(), v.as_bool().unwrap_or(false));
        }
    }
    Ok(state)
}

fn write_state(app: &tauri::AppHandle, dir: &Path, state: &DirectoryState) -> Result<(), String> {
    let aliases: serde_json::Map<String, Value> = state.aliases.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
    let locks: serde_json::Map<String, Value> = state.locks.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
    let payload = json!({ "aliases": aliases, "locks": locks });
    let path = state_file(app, dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, payload.to_string()).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

/// modIdentity()
pub fn mod_identity(app: &tauri::AppHandle, dir: &Path, sha1: &str) -> String {
    read_state(app, dir).ok().and_then(|s| s.aliases.get(sha1).cloned()).unwrap_or_else(|| format!("sha1:{sha1}"))
}

/// isModLocked()
pub fn is_mod_locked(app: &tauri::AppHandle, dir: &Path, sha1: &str) -> bool {
    let Ok(state) = read_state(app, dir) else { return false };
    let identity = state.aliases.get(sha1).cloned();
    state.locks.get(identity.as_deref().unwrap_or(&format!("sha1:{sha1}"))).copied().unwrap_or(false)
        || state.locks.get(&format!("sha1:{sha1}")).copied().unwrap_or(false)
}

/// setModLocked()
pub fn set_mod_locked(app: &tauri::AppHandle, dir: &Path, sha1: &str, locked: bool) {
    let mut state = match read_state(app, dir) {
        Ok(s) => s,
        Err(_) => return,
    };
    let identity = state.aliases.get(sha1).cloned();
    let key = identity.clone().unwrap_or_else(|| format!("sha1:{sha1}"));
    state.locks.insert(key, locked);
    state.locks.insert(format!("sha1:{sha1}"), locked);
    if let Some(id) = &identity {
        let same: Vec<String> = state.aliases.iter().filter(|(_, v)| v == &id).map(|(h, _)| h.clone()).collect();
        for hash in same {
            state.locks.insert(format!("sha1:{hash}"), locked);
        }
    }
    let _ = write_state(app, dir, &state);
}

/// rememberModIdentity()
pub fn remember_mod_identity(app: &tauri::AppHandle, dir: &Path, sha1: &str, identity: &str) {
    static ID_RE: OnceLock<regex::Regex> = OnceLock::new();
    static SHA_RE: OnceLock<regex::Regex> = OnceLock::new();
    let id_ok = ID_RE.get_or_init(|| regex::Regex::new(r"^(modrinth|curseforge):[a-zA-Z0-9_-]+$").unwrap()).is_match(identity);
    let sha_ok = SHA_RE.get_or_init(|| regex::Regex::new(r"^[a-f0-9]{40}$").unwrap()).is_match(sha1);
    if !id_ok || !sha_ok {
        return;
    }
    let mut state = match read_state(app, dir) {
        Ok(s) => s,
        Err(_) => return,
    };
    if state.aliases.get(sha1).map(String::as_str) == Some(identity) {
        return;
    }
    if state.locks.get(&format!("sha1:{sha1}")).copied().unwrap_or(false) {
        state.locks.insert(identity.to_string(), true);
    }
    state.aliases.insert(sha1.to_string(), identity.to_string());
    let _ = write_state(app, dir, &state);
}

/// transferModLock()
pub fn transfer_mod_lock(app: &tauri::AppHandle, dir: &Path, old_sha: &str, new_sha: &str) {
    let identity = mod_identity(app, dir, old_sha);
    let locked = is_mod_locked(app, dir, old_sha);
    if !identity.starts_with("sha1:") {
        remember_mod_identity(app, dir, new_sha, &identity);
    }
    if locked {
        set_mod_locked(app, dir, new_sha, true);
    }
}

// ---------------- resourceDirectory.ts ----------------

/// requireResourceVersion()
pub fn require_resource_version(id: &str) -> Result<(), String> {
    if id.trim().is_empty() {
        return Err("请先安装或选择一个游戏版本".into());
    }
    if id == "." || id == ".." || id.contains(['\\', '/', ':', '\0']) {
        return Err("游戏版本名称无效，请重新选择".into());
    }
    Ok(())
}

/// resolveResourceDirectory()：只读选定实例，绝不回退共享目录；junction 指向实例外即拒绝
pub fn resolve_resource_directory(app: &tauri::AppHandle, folder: &str, id: &str, kind: &str) -> Result<PathBuf, String> {
    require_resource_version(id)?;
    if folder.is_empty() || !["mods", "resourcepacks", "shaderpacks"].contains(&kind) {
        return Err("资源目录无效，请重新选择游戏版本".into());
    }
    let json_path = installer::version_json_path_in(folder, id);
    let raw = std::fs::read(&json_path)
        .map_err(|_| "所选游戏版本已移除或版本描述损坏，请到「游戏版本」页刷新或修复".to_string())?;
    let text = String::from_utf8_lossy(&raw);
    let version_json: Value =
        serde_json::from_str(strip_bom(&text)).map_err(|_| "所选游戏版本已移除或版本描述损坏，请到「游戏版本」页刷新或修复".to_string())?;
    let game_directory = installer::instance_directory_state(app, folder, id, &version_json)
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let dir = Path::new(&game_directory).join(kind);
    // Junction/symlink 校验
    let base_ok = std::fs::canonicalize(&game_directory);
    let dir_ok = std::fs::canonicalize(&dir);
    if let (Ok(base), Ok(target)) = (base_ok, dir_ok) {
        let rel_ok = target.strip_prefix(&base).is_ok();
        if !rel_ok || base == target {
            return Err("资源目录链接指向实例外部，请检查游戏目录".into());
        }
    }
    Ok(dir)
}

fn mods_dir_of(app: &tauri::AppHandle, version_id: &str) -> Result<PathBuf, String> {
    let folder = installer::folder_of_version(app, version_id);
    resolve_resource_directory(app, &folder, version_id, "mods")
}

// ---------------- modIconIdentity.ts：CurseForge 指纹（MurmurHash2, seed 1, 去空白） ----------------

/// curseFingerprint()
pub fn curse_fingerprint(input: &[u8]) -> u32 {
    let mut length: u32 = 0;
    for &b in input {
        if !matches!(b, 9 | 10 | 13 | 32) {
            length = length.wrapping_add(1);
        }
    }
    const M: u32 = 0x5bd1e995;
    let mut h = 1u32.wrapping_sub(1).wrapping_add(length); // (1 ^ length) >>> 0
    let mut word: u32 = 0;
    let mut shift: u32 = 0;
    for &byte in input {
        if matches!(byte, 9 | 10 | 13 | 32) {
            continue;
        }
        word |= (byte as u32) << shift;
        if shift == 24 {
            word = word.wrapping_mul(M);
            word ^= word >> 24;
            word = word.wrapping_mul(M);
            h = h.wrapping_mul(M) ^ word;
            word = 0;
            shift = 0;
        } else {
            shift += 8;
        }
    }
    if shift != 0 {
        h ^= word;
        h = h.wrapping_mul(M);
    }
    h ^= h >> 13;
    h = h.wrapping_mul(M);
    h ^= h >> 15;
    h
}

// ---------------- modMetadata.ts：jar 内元数据解析 ----------------

const BUILTINS: [&str; 6] = ["minecraft", "forge", "neoforge", "fabricloader", "quilt_loader", "java"];

struct ArchiveEntry {
    bytes: Vec<u8>,
}

fn zip_read_entry<R: Read + std::io::Seek>(archive: &mut zip::ZipArchive<R>, name: &str, max_bytes: u64) -> Result<Option<ArchiveEntry>, String> {
    let mut file = match archive.by_name(name) {
        Ok(f) => f,
        Err(_) => return Ok(None),
    };
    if file.size() > max_bytes {
        return Err(format!("{name} 元数据过大"));
    }
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    Ok(Some(ArchiveEntry { bytes: buf }))
}

fn zip_read_text<R: Read + std::io::Seek>(archive: &mut zip::ZipArchive<R>, name: &str) -> Result<Option<String>, String> {
    match zip_read_entry(archive, name, 2 * 1024 * 1024)? {
        Some(e) => Ok(Some(String::from_utf8_lossy(&e.bytes).strip_prefix('\u{FEFF}').map(str::to_string).unwrap_or_else(|| String::from_utf8_lossy(&e.bytes).into_owned()))),
        None => Ok(None),
    }
}

/// TOML 子集解析器（mods.toml：表头/数组表/键值/字符串/整数/布尔/数组/多行字符串）
fn parse_mods_toml(text: &str) -> Result<Value, String> {
    fn strip_comment(line: &str) -> String {
        let mut out = String::new();
        let mut in_basic = false;
        let mut in_literal = false;
        let mut escaped = false;
        for c in line.chars() {
            if in_basic {
                out.push(c);
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == '"' {
                    in_basic = false;
                }
                continue;
            }
            if in_literal {
                out.push(c);
                if c == '\'' {
                    in_literal = false;
                }
                continue;
            }
            match c {
                '"' => {
                    in_basic = true;
                    out.push(c);
                }
                '\'' => {
                    in_literal = true;
                    out.push(c);
                }
                '#' => break,
                _ => out.push(c),
            }
        }
        out
    }

    fn parse_scalar(s: &str) -> Value {
        let s = s.trim();
        if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
            let inner = &s[1..s.len() - 1];
            let mut out = String::new();
            let mut chars = inner.chars();
            while let Some(c) = chars.next() {
                if c == '\\' {
                    match chars.next() {
                        Some('n') => out.push('\n'),
                        Some('t') => out.push('\t'),
                        Some('r') => out.push('\r'),
                        Some('"') => out.push('"'),
                        Some('\\') => out.push('\\'),
                        Some('u') => {
                            let hex: String = chars.by_ref().take(4).collect();
                            if let Some(cp) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                                out.push(cp);
                            }
                        }
                        Some(other) => {
                            out.push('\\');
                            out.push(other);
                        }
                        None => out.push('\\'),
                    }
                } else {
                    out.push(c);
                }
            }
            return Value::String(out);
        }
        if s.len() >= 2 && s.starts_with('\'') && s.ends_with('\'') {
            return Value::String(s[1..s.len() - 1].to_string());
        }
        if s == "true" {
            return Value::Bool(true);
        }
        if s == "false" {
            return Value::Bool(false);
        }
        if let Ok(i) = s.parse::<i64>() {
            return json!(i);
        }
        if let Ok(f) = s.parse::<f64>() {
            return json!(f);
        }
        Value::String(s.to_string())
    }

    fn split_top_level(s: &str, sep: char) -> Vec<String> {
        let mut out = Vec::new();
        let mut depth = 0i32;
        let mut cur = String::new();
        let mut in_basic = false;
        let mut in_literal = false;
        let mut escaped = false;
        for c in s.chars() {
            if in_basic {
                cur.push(c);
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == '"' {
                    in_basic = false;
                }
                continue;
            }
            match c {
                '\'' => {
                    in_literal = !in_literal;
                    cur.push(c);
                }
                '"' => {
                    in_basic = true;
                    cur.push(c);
                }
                '[' | '{' => {
                    depth += 1;
                    cur.push(c);
                }
                ']' | '}' => {
                    depth -= 1;
                    cur.push(c);
                }
                c if c == sep && depth == 0 => {
                    out.push(cur.clone());
                    cur.clear();
                }
                _ => cur.push(c),
            }
        }
        if !cur.trim().is_empty() || out.is_empty() {
            out.push(cur);
        }
        out
    }

    fn parse_array(s: &str) -> Value {
        let inner = s.trim();
        let inner = &inner[1..inner.len().saturating_sub(1)];
        let mut items = Vec::new();
        for part in split_top_level(inner, ',') {
            let p = part.trim();
            if p.is_empty() {
                continue;
            }
            items.push(parse_scalar(p));
        }
        Value::Array(items)
    }

    fn parse_header_keys(inner: &str) -> Vec<String> {
        split_top_level(inner, '.')
            .into_iter()
            .map(|seg| {
                let seg = seg.trim();
                if seg.len() >= 2 && seg.starts_with('"') && seg.ends_with('"') {
                    seg[1..seg.len() - 1].to_string()
                } else if seg.len() >= 2 && seg.starts_with('\'') && seg.ends_with('\'') {
                    seg[1..seg.len() - 1].to_string()
                } else {
                    seg.to_string()
                }
            })
            .collect()
    }

    let mut root = serde_json::Map::new();
    enum Cursor {
        Table(Vec<String>),
        ArrayElem(Vec<String>),
    }
    let mut cursor = Cursor::Table(Vec::new());

    fn table_at<'a>(root: &'a mut serde_json::Map<String, Value>, path: &[String]) -> &'a mut serde_json::Map<String, Value> {
        let mut node = root;
        for key in path {
            let entry = node.entry(key.clone()).or_insert_with(|| json!({}));
            if !entry.is_object() {
                *entry = json!({});
            }
            node = entry.as_object_mut().unwrap();
        }
        node
    }

    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0usize;
    while i < lines.len() {
        let raw = strip_comment(lines[i]);
        let line = raw.trim().to_string();
        i += 1;
        if line.is_empty() {
            continue;
        }
        if line.starts_with("[[") && line.ends_with("]]") {
            let path = parse_header_keys(&line[2..line.len() - 2]);
            let parent = &path[..path.len() - 1];
            let last = path[path.len() - 1].clone();
            let node = table_at(&mut root, parent);
            let arr = node.entry(last).or_insert_with(|| json!([]));
            if !arr.is_array() {
                *arr = json!([]);
            }
            arr.as_array_mut().unwrap().push(json!({}));
            cursor = Cursor::ArrayElem(path);
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            let path = parse_header_keys(&line[1..line.len() - 1]);
            table_at(&mut root, &path);
            cursor = Cursor::Table(path);
            continue;
        }
        // key = value（值可能跨行：数组 / 多行字符串）
        let Some(eq) = line.find('=') else { continue };
        let key_part = line[..eq].trim().to_string();
        let mut value_part = line[eq + 1..].trim().to_string();
        let key = {
            let segs = parse_header_keys(&key_part);
            segs.last().cloned().unwrap_or_default()
        };
        // 多行数组 / 多行字符串补齐
        let mut balance = |s: &str| -> i32 {
            let mut d = 0i32;
            let mut in_basic = false;
            let mut in_literal = false;
            let mut escaped = false;
            for c in s.chars() {
                if in_basic {
                    if escaped {
                        escaped = false;
                    } else if c == '\\' {
                        escaped = true;
                    } else if c == '"' {
                        in_basic = false;
                    }
                    continue;
                }
                match c {
                    '\'' => in_literal = !in_literal,
                    '"' => in_basic = true,
                    '[' => d += 1,
                    ']' => d -= 1,
                    _ => {}
                }
            }
            d
        };
        let mut quote3: Option<&str> = None;
        if value_part.starts_with("\"\"\"") {
            quote3 = Some("\"\"\"");
        } else if value_part.starts_with("'''") {
            quote3 = Some("'''");
        }
        if let Some(q) = quote3 {
            let mut acc = value_part[3..].to_string();
            while !acc.contains(q) && i < lines.len() {
                acc.push('\n');
                acc.push_str(strip_comment(lines[i]).trim_start());
                i += 1;
            }
            if let Some(pos) = acc.find(q) {
                acc.truncate(pos);
            }
            value_part = format!("{q}{acc}{q}");
        } else if value_part.starts_with('[') && balance(&value_part) > 0 {
            while balance(&value_part) > 0 && i < lines.len() {
                value_part.push('\n');
                value_part.push_str(strip_comment(lines[i]).trim_start());
                i += 1;
            }
        }
        let value = if value_part.starts_with('[') && value_part.ends_with(']') {
            parse_array(&value_part)
        } else {
            parse_scalar(&value_part)
        };
        match &cursor {
            Cursor::Table(path) => {
                table_at(&mut root, path).insert(key, value);
            }
            Cursor::ArrayElem(path) => {
                let parent = &path[..path.len() - 1];
                let last = path[path.len() - 1].clone();
                let node = table_at(&mut root, parent);
                if let Some(arr) = node.get_mut(&last).and_then(|v| v.as_array_mut()) {
                    if let Some(elem) = arr.last_mut().and_then(|v| v.as_object_mut()) {
                        elem.insert(key, value);
                    }
                }
            }
        }
    }
    Ok(Value::Object(root))
}

fn dependency_requirements(deps: &Value, dialect: &str) -> Vec<Value> {
    deps.as_object()
        .map(|m| {
            m.iter()
                .filter(|(id, _)| !BUILTINS.contains(&id.as_str()))
                .map(|(id, range)| json!({ "id": id, "range": dependency_range(range, dialect) }))
                .collect()
        })
        .unwrap_or_default()
}

fn join_ranges(list: &[Value], key: &str) -> String {
    let parts: Vec<String> = list
        .iter()
        .filter(|d| d.get("modId").and_then(|v| v.as_str()) == Some(key))
        .map(|d| d.get("versionRange").and_then(|v| v.as_str()).unwrap_or("*").to_string())
        .collect();
    parts.join(" && ")
}

/// parseModArchive()：fabric.mod.json / quilt.mod.json / neoforge.mods.toml / mods.toml /
/// mcmod.info + jarjar 嵌套 + 图标提取。错误写入 info.error（与原版一致不抛出）。
pub fn parse_mod_archive(data: &[u8], file_path: &str, file_name: &str, depth: u32, include_icons: bool) -> Value {
    let mut info = json!({
        "filePath": file_path,
        "fileName": file_name,
        "id": "",
        "name": "",
        "version": "",
        "loader": Value::Null,
        "mcRange": "",
        "dependencies": [],
    });
    let build_error = |msg: &str| -> Value {
        let mut e = info.clone();
        e["error"] = json!(msg);
        e
    };
    let mut archive = match zip::ZipArchive::new(std::io::Cursor::new(data)) {
        Ok(a) => a,
        Err(_) => return build_error("未找到有效的 MOD 元数据 / mod id"),
    };
    let outcome = (|| -> Result<Value, String> {
        let mut records: Vec<Value> = Vec::new(); // {loader,id,name,version,mcRange,loaderRange,requirements,icon?}
        let mut provides: Vec<(String, String)> = Vec::new();
        let mut nested: Vec<String> = Vec::new();
        let mut java_requirements: Vec<Value> = Vec::new();

        // ---- fabric.mod.json ----
        if let Some(fabric) = zip_read_text(&mut archive, "fabric.mod.json")? {
            let j: Value = serde_json::from_str(strip_bom(&fabric)).map_err(|e| e.to_string())?;
            let dep = j.get("depends").cloned().unwrap_or(json!({}));
            let environment = j.get("environment").and_then(|v| v.as_str()).unwrap_or("");
            if environment != "server" {
                if dep.get("java").is_some() && !dep.get("java").unwrap().is_null() {
                    java_requirements.push(json!({ "loader": "fabric", "source": format!("{file_name}: {}", j.get("id").and_then(|v| v.as_str()).unwrap_or("")), "range": dependency_range(dep.get("java").unwrap(), "fabric") }));
                }
                if let Some(breaks) = j.get("breaks").and_then(|b| b.get("java")) {
                    if !breaks.is_null() {
                        java_requirements.push(json!({ "loader": "fabric", "source": format!("{file_name}: {}", j.get("id").and_then(|v| v.as_str()).unwrap_or("")), "range": dependency_range(breaks, "fabric"), "exclude": true }));
                    }
                }
            }
            let id = j.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let name = j.get("name").and_then(|v| v.as_str()).unwrap_or(id);
            let version = match j.get("version") {
                Some(Value::String(s)) => s.clone(),
                Some(v) => v.to_string(),
                None => String::new(),
            };
            let icon = if let Some(i) = j.get("icon") {
                if let Some(s) = i.as_str() {
                    Some(s.to_string())
                } else if let Some(m) = i.as_object() {
                    m.values().next().and_then(|v| v.as_str()).map(str::to_string)
                } else {
                    None
                }
            } else {
                None
            };
            records.push(json!({
                "loader": "fabric",
                "id": id,
                "name": name,
                "version": version,
                "mcRange": dependency_range(dep.get("minecraft").unwrap_or(&Value::Null), "fabric"),
                "loaderRange": dependency_range(dep.get("fabricloader").unwrap_or(&Value::Null), "fabric"),
                "requirements": dependency_requirements(&dep, "fabric"),
                "icon": icon,
            }));
            if let Some(p) = j.get("provides").and_then(|v| v.as_array()) {
                for pid in p {
                    if let Some(s) = pid.as_str() {
                        provides.push((s.to_string(), version.clone()));
                    }
                }
            }
            if let Some(jars) = j.get("jars").and_then(|v| v.as_array()) {
                for jar in jars {
                    if let Some(f) = jar.get("file").and_then(|v| v.as_str()) {
                        nested.push(f.to_string());
                    }
                }
            }
        }

        // ---- quilt.mod.json ----
        if let Some(quilt) = zip_read_text(&mut archive, "quilt.mod.json")? {
            let parsed: Value = serde_json::from_str(strip_bom(&quilt)).map_err(|e| e.to_string())?;
            let j = parsed.get("quilt_loader").cloned().unwrap_or(json!({}));
            let mut deps: serde_json::Map<String, Value> = serde_json::Map::new();
            if let Some(list) = j.get("depends").and_then(|v| v.as_array()) {
                for raw in list {
                    let d = if let Some(s) = raw.as_str() {
                        json!({ "id": s, "versions": "*" })
                    } else {
                        raw.clone()
                    };
                    if d.is_array() || d.get("unless").is_some() {
                        return Err("Quilt 条件前置需要手动检查；未自动安装".into());
                    }
                    if let Some(id) = d.get("id").and_then(|v| v.as_str()) {
                        if !d.get("optional").and_then(|v| v.as_bool()).unwrap_or(false)
                            && d.get("environment").and_then(|v| v.as_str()) != Some("server")
                        {
                            let key = id.split(':').next_back().unwrap_or(id).to_string();
                            let versions = d.get("versions").cloned().unwrap_or(json!("*"));
                            let merged = match deps.get(&key) {
                                None => versions,
                                Some(prev) => json!({ "all": [prev.clone(), versions] }),
                            };
                            deps.insert(key, merged);
                        }
                    }
                }
            }
            let deps_value = Value::Object(deps.clone());
            let id = j.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let name = j.pointer("/metadata/name").and_then(|v| v.as_str()).unwrap_or(id);
            let version = match j.get("version") {
                Some(Value::String(s)) => s.clone(),
                Some(v) => v.to_string(),
                None => String::new(),
            };
            records.push(json!({
                "loader": "quilt",
                "id": id,
                "name": name,
                "version": version,
                "mcRange": dependency_range(deps_value.get("minecraft").unwrap_or(&Value::Null), "quilt"),
                "loaderRange": dependency_range(deps_value.get("quilt_loader").unwrap_or(&Value::Null), "quilt"),
                "requirements": dependency_requirements(&deps_value, "quilt"),
                "icon": j.pointer("/metadata/icon").and_then(|v| v.as_str()).map(str::to_string),
            }));
            if deps_value.get("java").is_some() {
                java_requirements.push(json!({ "loader": "quilt", "source": format!("{file_name}: {id}"), "range": dependency_range(deps_value.get("java").unwrap(), "quilt") }));
            }
            if let Some(p) = j.get("provides").and_then(|v| v.as_array()) {
                for pv in p {
                    let pid = match pv {
                        Value::String(s) => s.clone(),
                        other => other.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    };
                    let ver = pv.get("version").and_then(|v| v.as_str()).map(str::to_string).unwrap_or_else(|| version.clone());
                    provides.push((pid.split(':').next_back().unwrap_or("").to_string(), ver));
                }
            }
            if let Some(jars) = j.get("jars").and_then(|v| v.as_array()) {
                for jar in jars {
                    match jar {
                        Value::String(s) => nested.push(s.clone()),
                        other => {
                            if let Some(f) = other.get("file").and_then(|v| v.as_str()) {
                                nested.push(f.to_string());
                            }
                        }
                    }
                }
            }
        }

        // ---- neoforge.mods.toml / mods.toml ----
        for file in ["META-INF/neoforge.mods.toml", "META-INF/mods.toml"] {
            let Some(text) = zip_read_text(&mut archive, file)? else { continue };
            let j = parse_mods_toml(&text)?;
            let mods = j.get("mods").and_then(|v| v.as_array()).cloned().unwrap_or_default();
            if mods.is_empty() {
                return Err(format!("{file} 缺少 [[mods]]"));
            }
            let mut all_deps: Vec<Value> = Vec::new();
            if let Some(dep_map) = j.get("dependencies").and_then(|v| v.as_object()) {
                for list in dep_map.values() {
                    if let Some(arr) = list.as_array() {
                        all_deps.extend(arr.iter().cloned());
                    } else {
                        all_deps.push(list.clone());
                    }
                }
            }
            let loader: &str = if file.contains("neoforge") || all_deps.iter().any(|d| d.get("modId").and_then(|v| v.as_str()) == Some("neoforge")) {
                "neoforge"
            } else {
                "forge"
            };
            let is_required = |d: &Value| -> bool {
                d.get("mandatory").and_then(|v| v.as_bool()) != Some(false)
                    && d.get("type").and_then(|v| v.as_str()).map(|t| t.is_empty() || t == "required").unwrap_or(true)
                    && d.get("side").and_then(|v| v.as_str()).unwrap_or("BOTH").to_uppercase() != "SERVER"
            };
            let required: Vec<Value> = all_deps.iter().filter(|d| is_required(d)).cloned().collect();
            let manifest = zip_read_text(&mut archive, "META-INF/MANIFEST.MF")?.unwrap_or_default();
            let jar_version = regex::Regex::new(r"(?mi)^Implementation-Version:\s*(.+)$")
                .ok()
                .and_then(|re| re.captures(&manifest))
                .map(|c| c[1].trim().to_string())
                .unwrap_or_default();
            let mod_loader = j.get("modLoader").and_then(|v| v.as_str()).unwrap_or("");
            for m in &mods {
                let mod_id = m.get("modId").and_then(|v| v.as_str()).unwrap_or("");
                let own: Vec<Value> = j
                    .get("dependencies")
                    .and_then(|d| d.get(mod_id))
                    .and_then(|d| d.as_array())
                    .map(|arr| arr.iter().filter(|d| is_required(d)).cloned().collect())
                    .unwrap_or_default();
                let raw_version = match m.get("version") {
                    Some(Value::String(s)) => s.clone(),
                    Some(v) => v.to_string(),
                    None => String::new(),
                };
                let version = raw_version.replace("${file.jarVersion}", &jar_version);
                provides.push((mod_id.to_string(), version.clone()));
                if let Some(jr) = j.pointer(&format!("/features/{mod_id}/java_version")).and_then(|v| v.as_str()) {
                    java_requirements.push(json!({ "loader": loader, "source": format!("{file_name}: {mod_id}"), "range": jr }));
                }
                for d in own.iter().filter(|d| d.get("modId").and_then(|v| v.as_str()) == Some("java")) {
                    java_requirements.push(json!({ "loader": loader, "source": format!("{file_name}: {mod_id}"), "range": d.get("versionRange").and_then(|v| v.as_str()).unwrap_or("*") }));
                }
                let loader_range = {
                    let own_ranges = join_ranges(&own, loader);
                    if !own_ranges.is_empty() {
                        own_ranges
                    } else if loader == "forge" && (mod_loader == "javafml" || mod_loader == "lowcodefml") {
                        j.get("loaderVersion").and_then(|v| v.as_str()).unwrap_or("").to_string()
                    } else {
                        String::new()
                    }
                };
                records.push(json!({
                    "loader": loader,
                    "id": mod_id,
                    "name": m.get("displayName").and_then(|v| v.as_str()).unwrap_or(mod_id),
                    "version": version,
                    "mcRange": join_ranges(&own, "minecraft"),
                    "loaderRange": loader_range,
                    "requirements": own.iter().filter(|d| !d.get("modId").and_then(|v| v.as_str()).map(|id| BUILTINS.contains(&id)).unwrap_or(false)).map(|d| json!({"id": d.get("modId").and_then(|v| v.as_str()).unwrap_or(""), "range": d.get("versionRange").and_then(|v| v.as_str()).unwrap_or("*")})).collect::<Vec<Value>>(),
                    "icon": m.get("logoFile").and_then(|v| v.as_str()).or_else(|| j.get("logoFile").and_then(|v| v.as_str())).map(str::to_string),
                }));
            }
        }

        // ---- mcmod.info（legacy） ----
        if records.is_empty() {
            if let Some(legacy) = zip_read_text(&mut archive, "mcmod.info")? {
                let j: Value = serde_json::from_str(strip_bom(&legacy)).unwrap_or(Value::Null);
                let mods = if j.is_array() { j.clone() } else { j.get("modList").cloned().unwrap_or(json!([])) };
                if let Some(list) = mods.as_array() {
                    for m in list {
                        let req_src = m.get("requiredMods").filter(|v| v.is_array()).or_else(|| m.get("dependencies").filter(|v| v.is_array())).cloned().unwrap_or(json!([]));
                        let req: Vec<Value> = req_src
                            .as_array()
                            .unwrap()
                            .iter()
                            .filter_map(|s| s.as_str())
                            .map(|s| {
                                let stripped = regex::Regex::new(r"^required-(?:after|before):").unwrap().replace(s, "").to_string();
                                match stripped.split_once('@') {
                                    Some((id, range)) => json!({ "id": id, "range": range }),
                                    None => json!({ "id": stripped, "range": "*" }),
                                }
                            })
                            .filter(|d: &Value| !d.get("id").and_then(|v| v.as_str()).map(|id| BUILTINS.contains(&id)).unwrap_or(false))
                            .collect();
                        let id = m.get("modid").and_then(|v| v.as_str()).unwrap_or("");
                        records.push(json!({
                            "loader": "forge",
                            "id": id,
                            "name": m.get("name").and_then(|v| v.as_str()).unwrap_or(id),
                            "version": m.get("version").and_then(|v| v.as_str()).unwrap_or(""),
                            "mcRange": m.get("mcversion").and_then(|v| v.as_str()).unwrap_or(""),
                            "loaderRange": "",
                            "requirements": req,
                            "icon": Value::Null,
                        }));
                        provides.push((id.to_string(), m.get("version").and_then(|v| v.as_str()).unwrap_or("").to_string()));
                    }
                }
            }
        }

        if records.is_empty() || records[0].get("id").and_then(|v| v.as_str()).unwrap_or("").is_empty() {
            return Err("未找到有效的 MOD 元数据 / mod id".into());
        }
        let first = records[0].clone();
        let mut out = json!({
            "filePath": file_path,
            "fileName": file_name,
            "id": first.get("id").cloned().unwrap_or(json!("")),
            "name": first.get("name").cloned().unwrap_or(json!("")),
            "version": first.get("version").cloned().unwrap_or(json!("")),
            "loader": first.get("loader").cloned().unwrap_or(Value::Null),
            "mcRange": first.get("mcRange").cloned().unwrap_or(json!("")),
            "dependencies": first.get("requirements").and_then(|r| r.as_array()).map(|a| a.iter().filter_map(|d| d.get("id").cloned()).collect::<Vec<Value>>()).unwrap_or_default(),
        });
        if let Some(lr) = first.get("loaderRange").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
            out["loaderRange"] = json!(lr);
        }
        if let Some(icon) = first.get("icon") {
            if icon.is_string() {
                out["_icon"] = icon.clone();
            }
        }
        out["requirements"] = first.get("requirements").cloned().unwrap_or(json!([]));
        out["variants"] = json!(records
            .iter()
            .map(|r| {
                let mut v = json!({
                    "loader": r.get("loader").cloned().unwrap_or(Value::Null),
                    "mcRange": r.get("mcRange").cloned().unwrap_or(json!("")),
                    "requirements": r.get("requirements").cloned().unwrap_or(json!([])),
                });
                if let Some(lr) = r.get("loaderRange").and_then(|x| x.as_str()).filter(|s| !s.is_empty()) {
                    v["loaderRange"] = json!(lr);
                }
                v
            })
            .collect::<Vec<Value>>());
        for r in &records {
            provides.push((r.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string(), r.get("version").and_then(|v| v.as_str()).unwrap_or("").to_string()));
        }
        // jarjar 嵌套 jar
        if let Some(jarjar) = zip_read_text(&mut archive, "META-INF/jarjar/metadata.json")? {
            if let Ok(j) = serde_json::from_str::<Value>(strip_bom(&jarjar)) {
                if let Some(jars) = j.get("jars").and_then(|v| v.as_array()) {
                    for jar in jars {
                        if let Some(p) = jar.get("path").and_then(|v| v.as_str()) {
                            nested.push(p.to_string());
                        }
                    }
                }
            }
        }
        if depth < 3 {
            let mut seen = HashSet::new();
            let unique: Vec<String> = nested.iter().filter(|n| seen.insert(n.clone())).take(64).cloned().collect();
            for name in unique {
                let entry = match zip_read_entry(&mut archive, &name, 64 * 1024 * 1024) {
                    Ok(Some(e)) => e,
                    _ => continue,
                };
                // 嵌套归档只贡献依赖身份，不提取图标
                if let Ok(child) = std::thread::scope(|s| {
                    let handle = s.spawn(|| parse_mod_archive(&entry.bytes, file_path, &name, depth + 1, false));
                    handle.join()
                }) {
                    if let Some(provs) = child.get("provides").and_then(|v| v.as_array()) {
                        for p in provs {
                            if let (Some(id), Some(ver)) = (p.get("id").and_then(|v| v.as_str()), p.get("version").and_then(|v| v.as_str())) {
                                provides.push((id.to_string(), ver.to_string()));
                            }
                        }
                    }
                    if let Some(jreqs) = child.get("javaRequirements").and_then(|v| v.as_array()) {
                        for r in jreqs {
                            let mut rr = r.clone();
                            if let Some(src) = r.get("source").and_then(|v| v.as_str()) {
                                rr["source"] = json!(format!("{file_name} > {src}"));
                            }
                            java_requirements.push(rr);
                        }
                    }
                }
            }
        }
        // provides 去重（按 id 保留首个，Map 语义）
        let mut seen = HashSet::new();
        let unique_provides: Vec<Value> = provides
            .into_iter()
            .filter(|(id, _)| !id.is_empty())
            .filter(|(id, _)| seen.insert(id.clone()))
            .map(|(id, version)| json!({ "id": id, "version": version }))
            .collect();
        out["provides"] = json!(unique_provides);
        if !java_requirements.is_empty() {
            out["javaRequirements"] = json!(java_requirements);
        }
        // 图标（≤512KB，dataURL png）
        if include_icons {
            if let Some(icon) = first.get("icon").and_then(|v| v.as_str()) {
                if let Ok(Some(entry)) = zip_read_entry(&mut archive, icon, 512 * 1024) {
                    use base64::Engine;
                    out["iconDataUrl"] = json!(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(&entry.bytes)));
                }
            }
        }
        Ok(out)
    })();
    match outcome {
        Ok(v) => v,
        Err(e) => build_error(&e),
    }
}

/// parseModFile()：单文件解析；损坏 jar → 固定错误文案
pub fn parse_mod_file(file_path: &str) -> Value {
    let file_name = installer::basename_of(file_path);
    match std::fs::read(file_path) {
        Ok(bytes) => parse_mod_archive(&bytes, file_path, &file_name, 0, true),
        Err(_) => json!({
            "filePath": file_path,
            "fileName": file_name,
            "id": "",
            "name": "",
            "version": "",
            "loader": Value::Null,
            "mcRange": "",
            "dependencies": [],
            "error": "文件损坏或不是有效的 jar 文件",
        }),
    }
}

/// expandJarPaths()：文件/文件夹（一层）展开为 .jar 列表；非 jar 逐个给出原因
pub fn expand_jar_paths(paths: &[String]) -> (Vec<String>, Vec<String>) {
    let mut files = Vec::new();
    let mut skipped = Vec::new();
    for p in paths {
        match std::fs::metadata(p) {
            Ok(st) if st.is_dir() => {
                let mut jars: Vec<String> = std::fs::read_dir(p)
                    .map(|rd| {
                        rd.flatten()
                            .map(|e| e.file_name().to_string_lossy().into_owned())
                            .filter(|n| n.to_lowercase().ends_with(".jar"))
                            .map(|n| Path::new(p).join(n).to_string_lossy().into_owned())
                            .collect()
                    })
                    .unwrap_or_default();
                jars.sort();
                if !jars.is_empty() {
                    files.extend(jars);
                } else {
                    skipped.push(format!("{}（文件夹内无 .jar）", installer::basename_of(p)));
                }
            }
            Ok(_) => {
                if p.to_lowercase().ends_with(".jar") {
                    files.push(p.clone());
                } else {
                    skipped.push(installer::basename_of(p));
                }
            }
            Err(_) => skipped.push(installer::basename_of(p)),
        }
    }
    (files, skipped)
}

// ---------------- modScan.ts / modScanWorker.ts ----------------

/// scanModDirectory()：analysis/icons 含图标，catalog 不含；hash=true 时附 sha1；
/// 传入 names 时同时计算 CurseForge 指纹并接受 .jar.disabled/.zip
pub fn scan_mod_directory(dir: &str, hash: bool, names: Option<&[String]>, purpose: &str) -> Result<Vec<Value>, String> {
    let include_icons = purpose != "catalog";
    let requested: Option<HashSet<String>> = names.map(|n| n.iter().cloned().collect());
    let entries = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.to_string()),
    };
    let mut result: Vec<Value> = Vec::new();
    for entry in entries.flatten() {
        let is_file = entry.file_type().map(|t| t.is_file()).unwrap_or(false);
        if !is_file {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        static JAR_ZIP: OnceLock<regex::Regex> = OnceLock::new();
        static JAR: OnceLock<regex::Regex> = OnceLock::new();
        let accept = match &requested {
            Some(set) => {
                if !set.contains(&name) {
                    false
                } else {
                    JAR_ZIP.get_or_init(|| regex::Regex::new(r"(?i)\.(?:jar(?:\.disabled)?|zip)$").unwrap()).is_match(&name)
                }
            }
            None => JAR.get_or_init(|| regex::Regex::new(r"(?i)\.jar$").unwrap()).is_match(&name),
        };
        if !accept {
            continue;
        }
        let file = entry.path();
        let file_str = file.to_string_lossy().into_owned();
        match std::fs::read(&file) {
            Ok(data) => {
                let mut info = parse_mod_archive(&data, &file_str, &name, 0, include_icons);
                if include_icons && info.get("iconDataUrl").is_none() {
                    if let Ok(bytes) = read_zip_entry(&data, "pack.png", 512 * 1024) {
                        use base64::Engine;
                        info["iconDataUrl"] = json!(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)));
                    }
                }
                if hash {
                    let mut h = Sha1::new();
                    h.update(&data);
                    info["sha1"] = json!(hex::encode(h.finish()));
                }
                if names.is_some() {
                    info["fingerprint"] = json!(curse_fingerprint(&data) as u64);
                }
                result.push(info);
            }
            Err(_) => {
                result.push(json!({
                    "fileName": name,
                    "filePath": file_str,
                    "error": "文件损坏或不可读取",
                }));
            }
        }
    }
    Ok(result)
}

fn read_zip_entry(data: &[u8], name: &str, max_bytes: u64) -> Result<Vec<u8>, String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(data)).map_err(|e| e.to_string())?;
    let mut file = archive.by_name(name).map_err(|e| e.to_string())?;
    if file.size() > max_bytes {
        return Err("过大".into());
    }
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    Ok(buf)
}

/// scanManagedModDirectory()：management 含 disabled 文件；查重/运行分析保持仅启用
pub fn scan_managed_mod_directory(dir: &str, purpose: &str) -> Result<Vec<Value>, String> {
    let names: Vec<String> = match std::fs::read_dir(dir) {
        Ok(rd) => rd
            .flatten()
            .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| regex::Regex::new(r"(?i)\.jar(?:\.disabled)?$").unwrap().is_match(n))
            .collect(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.to_string()),
    };
    scan_mod_directory(dir, true, Some(&names), purpose)
}

// ---------------- 运行实例钩子 + 空闲校验 ----------------

pub type RunningIdsFn = Arc<dyn Fn() -> Vec<String> + Send + Sync>;

static RUNNING_IDS: Mutex<Option<RunningIdsFn>> = Mutex::new(None);

/// launch.rs 移植后注册 getRunningVersionIds 等价实现
pub fn set_running_version_ids_provider(f: RunningIdsFn) {
    *RUNNING_IDS.lock().unwrap() = Some(f);
}

pub fn running_version_ids() -> HashSet<String> {
    let f = RUNNING_IDS.lock().unwrap().clone();
    f.map(|f| f().into_iter().collect()).unwrap_or_default()
}

fn list_all_installed(app: &tauri::AppHandle) -> Vec<Value> {
    let settings = crate::settings::get(app);
    versions_list_installed(&settings, true)
}

fn versions_list_installed(settings: &Value, all: bool) -> Vec<Value> {
    // 对应原版 versions.listAllInstalled（复用 crate::versions 的扫描）
    crate::versions::list_installed(settings, all).as_array().cloned().unwrap_or_default()
}

/// assertModsIdle()：使用该模组目录的游戏正在运行 → 拒绝
pub fn assert_mods_idle(app: &tauri::AppHandle, dir: &Path) -> Result<(), String> {
    let running = running_version_ids();
    if running.is_empty() {
        return Ok(());
    }
    let settings = crate::settings::get(app);
    for v in versions_list_installed(&settings, true) {
        let id = v.get("id").and_then(|x| x.as_str()).unwrap_or("");
        if !running.contains(id) {
            continue;
        }
        let folder = v.get("folder").and_then(|x| x.as_str()).unwrap_or("").to_string();
        let folder = if folder.is_empty() { installer::game_dir(app) } else { folder };
        if let Ok(other) = resolve_resource_directory(app, &folder, id, "mods") {
            if same_fs_path(&other, dir) {
                return Err("使用该模组目录的游戏正在运行，请先退出游戏".into());
            }
        }
    }
    Ok(())
}

/// assertInstanceIdle()（instanceCenter 等价）：目录被运行实例占用 → 拒绝
pub fn assert_instance_idle(app: &tauri::AppHandle, dir: &Path) -> Result<(), String> {
    let running = running_version_ids();
    if running.is_empty() {
        return Ok(());
    }
    let settings = crate::settings::get(app);
    for v in versions_list_installed(&settings, true) {
        let id = v.get("id").and_then(|x| x.as_str()).unwrap_or("");
        if !running.contains(id) {
            continue;
        }
        let folder = v.get("folder").and_then(|x| x.as_str()).unwrap_or("").to_string();
        let folder = if folder.is_empty() { installer::game_dir(app) } else { folder };
        if let Ok(j) = installer::read_version_json(app, &folder, id) {
            let state = installer::instance_directory_state(app, &folder, id, &j);
            if let Some(p) = state.get("path").and_then(|x| x.as_str()) {
                if same_fs_path(Path::new(p), dir) {
                    return Err("使用该目录的游戏正在运行，请退出游戏后重试".into());
                }
            }
        }
    }
    Ok(())
}

// ---------------- modTransaction.ts ----------------

/// safeModName()
pub fn safe_mod_name(name: &str) -> Result<String, String> {
    let ok = !name.contains(['\\', '/', ':', '\0'])
        && installer::basename_of(name) == name
        && regex::Regex::new(r"(?i)^.+\.jar(?:\.disabled)?$").unwrap().is_match(name);
    if !ok {
        return Err("无效的模组文件名".into());
    }
    Ok(name.to_string())
}

/// validateModFile()
pub fn validate_mod_file(dir: &Path, name: &str, sha1: Option<&str>) -> Result<(), String> {
    safe_mod_name(name)?;
    let p = dir.join(name);
    let meta = std::fs::symlink_metadata(&p).map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err("模组不是普通文件".into());
    }
    if let Some(expect) = sha1 {
        if installer::sha1_file_hex(&p)? != expect.to_lowercase() {
            return Err(format!("模组文件已变化，请重新检查：{name}"));
        }
    }
    Ok(())
}

pub fn mod_hash(file: &Path) -> Result<String, String> {
    installer::sha1_file_hex(file)
}

#[derive(Clone, Debug)]
pub struct ModReplacement {
    pub old_name: Option<String>,
    pub old_sha1: Option<String>,
    pub name: String,
    pub sha1: String,
    pub url: Option<String>,
    pub size: Option<u64>,
}

/// 备份接线点（changeProtection → BackupStore，归 instance-center/backup 子系统）
pub type ModBackupHook = Arc<dyn Fn(&Path, &[String], &str) -> Result<(), String> + Send + Sync>;
static MOD_BACKUP_HOOK: Mutex<Option<ModBackupHook>> = Mutex::new(None);

pub fn set_mod_backup_hook(hook: ModBackupHook) {
    *MOD_BACKUP_HOOK.lock().unwrap() = Some(hook);
}

fn protect_mod_change(dir: &Path, names: &[String], title: &str) -> Result<(), String> {
    let hook = MOD_BACKUP_HOOK.lock().unwrap().clone();
    if let Some(hook) = hook {
        hook(dir, names, title)?;
    }
    Ok(())
}

/// replaceModFiles()：暂存区下载 → 校验 → 备份 → 原子替换 → 失败回滚
pub fn replace_mod_files(
    app: &tauri::AppHandle,
    rec: &Arc<TaskRecord>,
    dir: &Path,
    items: &[ModReplacement],
    validate: Option<&(dyn Fn() -> Result<(), String> + Sync)>,
    on_progress: Option<&(dyn Fn(f64) + Sync)>,
) -> Result<(), String> {
    let mut names: HashSet<String> = HashSet::new();
    let mut olds: HashSet<String> = HashSet::new();
    for i in items {
        safe_mod_name(&i.name)?;
        if !regex::Regex::new(r"^[a-f0-9]{40}$").unwrap().is_match(&i.sha1)
            || !i.url.as_deref().map(|u| u.starts_with("https://")).unwrap_or(false)
        {
            return Err("文件缺少可信哈希或下载地址".into());
        }
        let k = i.name.to_lowercase();
        if !names.insert(k) {
            return Err(format!("目标文件名重复：{}", i.name));
        }
        if let Some(old) = &i.old_name {
            safe_mod_name(old)?;
            if !olds.insert(old.to_lowercase()) {
                return Err("重复的源文件".into());
            }
            validate_mod_file(dir, old, i.old_sha1.as_deref())?;
        }
    }
    let stage = {
        let parent = dir.parent().unwrap_or(dir);
        let mut created: Option<PathBuf> = None;
        for _ in 0..10 {
            let candidate = parent.join(format!(".kamucl-mod-change-{}", crate::accounts::uuid_v4()));
            match std::fs::create_dir(&candidate) {
                Ok(()) => {
                    created = Some(candidate);
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.to_string()),
            }
        }
        created.ok_or_else(|| "创建暂存目录失败".to_string())?
    };

    let mut backups: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut written: Vec<(PathBuf, String)> = Vec::new();
    let mut phase = "下载新模组".to_string();
    let mut can_clean = true;
    let result = (|| -> Result<(), String> {
        let mirror = download::mirror_from_settings(app);
        let total: u64 = items.iter().map(|i| i.size.unwrap_or(0)).sum();
        let finished_bytes = std::sync::atomic::AtomicU64::new(0);
        let finished_count = std::sync::atomic::AtomicU64::new(0);
        for (n, i) in items.iter().enumerate() {
            let dest = stage.join(format!("new-{n}-{}", i.name));
            let url = i.url.clone().unwrap_or_default();
            let progress = |done: u64, ftotal: u64, _wire: u64| {
                if let Some(cb) = on_progress {
                    let frac = if total > 0 {
                        (finished_bytes.load(Ordering::SeqCst) + done.min(ftotal)) as f64 / total as f64
                    } else {
                        let count = items.len() as f64;
                        finished_count.load(Ordering::SeqCst) as f64 / count
                    };
                    cb(frac.min(1.0));
                }
            };
            let opts = DownloadFileOpts {
                sha1: Some(&i.sha1),
                sha512: None,
                sha256: None,
                size: i.size,
                alternatives: &[],
                reuse_dirs: &[],
                reuse_files: &[],
                mirror,
                max_attempts: 4,
                source_pool: None,
                progress: Some(&progress),
                on_wait: None,
            };
            download::download_file(app, rec, &url, &dest, &opts)?;
            finished_bytes.fetch_add(i.size.unwrap_or_else(|| std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0)), Ordering::SeqCst);
            finished_count.fetch_add(1, Ordering::SeqCst);
        }
        phase = "检查模组状态".into();
        if let Some(v) = validate {
            v()?;
        }
        for i in items {
            if let Some(old) = &i.old_name {
                validate_mod_file(dir, old, i.old_sha1.as_deref())?;
            }
            if i.old_name.as_deref() != Some(&i.name) && dir.join(&i.name).exists() {
                return Err(format!("目标文件已存在，未覆盖：{}", i.name));
            }
        }
        phase = "保存修改前备份".into();
        {
            let mut list: Vec<String> = Vec::new();
            for i in items {
                if let Some(old) = &i.old_name {
                    list.push(old.clone());
                }
                list.push(i.name.clone());
            }
            list.sort();
            list.dedup();
            protect_mod_change(dir, &list, "模组版本修改前")?;
        }
        phase = "检查模组状态".into();
        if let Some(v) = validate {
            v()?;
        }
        for i in items {
            if let Some(old) = &i.old_name {
                validate_mod_file(dir, old, i.old_sha1.as_deref())?;
            }
        }
        check_cancel(rec)?;
        phase = "替换模组文件".into();
        can_clean = false; // 外部改动阻止回滚时保留原件
        let mut rolled: Result<(), String> = Ok(());
        {
            // 旧文件改名进暂存区
            for (n, i) in items.iter().enumerate() {
                if let Some(old) = &i.old_name {
                    let original = dir.join(old);
                    let backup = stage.join(format!("old-{n}"));
                    std::fs::rename(&original, &backup).map_err(|e| e.to_string())?;
                    backups.push((original, backup));
                }
            }
            // 新文件落位（EXCL：绝不覆盖）
            for (n, i) in items.iter().enumerate() {
                let src = stage.join(format!("new-{n}-{}", i.name));
                let file = dir.join(&i.name);
                match std::fs::OpenOptions::new().write(true).create_new(true).open(&file) {
                    Ok(mut out) => {
                        let mut inp = std::fs::File::open(&src).map_err(|e| e.to_string())?;
                        std::io::copy(&mut inp, &mut out).map_err(|e| e.to_string())?;
                        written.push((file, i.sha1.clone()));
                    }
                    Err(e) => {
                        rolled = Err(e.to_string());
                        break;
                    }
                }
            }
        }
        if let Err(e) = rolled {
            // 回滚：删除已写入 + 恢复备份
            let mut rollback_err: Option<String> = None;
            for (file, sha1) in &written {
                if file.exists() {
                    if let Ok(h) = mod_hash(file) {
                        if h == *sha1 {
                            let _ = std::fs::remove_file(file);
                        }
                    }
                }
            }
            for (original, backup) in &backups {
                if let Err(re) = std::fs::copy(backup, original) {
                    rollback_err = Some(re.to_string());
                }
            }
            if let Some(re) = rollback_err {
                return Err(format!("文件被外部修改，自动恢复未完成；原文件已保存在 {}。原因：{}", stage.to_string_lossy(), re));
            }
            can_clean = true;
            return Err(e);
        }
        can_clean = true;
        Ok(())
    })();
    let final_result = match result {
        Ok(()) => Ok(()),
        Err(e) => Err(format!("{phase}失败：{e}")),
    };
    // 可恢复时清理暂存区（外部改动阻止回滚时保留原件）
    if can_clean {
        let _ = std::fs::remove_dir_all(&stage);
    }
    final_result
}

// ---------------- 运行实例元数据 ----------------

fn resolve_instance_meta(app: &tauri::AppHandle, folder: &str, id: &str) -> Result<(String, Option<&'static str>, Option<String>), String> {
    let j = installer::read_version_json(app, folder, id)?;
    let meta = installer::resolve_instance_metadata(&j, &|pid: &str| installer::read_version_json(app, &installer::folder_of_version(app, pid), pid).ok());
    Ok((meta.mc_version, meta.loader, meta.loader_version))
}

// ---------------- modUpdates.ts ----------------

/// mapUpdateEntries()：同 sha1 = 已最新；无响应 = 未匹配来源；有响应且文件不同 = 可更新
fn map_update_entries(entries: &mut [Value], by_hash: &Value) {
    for entry in entries.iter_mut() {
        let sha1 = entry.get("sha1").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let version = by_hash.get(&sha1).cloned().unwrap_or(Value::Null);
        if version.is_null() {
            continue;
        }
        // hash 被 Modrinth 识别即标记来源；无可用文件时不产生更新项
        entry["source"] = json!("modrinth");
        let files = version.get("files").and_then(|v| v.as_array()).cloned().unwrap_or_default();
        if files.is_empty() {
            continue;
        }
        let file = files.iter().find(|f| f.get("primary").and_then(|v| v.as_bool()).unwrap_or(false)).unwrap_or(&files[0]);
        let url = file.get("url").and_then(|v| v.as_str()).unwrap_or("");
        let filename = file.get("filename").and_then(|v| v.as_str()).unwrap_or("");
        if url.is_empty() || filename.is_empty() {
            continue;
        }
        let new_sha1 = file.pointer("/hashes/sha1").and_then(|v| v.as_str()).map(str::to_string);
        if new_sha1.as_deref() == Some(sha1.as_str()) {
            entry["alreadyLatest"] = json!(true);
            continue;
        }
        entry["update"] = json!({
            "projectId": version.get("project_id").and_then(|v| v.as_str()).unwrap_or(""),
            "versionId": version.get("id").and_then(|v| v.as_str()).unwrap_or(""),
            "versionNumber": version.get("version_number").and_then(|v| v.as_str()).unwrap_or(""),
            "fileName": filename,
            "url": url,
            "sha1": new_sha1,
            "size": file.get("size").and_then(|v| v.as_u64()),
        });
    }
}

/// checkModUpdates()：按 jar sha1 在 Modrinth version_files 批量反查
pub fn check_mod_updates(app: &tauri::AppHandle, dir: &Path, folder: &str, version_id: &str) -> Result<Value, String> {
    let scanned = scan_managed_mod_directory(&dir.to_string_lossy(), "analysis")?;
    let (mc, loader, _lv) = resolve_instance_meta(app, folder, version_id)?;
    let mut report = json!({ "mcVersion": mc, "loader": loader.unwrap_or(""), "entries": [] });
    if scanned.is_empty() {
        return Ok(report);
    }
    if mc.is_empty() || loader.is_none() {
        return Err("实例缺少加载器或 Minecraft 版本元数据，无法检测更新".into());
    }
    let hashes: Vec<String> = scanned
        .iter()
        .filter(|i| i.get("error").is_none() && i.get("sha1").and_then(|v| v.as_str()).map(|s| !s.is_empty()).unwrap_or(false))
        .map(|i| i.get("sha1").and_then(|v| v.as_str()).unwrap_or("").to_string())
        .collect();
    let mut entries: Vec<Value> = scanned
        .iter()
        .filter(|i| i.get("error").is_none() && i.get("sha1").and_then(|v| v.as_str()).map(|s| !s.is_empty()).unwrap_or(false))
        .map(|i| {
            json!({
                "fileName": i.get("fileName").cloned().unwrap_or(json!("")),
                "name": i.get("name").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).or_else(|| i.get("id").and_then(|v| v.as_str()).filter(|s| !s.is_empty())).unwrap_or_else(|| i.get("fileName").and_then(|v| v.as_str()).unwrap_or("")),
                "modId": i.get("id").and_then(|v| v.as_str()).unwrap_or(""),
                "currentVersion": i.get("version").and_then(|v| v.as_str()).unwrap_or(""),
                "sha1": i.get("sha1").cloned().unwrap_or(json!("")),
                "source": Value::Null,
                "alreadyLatest": false,
                "update": Value::Null,
            })
        })
        .collect();
    let body = json!({
        "hashes": hashes,
        "algorithm": "sha1",
        "loaders": [loader.unwrap_or("")],
        "game_versions": [mc],
    });
    let by_hash = mr_post(app, "/version_files/update", &body, UA_UPDATES, 15_000)?;
    map_update_entries(&mut entries, &by_hash);
    for e in &entries {
        if let Some(project) = e.pointer("/update/projectId").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
            let sha1 = e.get("sha1").and_then(|v| v.as_str()).unwrap_or("");
            remember_mod_identity(app, dir, sha1, &format!("modrinth:{project}"));
        }
    }
    report["entries"] = json!(entries);
    Ok(report)
}

/// applyModUpdates()：按所选下载源校验后备份替换；单项失败不影响其他项
pub fn apply_mod_updates(app: &tauri::AppHandle, dir: &Path, items: &[Value]) -> Result<Value, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let rec = tasks::register_task("模组更新", "download");
    let result = download::with_file_job(dir, Some(rec.as_ref()), || -> Result<Value, String> {
        let mut results: Vec<Value> = Vec::new();
        for item in items {
            let file_name = item.get("fileName").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let old_sha1 = item.get("oldSha1").and_then(|v| v.as_str()).map(str::to_string);
            let url = item.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let target_name = item.get("targetName").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let sha1 = item.get("sha1").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let size = item.get("size").and_then(|v| v.as_u64());
            let run = || -> Result<(), String> {
                assert_mods_idle(app, dir)?;
                validate_mod_file(dir, &file_name, old_sha1.as_deref())?;
                let old_hash = mod_hash(&dir.join(&file_name))?;
                if is_mod_locked(app, dir, &old_hash) {
                    return Err("此模组已锁定，请解除锁定后更新".into());
                }
                let name = format!(
                    "{}{}",
                    regex::Regex::new(r"(?i)\.disabled$").unwrap().replace(&target_name, ""),
                    if regex::Regex::new(r"(?i)\.disabled$").unwrap().is_match(&file_name) { ".disabled" } else { "" }
                );
                let items = vec![ModReplacement {
                    old_name: Some(file_name.clone()),
                    old_sha1: Some(old_hash.clone()),
                    name,
                    sha1: sha1.clone(),
                    url: Some(url.clone()),
                    size,
                }];
                let validate = || -> Result<(), String> {
                    assert_mods_idle(app, dir)?;
                    if is_mod_locked(app, dir, &old_hash) {
                        return Err("此模组已锁定".into());
                    }
                    Ok(())
                };
                replace_mod_files(app, &rec, dir, &items, Some(&validate), None)?;
                transfer_mod_lock(app, dir, &old_hash, &sha1);
                Ok(())
            };
            match run() {
                Ok(()) => results.push(json!({ "fileName": file_name, "ok": true })),
                Err(e) => results.push(json!({ "fileName": file_name, "ok": false, "error": e })),
            }
        }
        Ok(json!(results))
    });
    tasks::finish_task(&rec.id);
    result
}

// ---------------- modManagement.ts ----------------

/// modCatalog()
pub fn mod_catalog(app: &tauri::AppHandle, version_id: &str, folder: &str) -> Result<(PathBuf, Vec<Value>), String> {
    let dir = resolve_resource_directory(app, folder, version_id, "mods")?;
    let entries = scan_managed_mod_directory(&dir.to_string_lossy(), "catalog")?;
    let mut list: Vec<Value> = Vec::new();
    for i in entries {
        let sha1 = i.get("sha1").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let mut item = json!({
            "fileName": i.get("fileName").cloned().unwrap_or(json!("")),
            "name": i.get("name").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(Value::from).unwrap_or_else(|| i.get("fileName").cloned().unwrap_or(json!(""))),
            "version": i.get("version").and_then(|v| v.as_str()).unwrap_or(""),
            "sha1": sha1,
        });
        if let Some(fp) = i.get("fingerprint") {
            item["fingerprint"] = fp.clone();
        }
        if !sha1.is_empty() {
            item["identity"] = json!(mod_identity(app, &dir, &sha1));
            item["locked"] = json!(is_mod_locked(app, &dir, &sha1));
        }
        if let Some(err) = i.get("error") {
            item["error"] = err.clone();
        }
        list.push(item);
    }
    Ok((dir, list))
}

/// identify()：Modrinth 批量哈希反查 + CurseForge 指纹（离线失败静默，身份保持稳定）
pub fn identify(app: &tauri::AppHandle, dir: &Path, mods: &[Value]) -> Result<(), String> {
    let missing: Vec<Value> = mods
        .iter()
        .filter(|m| {
            m.get("sha1").and_then(|v| v.as_str()).map(|s| !s.is_empty()).unwrap_or(false)
                && mod_identity(app, dir, m.get("sha1").and_then(|v| v.as_str()).unwrap_or("")).starts_with("sha1:")
        })
        .cloned()
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    let mut by_hash: Value = json!({});
    for chunk in missing.chunks(1000) {
        let hashes: Vec<String> = chunk
            .iter()
            .map(|m| m.get("sha1").and_then(|v| v.as_str()).unwrap_or("").to_string())
            .collect();
        let r = mgmt_json("https://api.modrinth.com/v2/version_files", Some(&json!({ "hashes": hashes, "algorithm": "sha1" })), &[])?;
        if let Some(map) = r.as_object() {
            if let Some(target) = by_hash.as_object_mut() {
                for (k, v) in map {
                    target.insert(k.clone(), v.clone());
                }
            }
        }
    }
    for m in &missing {
        let sha1 = m.get("sha1").and_then(|v| v.as_str()).unwrap_or("");
        if let Some(id) = by_hash.get(sha1).and_then(|v| v.get("project_id")).and_then(|v| v.as_str()) {
            remember_mod_identity(app, dir, sha1, &format!("modrinth:{id}"));
        }
    }
    let cf: Vec<Value> = missing
        .iter()
        .filter(|m| {
            let sha1 = m.get("sha1").and_then(|v| v.as_str()).unwrap_or("");
            by_hash.get(sha1).and_then(|v| v.get("project_id")).is_none() && m.get("fingerprint").is_some()
        })
        .cloned()
        .collect();
    if cf.is_empty() {
        return Ok(());
    }
    let ch = community::cf_channel(app);
    let headers: Vec<(&str, &str)> = if ch.official { vec![("x-api-key", &ch.key)] } else { vec![] };
    let fingerprints: Vec<u64> = cf.iter().filter_map(|m| m.get("fingerprint").and_then(|v| v.as_u64())).collect();
    let r = mgmt_json(&format!("{}/fingerprints", ch.base), Some(&json!({ "fingerprints": fingerprints })), &headers)?;
    for m in &cf {
        let sha1 = m.get("sha1").and_then(|v| v.as_str()).unwrap_or("");
        let fp = m.get("fingerprint").and_then(|v| v.as_u64());
        let matched = r
            .pointer("/data/exactMatches")
            .and_then(|v| v.as_array())
            .and_then(|list| {
                list.iter().find(|x| {
                    x.pointer("/file/fileFingerprint").and_then(|v| v.as_u64()) == fp
                        && x.pointer("/file/hashes").and_then(|v| v.as_array()).map(|hashes| {
                            hashes.iter().any(|h| h.get("algo").and_then(|v| v.as_u64()) == Some(1) && h.get("value").and_then(|v| v.as_str()).map(|s| s.to_lowercase() == sha1).unwrap_or(false))
                        }).unwrap_or(false)
                })
            });
        if let Some(matched) = matched {
            let id = matched.pointer("/file/modId").and_then(|v| v.as_u64()).map(|v| v.to_string()).unwrap_or_else(|| matched.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string());
            remember_mod_identity(app, dir, sha1, &format!("curseforge:{id}"));
        }
    }
    Ok(())
}

/// setModsEnabled()：批量启停（.disabled 后缀交换，绝不覆盖）
pub fn set_mods_enabled(app: &tauri::AppHandle, version_id: &str, folder: &str, names: &[String], enabled: bool) -> Result<Value, String> {
    let dir = resolve_resource_directory(app, folder, version_id, "mods")?;
    let rec = tasks::register_task("批量启停模组", "download");
    let result = download::with_file_job(&dir, Some(rec.as_ref()), || -> Result<Value, String> {
        assert_mods_idle(app, &dir)?;
        if names.len() > 10_000 {
            return Err("单次最多处理10000个模组".into());
        }
        let unique: Vec<String> = {
            let mut seen = HashSet::new();
            names.iter().filter(|n| seen.insert((*n).clone())).cloned().collect()
        };
        let valid_names: Vec<String> = unique.iter().filter(|n| safe_mod_name(n).is_ok()).cloned().collect();
        if !valid_names.is_empty() {
            let mut list: Vec<String> = Vec::new();
            for n in &valid_names {
                let base = regex::Regex::new(r"(?i)\.disabled$").unwrap().replace(n, "").to_string();
                list.push(n.clone());
                list.push(format!("{base}{}", if enabled { "" } else { ".disabled" }));
            }
            protect_mod_change(&dir, &list, "批量启停模组前")?;
        }
        let mut results: Vec<Value> = Vec::new();
        for name in &unique {
            let run = || -> Result<Value, String> {
                safe_mod_name(name)?;
                let base = regex::Regex::new(r"(?i)\.disabled$").unwrap().replace(name, "").to_string();
                let target = format!("{base}{}", if enabled { "" } else { ".disabled" });
                if !dir.join(name).exists() && dir.join(&target).exists() {
                    validate_mod_file(&dir, &target, None)?;
                    return Ok(json!({ "fileName": name, "name": target, "ok": true }));
                }
                validate_mod_file(&dir, name, None)?;
                if name != &target {
                    if dir.join(&target).exists() {
                        return Err("目标文件已存在，未覆盖".into());
                    }
                    std::fs::copy(dir.join(name), dir.join(&target)).map_err(|e| e.to_string())?;
                    if let Err(e) = std::fs::remove_file(dir.join(name)) {
                        let _ = std::fs::remove_file(dir.join(&target));
                        return Err(e.to_string());
                    }
                }
                Ok(json!({ "fileName": name, "name": target, "ok": true }))
            };
            match run() {
                Ok(v) => results.push(v),
                // 对应原版 String(e)：Error 对象序列化带 "Error: " 前缀
                Err(e) => results.push(json!({ "fileName": name, "ok": false, "error": format!("Error: {e}") })),
            }
        }
        Ok(json!(results))
    });
    tasks::finish_task(&rec.id);
    result
}

/// lockMods()
pub fn lock_mods(app: &tauri::AppHandle, version_id: &str, folder: &str, names: &[String], locked: bool) -> Result<Value, String> {
    let dir = resolve_resource_directory(app, folder, version_id, "mods")?;
    let (dir2, mods) = mod_catalog(app, version_id, folder)?;
    let _ = dir2;
    let selected: Vec<Value> = mods.iter().filter(|m| names.contains(&m.get("fileName").and_then(|v| v.as_str()).unwrap_or("").to_string())).cloned().collect();
    // Hash identity remains stable when offline.
    let _ = identify(app, &dir, &selected);
    let mut results: Vec<Value> = Vec::new();
    for name in names {
        let run = || -> Result<(), String> {
            safe_mod_name(name)?;
            let m = selected.iter().find(|m| m.get("fileName").and_then(|v| v.as_str()) == Some(name.as_str()));
            let sha1 = m.and_then(|m| m.get("sha1")).and_then(|v| v.as_str()).unwrap_or("");
            if sha1.is_empty() {
                return Err("无法校验模组文件".into());
            }
            validate_mod_file(&dir, name, Some(sha1))?;
            set_mod_locked(app, &dir, sha1, locked);
            Ok(())
        };
        match run() {
            Ok(()) => results.push(json!({ "fileName": name, "ok": true })),
            Err(e) => results.push(json!({ "fileName": name, "ok": false, "error": format!("Error: {e}") })),
        }
    }
    Ok(json!(results))
}

// ---------------- modPlanStore.ts ----------------

struct PlanEntry {
    value: Value,
    expires: u64,
    users: u32,
    retired: bool,
}

pub struct PlanStore {
    inner: Mutex<(HashMap<String, PlanEntry>, VecDeque<String>)>,
    ttl_ms: u64,
    limit: usize,
}

impl PlanStore {
    fn new(ttl_ms: u64, limit: usize) -> Self {
        Self { inner: Mutex::new((HashMap::new(), VecDeque::new())), ttl_ms, limit }
    }

    fn set(&self, id: &str, value: Value) {
        let (map, order) = &mut *self.inner.lock().unwrap();
        let expires = now_ms() + self.ttl_ms;
        map.insert(id.to_string(), PlanEntry { value, expires, users: 0, retired: false });
        order.push_back(id.to_string());
        self.trim_locked(map, order);
    }

    fn trim_locked(&self, map: &mut HashMap<String, PlanEntry>, order: &mut VecDeque<String>) {
        // 运行中的文件事务（users>0）不参与淘汰
        order.retain(|id| map.contains_key(id));
        let mut unused: Vec<String> = Vec::new();
        for id in order.iter() {
            if let Some(e) = map.get(id) {
                if e.users == 0 {
                    unused.push(id.clone());
                }
            }
        }
        let excess = unused.len().saturating_sub(self.limit);
        for id in unused.into_iter().take(excess) {
            map.remove(&id);
        }
    }

    fn get(&self, id: &str) -> Option<Value> {
        let (map, order) = &mut *self.inner.lock().unwrap();
        let fresh = map.get(id).map(|e| !e.retired && e.expires > now_ms()).unwrap_or(false);
        if !fresh {
            map.remove(id);
            return None;
        }
        map.get(id).map(|e| e.value.clone())
    }

    fn lease(&self, id: &str) -> bool {
        let (map, _) = &mut *self.inner.lock().unwrap();
        match map.get_mut(id) {
            Some(e) if !e.retired && e.expires > now_ms() => {
                e.users += 1;
                true
            }
            _ => false,
        }
    }

    fn release(&self, id: &str) {
        let (map, order) = &mut *self.inner.lock().unwrap();
        let mut drop_it = false;
        if let Some(e) = map.get_mut(id) {
            e.users = e.users.saturating_sub(1);
            if e.users == 0 && (e.retired || e.expires <= now_ms()) {
                drop_it = true;
            }
        }
        if drop_it {
            map.remove(id);
            order.retain(|x| x != id);
        }
    }

    fn delete(&self, id: &str) {
        let (map, order) = &mut *self.inner.lock().unwrap();
        if let Some(e) = map.get_mut(id) {
            e.retired = true;
            if e.users == 0 {
                map.remove(id);
                order.retain(|x| x != id);
            }
        }
    }

    fn delete_matching(&self, pred: impl Fn(&Value) -> bool) {
        let (map, order) = &mut *self.inner.lock().unwrap();
        let ids: Vec<String> = map
            .iter()
            .filter(|(_, e)| !e.retired && pred(&e.value))
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            if let Some(e) = map.get_mut(&id) {
                e.retired = true;
                if e.users == 0 {
                    map.remove(&id);
                    order.retain(|x| x != &id);
                }
            }
        }
    }
}

static CHOICES: OnceLock<PlanStore> = OnceLock::new();
static CHANGES: OnceLock<PlanStore> = OnceLock::new();
static DEPENDENCY_CHANGES: OnceLock<PlanStore> = OnceLock::new();

fn choices_store() -> &'static PlanStore {
    CHOICES.get_or_init(|| PlanStore::new(30 * 60_000, 8))
}
fn changes_store() -> &'static PlanStore {
    CHANGES.get_or_init(|| PlanStore::new(30 * 60_000, 8))
}
fn dependency_changes_store() -> &'static PlanStore {
    DEPENDENCY_CHANGES.get_or_init(|| PlanStore::new(30 * 60_000, 8))
}

/// discardModVersionPlan()：关闭列表同时回收其派生计划
pub fn discard_mod_version_plan(id: &str) {
    if id.len() > 64 {
        return;
    }
    choices_store().delete(id);
    changes_store().delete(id);
    dependency_changes_store().delete(id);
    let source_id = id.to_string();
    changes_store().delete_matching(move |v| v.get("_sourceId").and_then(|x| x.as_str()) == Some(source_id.as_str()));
}

fn compatible_choice_file(file: &Value, mc: &str, loader: &str) -> bool {
    let url = file.get("url").and_then(|v| v.as_str()).unwrap_or("");
    let sha1 = file.get("sha1").and_then(|v| v.as_str()).unwrap_or("");
    let file_name = file.get("fileName").and_then(|v| v.as_str()).unwrap_or("");
    url.starts_with("https://")
        && regex::Regex::new(r"^[a-f0-9]{40}$").unwrap().is_match(sha1)
        && file.get("gameVersions").and_then(|v| v.as_array()).map(|a| a.iter().any(|x| x.as_str() == Some(mc))).unwrap_or(false)
        && file.get("loaders").and_then(|v| v.as_array()).map(|a| a.iter().any(|x| x.as_str() == Some(loader))).unwrap_or(false)
        && installer::basename_of(file_name) == file_name
        && !file_name.contains(['\\', '/', ':', '\0'])
        && regex::Regex::new(r"(?i)\.jar$").unwrap().is_match(file_name)
}

/// modVersionChoices()
pub fn mod_version_choices(app: &tauri::AppHandle, version_id: &str, folder: &str, file_name: &str) -> Result<Value, String> {
    safe_mod_name(file_name)?;
    let (dir, mods) = mod_catalog(app, version_id, folder)?;
    let dir2 = dir.clone();
    let _ = dir2;
    let mod_item = mods.iter().find(|m| m.get("fileName").and_then(|v| v.as_str()) == Some(file_name)).cloned();
    let Some(mod_item) = mod_item else { return Err("无法读取该模组，请刷新后重试".into()) };
    let sha1 = mod_item.get("sha1").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if sha1.is_empty() {
        return Err("无法读取该模组，请刷新后重试".into());
    }
    let _ = identify(app, &dir, &mods);
    let identity = mod_identity(app, &dir, &sha1);
    if identity.starts_with("sha1:") {
        return Err("未能在社区精确识别此文件，无法可靠提供其他版本；可手动下载并导入。".into());
    }
    let (source, id) = identity.split_once(':').ok_or_else(|| "未能在社区精确识别此文件，无法可靠提供其他版本；可手动下载并导入。".to_string())?;
    let (mc, loader, _lv) = resolve_instance_meta(app, folder, version_id)?;
    if mc.is_empty() || loader.is_none() {
        return Err("实例缺少游戏或加载器版本信息".into());
    }
    let loader_str = loader.unwrap_or("");
    let filter = community::FileFilter { mc_version: Some(mc.clone()), loader: Some(loader_str.to_string()), kind: Some("mod".into()) };
    let mut files = community_files(app, source, &id, Some(&filter))?;
    files.retain(|f| compatible_choice_file(f, &mc, loader_str));
    let value = json!({
        "id": crate::accounts::uuid_v4(),
        "fileName": file_name,
        "name": mod_item.get("name").cloned().unwrap_or(json!("")),
        "currentVersion": mod_item.get("version").cloned().unwrap_or(json!("")),
        "mcVersion": mc,
        "loader": loader_str,
        "locked": is_mod_locked(app, &dir, &sha1),
        "files": files,
    });
    let store_id = value.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    choices_store().set(&store_id, json!({
        "value": value,
        "dir": dir.to_string_lossy(),
        "folder": folder,
        "versionId": version_id,
        "mods": mods,
    }));
    Ok(value)
}

/// planModVersionChange()：目标版本 + 必要前置替换清单 + changelog
pub fn plan_mod_version_change(app: &tauri::AppHandle, id: &str, file_id: &str) -> Result<Value, String> {
    let c = choices_store().get(id).ok_or_else(|| "版本列表已过期，请重新打开".to_string())?;
    let value = c.get("value").cloned().unwrap_or(json!({}));
    let dir = PathBuf::from(c.get("dir").and_then(|v| v.as_str()).unwrap_or(""));
    let mods: Vec<Value> = c.get("mods").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let mc = value.get("mcVersion").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let loader = value.get("loader").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let file = value
        .get("files")
        .and_then(|v| v.as_array())
        .and_then(|files| files.iter().find(|f| f.get("fileId").and_then(|x| x.as_str()) == Some(file_id)))
        .cloned()
        .ok_or_else(|| "请选择列表内的适配版本".to_string())?;
    let mod_item = mods
        .iter()
        .find(|m| m.get("fileName").and_then(|v| v.as_str()) == value.get("fileName").and_then(|v| v.as_str()))
        .cloned()
        .unwrap_or(json!({}));
    let mod_file_name = mod_item.get("fileName").and_then(|v| v.as_str()).unwrap_or("");
    let disabled = regex::Regex::new(r"(?i)\.disabled$").unwrap().is_match(mod_file_name);
    let mut warnings: Vec<String> = Vec::new();
    let mut replacements: Vec<Value> = vec![json!({
        "oldName": mod_file_name,
        "oldSha1": mod_item.get("sha1").cloned().unwrap_or(json!("")),
        "name": format!("{}{}", file.get("fileName").and_then(|v| v.as_str()).unwrap_or(""), if disabled { ".disabled" } else { "" }),
        "sha1": file.get("sha1").cloned().unwrap_or(json!("")),
        "url": file.get("url").cloned().unwrap_or(json!("")),
        "size": file.get("size").cloned().unwrap_or(json!(0)),
    })];
    let mut files: Vec<Value> = vec![json!({
        "fileName": file.get("fileName").cloned().unwrap_or(json!("")),
        "version": file.get("version").cloned().unwrap_or(json!("")),
        "dependency": false,
    })];
    let lock_sha1 = mod_item.get("sha1").and_then(|v| v.as_str()).unwrap_or("");
    if is_mod_locked(app, &dir, lock_sha1) {
        warnings.push("此模组已锁定。确认后仅本次切换版本，新文件仍保持锁定。".into());
    }
    // 必要前置 BFS
    let mut queue: Vec<Value> = vec![file.clone()];
    let mut visited: HashSet<String> = HashSet::new();
    let mut used: HashSet<String> = HashSet::new();
    used.insert(file.get("fileName").and_then(|v| v.as_str()).unwrap_or("").to_lowercase());
    let mut index = 0usize;
    while index < queue.len() {
        if queue.len() > 100 {
            return Err("前置依赖过多，请手动处理".into());
        }
        let current = queue[index].clone();
        index += 1;
        let deps = current.get("dependencies").and_then(|v| v.as_array()).cloned().unwrap_or_default();
        for d in deps {
            let required = d.get("required").and_then(|v| v.as_bool()).unwrap_or(false);
            if !required || disabled {
                continue;
            }
            let source = current.get("source").and_then(|v| v.as_str()).unwrap_or("modrinth").to_string();
            let dep_project = d.get("projectId").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let key = format!("{}:{}", source, dep_project);
            if !visited.insert(key.clone()) {
                continue;
            }
            let dep_file = d.get("fileId").and_then(|v| v.as_str()).map(str::to_string);
            let installed = mods.iter().find(|m| {
                let m_sha = m.get("sha1").and_then(|v| v.as_str()).unwrap_or("");
                mod_identity(app, &dir, m_sha) == key
                    && !regex::Regex::new(r"(?i)\.disabled$").unwrap().is_match(m.get("fileName").and_then(|v| v.as_str()).unwrap_or(""))
                    && m.get("fileName").and_then(|v| v.as_str()) != Some(mod_file_name)
            });
            if installed.is_some() && dep_file.is_none() {
                continue;
            }
            let outcome: Result<Value, String> = (|| {
                let f = match &dep_file {
                    Some(fid) => community_exact_file(app, &source, Some(&dep_project), fid)?,
                    None => community_files(app, &source, &dep_project, Some(&community::FileFilter { mc_version: Some(mc.clone()), loader: Some(loader.clone()), kind: Some("mod".into()) }))?
                        .into_iter()
                        .find(|f| compatible_choice_file(f, &mc, &loader))
                        .ok_or_else(|| "无兼容版本".to_string())?,
                };
                if !compatible_choice_file(&f, &mc, &loader) {
                    return Err("无兼容版本".into());
                }
                if let Some(inst) = installed {
                    let inst_sha = inst.get("sha1").and_then(|v| v.as_str()).unwrap_or("");
                    if inst_sha != f.get("sha1").and_then(|v| v.as_str()).unwrap_or("") {
                        warnings.push(format!(
                            "{} 与所需前置版本 {} 不一致，本次保留原文件，请检查兼容性",
                            inst.get("fileName").and_then(|v| v.as_str()).unwrap_or(""),
                            f.get("version").and_then(|v| v.as_str()).unwrap_or("")
                        ));
                    }
                    return Err("__skip__".into());
                }
                if used.contains(&f.get("fileName").and_then(|v| v.as_str()).unwrap_or("").to_lowercase()) {
                    return Err("前置文件名冲突".into());
                }
                Ok(f)
            })();
            match outcome {
                Ok(f) => {
                    used.insert(f.get("fileName").and_then(|v| v.as_str()).unwrap_or("").to_lowercase());
                    replacements.push(json!({
                        "name": f.get("fileName").cloned().unwrap_or(json!("")),
                        "sha1": f.get("sha1").cloned().unwrap_or(json!("")),
                        "url": f.get("url").cloned().unwrap_or(json!("")),
                        "size": f.get("size").cloned().unwrap_or(json!(0)),
                    }));
                    files.push(json!({
                        "fileName": f.get("fileName").cloned().unwrap_or(json!("")),
                        "version": f.get("version").cloned().unwrap_or(json!("")),
                        "dependency": true,
                    }));
                    queue.push(f);
                }
                Err(e) if e == "__skip__" => {}
                Err(e) => warnings.push(format!(
                    "必要前置 {} 未安装：Error: {}",
                    if dep_project.is_empty() { dep_file.as_deref().unwrap_or("未知") } else { dep_project.as_str() },
                    e
                )),
            }
        }
    }
    let mut changelog = "暂无版本说明".to_string();
    {
        let changelog_result: Result<String, String> = (|| {
            if file.get("source").and_then(|v| v.as_str()) == Some("modrinth") {
                let r = get_json(&format!("https://api.modrinth.com/v2/version/{}", community::enc(file_id)), UA_MGMT, 15_000, &[])?;
                Ok(r.get("changelog").and_then(|v| v.as_str()).unwrap_or("").to_string())
            } else {
                let project = file.get("projectId").and_then(|v| v.as_str()).unwrap_or("");
                if project.is_empty() {
                    return Ok(String::new());
                }
                let ch = community::cf_channel(app);
                let headers: Vec<(&str, &str)> = if ch.official { vec![("x-api-key", &ch.key)] } else { vec![] };
                let r = get_json(&format!("{}/mods/{}/files/{}/changelog", ch.base, project, file_id), UA_MGMT, 15_000, &headers)?;
                Ok(r.get("data").and_then(|v| v.as_str()).unwrap_or("").to_string())
            }
        })();
        match changelog_result {
            Ok(s) if !s.is_empty() => changelog = s,
            Ok(_) => changelog = "版本说明暂时无法获取，不影响已校验的适配文件选择".into(),
            Err(_) => changelog = "版本说明暂时无法获取，不影响已校验的适配文件选择".into(),
        }
    }
    if choices_store().get(id).as_ref() != Some(&c) {
        return Err("版本列表已关闭或过期，请重新打开".into());
    }
    let stripped = regex::Regex::new(r"<[^>]*>").unwrap().replace_all(&changelog, "").to_string();
    let view = json!({
        "id": crate::accounts::uuid_v4(),
        "fileName": mod_file_name,
        "target": file,
        "files": files,
        "warnings": warnings,
        "changelog": stripped.chars().take(20_000).collect::<String>(),
    });
    let plan_id = view.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    changes_store().set(&plan_id, json!({
        "view": view,
        "sourceId": id,
        "dir": dir.to_string_lossy(),
        "replacements": replacements,
        "mods": mods,
        "lockHash": lock_sha1,
    }));
    Ok(view)
}

/// validateSnapshot()：应用前比对目录快照
fn validate_snapshot(dir: &Path, mods: &[Value]) -> Result<(), String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| regex::Regex::new(r"(?i)\.jar(?:\.disabled)?$").unwrap().is_match(n))
        .collect();
    names.sort();
    let mut expected: Vec<String> = mods.iter().map(|m| m.get("fileName").and_then(|v| v.as_str()).unwrap_or("").to_string()).collect();
    expected.sort();
    if names != expected {
        return Err("检查后模组列表发生变化，请重新检查".into());
    }
    for m in mods {
        validate_mod_file(dir, m.get("fileName").and_then(|v| v.as_str()).unwrap_or(""), m.get("sha1").and_then(|v| v.as_str()))?;
    }
    Ok(())
}

fn replacements_from(v: &[Value]) -> Vec<ModReplacement> {
    v.iter()
        .map(|r| ModReplacement {
            old_name: r.get("oldName").and_then(|x| x.as_str()).map(str::to_string),
            old_sha1: r.get("oldSha1").and_then(|x| x.as_str()).map(str::to_string),
            name: r.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            sha1: r.get("sha1").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            url: r.get("url").and_then(|x| x.as_str()).map(str::to_string),
            size: r.get("size").and_then(|x| x.as_u64()),
        })
        .collect()
}

/// applyModVersionChange()
pub fn apply_mod_version_change(app: &tauri::AppHandle, id: &str, confirmed: bool) -> Result<Value, String> {
    let c = changes_store().get(id).ok_or_else(|| "切换计划已过期".to_string())?;
    let view = c.get("view").cloned().unwrap_or(json!({}));
    let dir = PathBuf::from(c.get("dir").and_then(|v| v.as_str()).unwrap_or(""));
    let lock_hash = c.get("lockHash").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let warnings = view.get("warnings").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    if (!warnings.is_empty() || is_mod_locked(app, &dir, &lock_hash)) && !confirmed {
        return Err("请确认锁定模组与前置提醒".into());
    }
    if !changes_store().lease(id) {
        return Err("切换计划已过期".into());
    }
    let rec = tasks::register_task("切换模组版本", "download");
    let result = download::with_file_job(&dir, Some(rec.as_ref()), || -> Result<Value, String> {
        assert_mods_idle(app, &dir)?;
        let mods: Vec<Value> = c.get("mods").and_then(|v| v.as_array()).cloned().unwrap_or_default();
        validate_snapshot(&dir, &mods)?;
        let replacements = replacements_from(c.get("replacements").and_then(|v| v.as_array()).unwrap_or(&Vec::new()));
        let validate = || -> Result<(), String> {
            assert_mods_idle(app, &dir)?;
            validate_snapshot(&dir, &mods)?;
            if is_mod_locked(app, &dir, &lock_hash) && !confirmed {
                return Err("模组已锁定，请重新确认".into());
            }
            Ok(())
        };
        replace_mod_files(app, &rec, &dir, &replacements, Some(&validate), None)?;
        if let Some(first) = replacements.first() {
            transfer_mod_lock(app, &dir, &lock_hash, &first.sha1);
        }
        changes_store().delete(id);
        Ok(json!({ "ok": true }))
    });
    tasks::finish_task(&rec.id);
    changes_store().release(id);
    result
}

/// planMissingDependencies()（center:dependencyPlan 复用）
pub fn plan_missing_dependencies(app: &tauri::AppHandle, version_id: &str, folder: &str, file_name: &str) -> Result<Value, String> {
    let list = mod_version_choices(app, version_id, folder, file_name)?;
    let mut plan_id: Option<String> = None;
    let result = (|| -> Result<Value, String> {
        let stored = choices_store().get(list.get("id").and_then(|v| v.as_str()).unwrap_or("")).ok_or_else(|| "版本列表已过期，请重新打开".to_string())?;
        let mods: Vec<Value> = stored.get("mods").and_then(|v| v.as_array()).cloned().unwrap_or_default();
        let mod_item = mods
            .iter()
            .find(|m| m.get("fileName").and_then(|v| v.as_str()) == Some(file_name))
            .cloned()
            .ok_or_else(|| "无法读取该模组，请刷新后重试".to_string())?;
        if regex::Regex::new(r"(?i)\.disabled$").unwrap().is_match(file_name) {
            return Err("该模组已禁用，无需安装运行前置".into());
        }
        let mod_sha = mod_item.get("sha1").and_then(|v| v.as_str()).unwrap_or("");
        let exact = list
            .get("files")
            .and_then(|v| v.as_array())
            .and_then(|files| files.iter().find(|f| f.get("sha1").and_then(|x| x.as_str()) == Some(mod_sha)))
            .cloned()
            .ok_or_else(|| "社区列表中没有与本地哈希完全一致的版本，无法可靠确定前置，请手动核对".to_string())?;
        let plan = plan_mod_version_change(app, list.get("id").and_then(|v| v.as_str()).unwrap_or(""), exact.get("fileId").and_then(|v| v.as_str()).unwrap_or(""))?;
        plan_id = plan.get("id").and_then(|v| v.as_str()).map(str::to_string);
        let change = changes_store().get(plan_id.as_deref().unwrap_or("")).ok_or_else(|| "切换计划已过期".to_string())?;
        let mut warnings: Vec<String> = plan
            .get("warnings")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|w| w.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        warnings.retain(|w| !w.starts_with("此模组已锁定"));
        if !warnings.is_empty() {
            if let Some(pid) = &plan_id {
                changes_store().delete(pid);
            }
            return Err(format!("无法完整确定兼容前置：{}", warnings.join("；")));
        }
        let files: Vec<Value> = plan.get("files").and_then(|v| v.as_array()).map(|a| a.iter().filter(|f| f.get("dependency").and_then(|x| x.as_bool()).unwrap_or(false)).cloned().collect()).unwrap_or_default();
        if files.is_empty() {
            if let Some(pid) = &plan_id {
                changes_store().delete(pid);
            }
            return Err("社区元数据未发现可安全补充的缺失前置，请核对日志中的版本限制".into());
        }
        // 独立存储，防止版本切换端点误用
        if let Some(pid) = &plan_id {
            changes_store().delete(pid);
            dependency_changes_store().set(pid, json!({
                "dir": change.get("dir").cloned().unwrap_or(json!("")),
                "mods": change.get("mods").cloned().unwrap_or(json!([])),
                "replacements": change.get("replacements").and_then(|v| v.as_array()).map(|a| a[1..].to_vec()).unwrap_or_default(),
            }));
        }
        Ok(json!({
            "id": plan.get("id").cloned().unwrap_or(json!("")),
            "fileName": plan.get("fileName").cloned().unwrap_or(json!("")),
            "target": plan.get("target").cloned().unwrap_or(Value::Null),
            "files": files,
            "warnings": [],
            "changelog": "仅补充所列必要前置，保留当前模组及其锁定状态。",
        }))
    })();
    choices_store().delete(list.get("id").and_then(|v| v.as_str()).unwrap_or(""));
    if let Some(pid) = &plan_id {
        changes_store().delete(pid);
    }
    result
}

/// applyMissingDependencies()（center:dependencyApply 复用）
pub fn apply_missing_dependencies(app: &tauri::AppHandle, id: &str, confirmed: bool, rec: &Arc<TaskRecord>, on_progress: Option<&(dyn Fn(f64) + Sync)>) -> Result<Value, String> {
    let c = dependency_changes_store().get(id).filter(|_| confirmed).ok_or_else(|| "请重新检查并确认前置安装清单".to_string())?;
    if !dependency_changes_store().lease(id) {
        return Err("请重新检查并确认前置安装清单".into());
    }
    let result = (|| -> Result<Value, String> {
        let dir = PathBuf::from(c.get("dir").and_then(|v| v.as_str()).unwrap_or(""));
        let mods: Vec<Value> = c.get("mods").and_then(|v| v.as_array()).cloned().unwrap_or_default();
        download::with_file_job(&dir, Some(rec.as_ref()), || -> Result<Value, String> {
            assert_mods_idle(app, &dir)?;
            validate_snapshot(&dir, &mods)?;
            let replacements = replacements_from(c.get("replacements").and_then(|v| v.as_array()).unwrap_or(&Vec::new()));
            let validate = || -> Result<(), String> {
                assert_mods_idle(app, &dir)?;
                validate_snapshot(&dir, &mods)
            };
            replace_mod_files(app, rec, &dir, &replacements, Some(&validate), on_progress)?;
            dependency_changes_store().delete(id);
            Ok(json!({ "ok": true }))
        })
    })();
    dependency_changes_store().release(id);
    result
}

// ---------------- modInstallPlan.ts ----------------

struct PrivatePlan {
    directory: PathBuf,
    roots: Vec<String>,
    root_files: HashMap<String, String>,
    downloads: Vec<Value>,
    pinned_files: HashMap<String, Value>,
    expires: u64,
    view: Value,
}

fn plans_store() -> &'static Mutex<HashMap<String, PrivatePlan>> {
    static P: OnceLock<Mutex<HashMap<String, PrivatePlan>>> = OnceLock::new();
    P.get_or_init(|| Mutex::new(HashMap::new()))
}

fn clean_plan(id: &str, plan: &PrivatePlan) {
    let _ = std::fs::remove_dir_all(&plan.directory);
    plans_store().lock().unwrap().remove(id);
}

fn provided(mod_info: &Value) -> Vec<Value> {
    match mod_info.get("provides").and_then(|v| v.as_array()) {
        Some(list) if !list.is_empty() => list.clone(),
        _ => vec![json!({ "id": mod_info.get("id").cloned().unwrap_or(json!("")), "version": mod_info.get("version").cloned().unwrap_or(json!("")) })],
    }
}

/// requirementsFor()
fn requirements_for(mod_info: &Value, target: &Value) -> Vec<Value> {
    let loader = normalize_loader(target.get("loader").and_then(|v| v.as_str()));
    let variants = mod_info.get("variants").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let matched: Vec<&Value> = variants.iter().filter(|v| loader.is_some() && loader == normalize_loader(v.get("loader").and_then(|x| x.as_str()))).collect();
    if !matched.is_empty() {
        return matched.iter().flat_map(|v| v.get("requirements").and_then(|r| r.as_array()).cloned().unwrap_or_default()).collect();
    }
    match mod_info.get("requirements").and_then(|v| v.as_array()) {
        Some(list) if !list.is_empty() => list.clone(),
        _ => mod_info
            .get("dependencies")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().map(|id| json!({ "id": id, "range": "*" })).collect())
            .unwrap_or_default(),
    }
}

/// missingRequirements()
fn missing_requirements(mods: &[Value], target: &Value, available: &[Value]) -> Vec<Value> {
    mods.iter()
        .flat_map(|m| requirements_for(m, target))
        .filter(|dep| {
            let dep_id = dep.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let dep_range = dep.get("range").and_then(|v| v.as_str()).unwrap_or("");
            !available.iter().any(|m| {
                provided(m).iter().any(|p| {
                    p.get("id").and_then(|v| v.as_str()) == Some(dep_id)
                        && matches_version_range(dep_range, p.get("version").and_then(|v| v.as_str()).unwrap_or(""))
                })
            })
        })
        .collect()
}

/// installedMods()：实例 mods 目录已启用 jar 解析
fn installed_mods(target: &Value) -> Vec<Value> {
    let game_dir = target.get("gameDirectory").and_then(|v| v.as_str()).unwrap_or("");
    let dir = Path::new(game_dir).join("mods");
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(&dir) else { return out };
    let mut names: Vec<String> = rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.to_lowercase().ends_with(".jar")).collect();
    names.sort();
    for n in names {
        let info = parse_mod_file(&dir.join(n).to_string_lossy());
        if info.get("error").is_none() {
            out.push(info);
        }
    }
    out
}

/// dependencyGraph()：必要传递依赖闭包；pinned 版本保持钉定，可选边不下载
fn dependency_graph(
    app: &tauri::AppHandle,
    roots: &[Value],
    target: &Value,
    pinned_files: &mut HashMap<String, Value>,
) -> Result<Vec<Value>, String> {
    let mut result: Vec<Value> = Vec::new();

    fn visit(
        app: &tauri::AppHandle,
        file: Value,
        target: &Value,
        roots: &[Value],
        seen: &mut HashSet<String>,
        projects: &mut HashMap<String, String>,
        result: &mut Vec<Value>,
        pinned_files: &mut HashMap<String, Value>,
    ) -> Result<(), String> {
        let key = format!(
            "{}:{}",
            file.get("source").and_then(|v| v.as_str()).unwrap_or(""),
            file.get("fileId").and_then(|v| v.as_str()).unwrap_or("")
        );
        if seen.contains(&key) {
            return Ok(());
        }
        if !community_file_matches_instance(&file, target) {
            return Err(format!(
                "{} 不支持 MC {} / {}",
                file.get("fileName").and_then(|v| v.as_str()).unwrap_or(""),
                target.get("mcVersion").and_then(|v| v.as_str()).unwrap_or(""),
                target.get("loader").and_then(|v| v.as_str()).unwrap_or("")
            ));
        }
        if seen.len() >= 96 {
            return Err("依赖图超过 96 项，请检查项目元数据".into());
        }
        let project_id = file.get("projectId").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let project = format!("{}:{}", file.get("source").and_then(|v| v.as_str()).unwrap_or(""), project_id);
        if !project_id.is_empty() {
            if let Some(prev) = projects.get(&project) {
                if prev != file.get("fileId").and_then(|v| v.as_str()).unwrap_or("") {
                    return Err(format!("前置版本冲突：{project_id}"));
                }
            }
            projects.insert(project, file.get("fileId").and_then(|v| v.as_str()).unwrap_or("").to_string());
        }
        seen.insert(key);
        let deps = file.get("dependencies").and_then(|v| v.as_array()).cloned().unwrap_or_default();
        for dep in deps {
            if !dep.get("required").and_then(|v| v.as_bool()).unwrap_or(false) {
                continue;
            }
            let dep_project = dep.get("projectId").and_then(|v| v.as_str()).unwrap_or("");
            let dep_file = dep.get("fileId").and_then(|v| v.as_str()).unwrap_or("");
            if file.get("source").and_then(|v| v.as_str()).unwrap_or("").is_empty() || (dep_project.is_empty() && dep_file.is_empty()) {
                return Err(format!(
                    "无法定位 {} 的必要前置，请手动安装",
                    file.get("fileName").and_then(|v| v.as_str()).unwrap_or("")
                ));
            }
            let source = file.get("source").and_then(|v| v.as_str()).unwrap_or("");
            let selected = roots.iter().find(|root| {
                root.get("source").and_then(|v| v.as_str()) == Some(source)
                    && (!dep_project.is_empty() && root.get("projectId").and_then(|v| v.as_str()) == Some(dep_project)
                        || root.get("fileId").and_then(|v| v.as_str()) == Some(dep_file))
            });
            if let Some(selected) = selected {
                if !dep_file.is_empty() && selected.get("fileId").and_then(|v| v.as_str()) != Some(dep_file) {
                    return Err(format!("前置版本冲突：{}", if dep_project.is_empty() { dep_file } else { dep_project }));
                }
            }
            let next = match selected {
                Some(s) => s.clone(),
                None => {
                    if !dep_file.is_empty() {
                        community_exact_file(app, source, if dep_project.is_empty() { None } else { Some(dep_project) }, dep_file)?
                    } else {
                        community_files(app, source, dep_project, Some(&community::FileFilter {
                            mc_version: Some(target.get("mcVersion").and_then(|v| v.as_str()).unwrap_or("").to_string()),
                            loader: Some(target.get("loader").and_then(|v| v.as_str()).unwrap_or("").to_string()),
                            kind: None,
                        }))?
                        .into_iter()
                        .next()
                        .ok_or_else(|| format!("前置 {dep_project} 没有兼容版本"))?
                    }
                }
            };
            if !dep_file.is_empty() && next.get("fileId").and_then(|v| v.as_str()) != Some(dep_file) {
                return Err(format!("前置版本冲突：{}", if dep_project.is_empty() { dep_file } else { dep_project }));
            }
            if !dep_file.is_empty() {
                pinned_files.insert(format!("{}:{}", next.get("source").and_then(|v| v.as_str()).unwrap_or(""), next.get("fileId").and_then(|v| v.as_str()).unwrap_or("")), next.clone());
            }
            visit(app, next, target, roots, seen, projects, result, pinned_files)?;
        }
        result.push(file);
        Ok(())
    }

    let mut seen = HashSet::new();
    let mut projects = HashMap::new();
    for root in roots {
        visit(app, root.clone(), target, roots, &mut seen, &mut projects, &mut result, pinned_files)?;
    }
    Ok(result)
}

/// fileHash()
fn file_hash(file: &Path) -> Result<String, String> {
    installer::sha1_file_hex(file)
}

/// 进度发射器（mods:prepare / mods:commit / migration 用）
pub struct TaskEmit {
    app: tauri::AppHandle,
    guard: Mutex<ProgressGuard>,
    task_id: String,
    task_title: String,
    operation_id: Option<String>,
    last_stage: Mutex<String>,
}

impl TaskEmit {
    pub fn new(app: &tauri::AppHandle, task_id: &str, task_title: &str, operation_id: Option<String>) -> Self {
        Self {
            app: app.clone(),
            guard: Mutex::new(ProgressGuard::new()),
            task_id: task_id.to_string(),
            task_title: task_title.to_string(),
            operation_id,
            last_stage: Mutex::new(String::new()),
        }
    }

    pub fn emit(&self, event: &Value) {
        let mut g = self.guard.lock().unwrap();
        let normalized = g.normalize(event);
        let stage = normalized.get("stage").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if stage != "error" && stage != "done" {
            *self.last_stage.lock().unwrap() = stage.clone();
        }
        let mut payload = normalized;
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("taskId".into(), json!(self.task_id));
            obj.insert("taskTitle".into(), json!(self.task_title));
            if let Some(op) = &self.operation_id {
                obj.insert("operationId".into(), json!(op));
            }
        }
        tasks::emit_progress(&self.app, payload);
    }

    pub fn last_stage(&self) -> String {
        self.last_stage.lock().unwrap().clone()
    }
}

/// 单文件下载 + 聚合进度（对应 prepareModInstall / executeModPlan 的 downloadAll 进度映射）
fn download_plan_file(
    app: &tauri::AppHandle,
    rec: &Arc<TaskRecord>,
    url: &str,
    dest: &Path,
    sha1: Option<&str>,
    size: Option<u64>,
    emit: &TaskEmit,
    text: &str,
) -> Result<(), String> {
    let mirror = download::MirrorPref::Official; // 原版 downloadAll(..., 8, 'official', signal)
    let est = Mutex::new(SmoothedSpeedEstimator::new());
    let bytes = std::sync::atomic::AtomicU64::new(0);
    let start = Instant::now();
    let paused_flag = rec.paused.clone();
    let text = text.to_string();
    let progress = move |done: u64, total: u64, wire: u64| {
        bytes.fetch_add(wire, Ordering::SeqCst);
        let remaining = if total > done { Some(total - done) } else { None };
        let snap = est.lock().unwrap().sample(bytes.load(Ordering::SeqCst), remaining, start.elapsed().as_millis() as f64, paused_flag.load(Ordering::SeqCst));
        let frac = if total > 0 { done as f64 / total as f64 } else { 0.0 };
        let mut ev = json!({
            "stage": "download",
            "progress": frac,
            "overall": frac * 0.85,
            "indeterminate": total == 0,
            "text": text,
            "speed": snap.speed_bps,
            "bytesDone": done,
        });
        if total > 0 {
            ev["bytesTotal"] = json!(total);
        }
        if let Some(eta) = snap.eta_seconds {
            ev["etaSeconds"] = json!(eta);
        }
        emit.emit(&ev);
    };
    let opts = DownloadFileOpts {
        sha1,
        sha512: None,
        sha256: None,
        size,
        alternatives: &[],
        reuse_dirs: &[],
        reuse_files: &[],
        mirror,
        max_attempts: 4,
        source_pool: None,
        progress: Some(&progress),
        on_wait: None,
    };
    download::download_file(app, rec, url, dest, &opts)
}

/// prepareModInstall()：预下载 + 兼容性/前置检测（不写入实例）
pub fn prepare_mod_install(
    app: &tauri::AppHandle,
    rec: &Arc<TaskRecord>,
    target: &Value,
    input: &Value,
    emit: &TaskEmit,
) -> Result<Value, String> {
    {
        let mut store = plans_store().lock().unwrap();
        let expired: Vec<String> = store.iter().filter(|(_, p)| p.expires < now_ms()).map(|(k, _)| k.clone()).collect();
        for id in expired {
            if let Some(p) = store.remove(&id) {
                let _ = std::fs::remove_dir_all(&p.directory);
            }
        }
        if store.len() >= 8 {
            return Err("待确认安装过多，请取消已有安装计划".into());
        }
    }
    let directory = std::env::temp_dir().join(format!("kamucl-mod-plan-{}", crate::accounts::uuid_v4()));
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let id = crate::accounts::uuid_v4();
    let mut plan = PrivatePlan {
        directory: directory.clone(),
        roots: Vec::new(),
        root_files: HashMap::new(),
        downloads: Vec::new(),
        pinned_files: HashMap::new(),
        expires: now_ms() + 20 * 60_000,
        view: json!({ "id": id, "target": target, "files": [], "missing": [], "warnings": [] }),
    };
    let mut run = || -> Result<(), String> {
        let mut roots: Vec<Value> = Vec::new();
        if let Some(file) = input.get("file").filter(|f| f.is_object()) {
            check_cancel(rec)?;
            let source = file.get("source").and_then(|v| v.as_str()).unwrap_or("");
            let project_id = file.get("projectId").and_then(|v| v.as_str()).unwrap_or("");
            if source.is_empty() || project_id.is_empty() {
                return Err("缺少项目来源，请重新打开下载页".into());
            }
            let exact = community_exact_file(app, source, Some(project_id), file.get("fileId").and_then(|v| v.as_str()).unwrap_or(""))?;
            if !community_file_matches_instance(&exact, target) {
                let gv: Vec<String> = exact.get("gameVersions").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str()).map(String::from).collect()).unwrap_or_default();
                let lds: Vec<String> = exact.get("loaders").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str()).map(String::from).collect()).unwrap_or_default();
                return Err(format!(
                    "所选 MOD 文件不支持目标 MC {} / {}；文件声明 MC {} / {}",
                    target.get("mcVersion").and_then(|v| v.as_str()).unwrap_or(""),
                    target.get("loader").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).unwrap_or("纯净版"),
                    if gv.is_empty() { "未知".to_string() } else { gv.join(" / ") },
                    if lds.is_empty() { "未知".to_string() } else { lds.join(" / ") }
                ));
            }
            let file_name = exact.get("fileName").and_then(|v| v.as_str()).unwrap_or("");
            let dest = directory.join(installer::basename_of(file_name));
            emit.emit(&json!({ "stage": "download", "progress": 0, "overall": 0, "indeterminate": true, "text": "读取所选 MOD，解析内置前置要求…" }));
            download_plan_file(
                app,
                rec,
                exact.get("url").and_then(|v| v.as_str()).unwrap_or(""),
                &dest,
                exact.get("sha1").and_then(|v| v.as_str()),
                exact.get("size").and_then(|v| v.as_u64()),
                emit,
                &format!("下载所选 MOD：{file_name}"),
            )?;
            plan.roots.push(dest.to_string_lossy().into_owned());
            plan.root_files.insert(format!("{}:{}", source, exact.get("fileId").and_then(|v| v.as_str()).unwrap_or("")), dest.to_string_lossy().into_owned());
            roots.push(exact);
        } else {
            let paths = input.get("paths").and_then(|v| v.as_array()).cloned().unwrap_or_default();
            for (index, file) in paths.iter().enumerate() {
                check_cancel(rec)?;
                let src = file.as_str().unwrap_or("");
                let dest_dir = directory.join(index.to_string());
                std::fs::create_dir_all(&dest_dir).map_err(|e| e.to_string())?;
                let dest = dest_dir.join(installer::basename_of(src));
                std::fs::copy(src, &dest).map_err(|e| e.to_string())?;
                plan.roots.push(dest.to_string_lossy().into_owned());
            }
        }
        check_cancel(rec)?;
        emit.emit(&json!({ "stage": "mod-prepare", "progress": 0, "overall": if input.get("file").is_some() { json!(0.85) } else { json!(0) }, "indeterminate": true, "text": "MOD 已准备，正在检测兼容性和必要前置（尚未安装）…" }));
        if plan.roots.is_empty() {
            return Err("没有待安装的 MOD".into());
        }
        let mods: Vec<Value> = plan.roots.iter().map(|p| parse_mod_file(p)).collect();
        for m in &mods {
            if !mod_matches_instance(m, target) {
                let reasons = mod_mismatch_reasons(m, target).join("；");
                return Err(format!("{}：{}", m.get("name").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).unwrap_or_else(|| m.get("fileName").and_then(|v| v.as_str()).unwrap_or("")), reasons));
            }
        }
        let installed = installed_mods(target);
        let mut available = installed.clone();
        available.extend(mods.iter().cloned());
        let missing = missing_requirements(&mods, target, &available);
        let graph = dependency_graph(app, &roots, target, &mut plan.pinned_files)?;
        check_cancel(rec)?;
        let root_keys: HashSet<String> = roots
            .iter()
            .map(|f| format!("{}:{}", f.get("source").and_then(|v| v.as_str()).unwrap_or(""), f.get("fileId").and_then(|v| v.as_str()).unwrap_or("")))
            .collect();
        plan.downloads = graph.into_iter().filter(|f| !root_keys.contains(&format!("{}:{}", f.get("source").and_then(|v| v.as_str()).unwrap_or(""), f.get("fileId").and_then(|v| v.as_str()).unwrap_or("")))).collect();
        // 本地拖入无仓库项目 ID：按 mod id 单独检索候选
        let mut missing_unique: Vec<Value> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for d in &missing {
            if seen.insert(d.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string()) {
                missing_unique.push(d.clone());
            }
        }
        let mut warnings: Vec<String> = Vec::new();
        for req in &missing_unique {
            check_cancel(rec)?;
            let req_id = req.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let normalized = req_id.replace(['-', '_'], "");
            let already = plan.downloads.iter().any(|f| {
                f.get("projectId")
                    .and_then(|v| v.as_str())
                    .map(|p| p.replace(['-', '_'], "") == normalized)
                    .unwrap_or(false)
            });
            if already {
                continue;
            }
            let candidate = dependency_find(app, &req_id, target)?;
            check_cancel(rec)?;
            match candidate {
                Some(c) => {
                    let mut pinned = HashMap::new();
                    let sub = dependency_graph(app, &[c], target, &mut pinned)?;
                    plan.downloads.extend(sub);
                }
                None => {
                    if plan.downloads.is_empty() {
                        warnings.push(format!(
                            "无法自动定位前置 {} {}，请手动补齐后重试",
                            req_id,
                            req.get("range").and_then(|v| v.as_str()).unwrap_or("*")
                        ));
                    }
                }
            }
        }
        // 已安装同哈希去重
        let installed_hashes: Mutex<HashMap<String, String>> = Mutex::new(HashMap::new());
        let unique: Vec<Value> = {
            let mut map: HashMap<String, Value> = HashMap::new();
            let mut order: Vec<String> = Vec::new();
            for f in &plan.downloads {
                let key = format!("{}:{}", f.get("source").and_then(|v| v.as_str()).unwrap_or(""), f.get("fileId").and_then(|v| v.as_str()).unwrap_or(""));
                if map.insert(key.clone(), f.clone()).is_none() {
                    order.push(key);
                }
            }
            order.into_iter().map(|k| map.remove(&k).unwrap()).collect()
        };
        plan.downloads = Vec::new();
        for file in unique {
            check_cancel(rec)?;
            let mut present = false;
            if let Some(sha1) = file.get("sha1").and_then(|v| v.as_str()) {
                for m in &installed {
                    let fp = m.get("filePath").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let mut cache = installed_hashes.lock().unwrap();
                    let hash = match cache.get(&fp) {
                        Some(h) => h.clone(),
                        None => {
                            let h = file_hash(Path::new(&fp))?;
                            cache.insert(fp.clone(), h.clone());
                            h
                        }
                    };
                    drop(cache);
                    if hash == sha1.to_lowercase() {
                        present = true;
                        break;
                    }
                }
            }
            if !present {
                plan.downloads.push(file);
            }
        }
        plan.view["missing"] = json!(missing.iter().map(|d| format!("{} {}", d.get("id").and_then(|v| v.as_str()).unwrap_or(""), d.get("range").and_then(|v| v.as_str()).unwrap_or("*"))).collect::<Vec<String>>());
        let mut files_view: Vec<Value> = mods
            .iter()
            .map(|m| json!({ "name": m.get("name").cloned().unwrap_or(json!("")), "version": m.get("version").cloned().unwrap_or(json!("")), "dependency": false, "fileName": m.get("fileName").cloned().unwrap_or(json!("")) }))
            .collect();
        for f in &plan.downloads {
            files_view.push(json!({
                "name": f.get("projectId").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(Value::from).unwrap_or_else(|| f.get("fileName").cloned().unwrap_or(json!(""))),
                "version": f.get("version").cloned().unwrap_or(json!("")),
                "dependency": true,
                "fileName": f.get("fileName").cloned().unwrap_or(json!("")),
                "source": f.get("source").cloned().unwrap_or(Value::Null),
                "projectId": f.get("projectId").cloned().unwrap_or(Value::Null),
            }));
        }
        plan.view["files"] = json!(files_view);
        if !warnings.is_empty() {
            plan.view["warnings"] = json!(warnings);
        }
        emit.emit(&json!({ "stage": "done", "progress": 1, "overall": 1, "indeterminate": false, "text": "预下载及检测完成，等待确认安装（尚未写入实例）" }));
        Ok(())
    };
    let result = run();
    match result {
        Ok(()) => {
            plans_store().lock().unwrap().insert(id.clone(), plan);
            Ok(plan_view_value(&id))
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&directory);
            Err(e)
        }
    }
}

fn plan_view_value(id: &str) -> Value {
    plans_store().lock().unwrap().get(id).map(|p| p.view.clone()).unwrap_or(Value::Null)
}

/// dependencyRepository.find()：检索仅作候选查找；下载 JAR 的实际 mod id/版本必须满足要求
fn dependency_find(app: &tauri::AppHandle, id: &str, target: &Value) -> Result<Option<Value>, String> {
    let q = json!({
        "keyword": id,
        "kind": "mod",
        "source": "all",
        "mcVersion": target.get("mcVersion").cloned().unwrap_or(json!("")),
        "loader": target.get("loader").cloned().unwrap_or(json!("")),
        "offset": 0,
        "limit": 20,
    });
    let hits = community_search_items(app, &q)?;
    let key = |s: &str| -> String {
        s.to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect()
    };
    for hit in hits {
        let slug = hit.get("slug").and_then(|v| v.as_str()).unwrap_or("");
        let title = hit.get("title").and_then(|v| v.as_str()).unwrap_or("");
        if key(slug) != key(id) && key(title) != key(id) {
            continue;
        }
        let files = community_files(
            app,
            hit.get("source").and_then(|v| v.as_str()).unwrap_or(""),
            hit.get("projectId").and_then(|v| v.as_str()).unwrap_or(""),
            Some(&community::FileFilter {
                mc_version: target.get("mcVersion").and_then(|v| v.as_str()).map(str::to_string),
                loader: target.get("loader").and_then(|v| v.as_str()).map(str::to_string),
                kind: None,
            }),
        )?;
        if let Some(first) = files.into_iter().next() {
            return Ok(Some(first));
        }
    }
    Ok(None)
}

/// discardModPlan()
pub fn discard_mod_plan(id: &str) {
    let mut store = plans_store().lock().unwrap();
    if let Some(plan) = store.remove(id) {
        let _ = std::fs::remove_dir_all(&plan.directory);
    }
}

/// executeModPlan()：下载前置 → 校验 → 事务写入；失败仅回滚本次创建的文件
pub fn execute_mod_plan(
    app: &tauri::AppHandle,
    rec: &Arc<TaskRecord>,
    id: &str,
    include_dependencies: bool,
    revalidate: &(dyn Fn(&Value) -> Result<Value, String> + Sync),
    emit: &TaskEmit,
) -> Result<String, String> {
    let plan = {
        let store = plans_store().lock().unwrap();
        match store.get(id) {
            Some(p) if p.expires >= now_ms() => PrivatePlan {
                directory: p.directory.clone(),
                roots: p.roots.clone(),
                root_files: p.root_files.clone(),
                downloads: p.downloads.clone(),
                pinned_files: p.pinned_files.clone(),
                expires: p.expires,
                view: p.view.clone(),
            },
            _ => return Err("安装计划已过期，请重新解析".into()),
        }
    };
    plans_store().lock().unwrap().remove(id); // A plan may only commit once, including concurrent invoke calls.
    let mut created: Vec<PathBuf> = Vec::new();
    let mut run = || -> Result<String, String> {
        let target = revalidate(&plan.view.get("target").cloned().unwrap_or(json!({})))?;
        if target.get("gameDirectory").and_then(|v| v.as_str()).unwrap_or("") != plan.view.pointer("/target/gameDirectory").and_then(|v| v.as_str()).unwrap_or("") {
            return Err("实例隔离目录已变更，请重新解析".into());
        }
        let mut installed = installed_mods(&target);
        let root_mods: Vec<Value> = plan.roots.iter().map(|p| parse_mod_file(p)).collect();
        let mut staged: Vec<Value> = root_mods.clone();
        let mut pinned_paths: HashMap<String, Value> = HashMap::new();
        let dependencies: Vec<(Value, PathBuf)> = if include_dependencies {
            plan.downloads
                .iter()
                .enumerate()
                .map(|(index, file)| {
                    let dest = plan.directory.join(format!("dep-{index}")).join(installer::basename_of(file.get("fileName").and_then(|v| v.as_str()).unwrap_or("")));
                    (file.clone(), dest)
                })
                .collect()
        } else {
            Vec::new()
        };
        emit.emit(&json!({
            "stage": if !dependencies.is_empty() { "download" } else { "mod-verify" },
            "progress": 0,
            "overall": 0,
            "indeterminate": true,
            "text": if !dependencies.is_empty() { format!("准备下载 {} 个必要前置…", dependencies.len()) } else { "正在校验已准备的 MOD…".to_string() }
        }));
        if !dependencies.is_empty() {
            let dep_count = dependencies.len();
            let est = Mutex::new(SmoothedSpeedEstimator::new());
            let bytes = std::sync::atomic::AtomicU64::new(0);
            let start = Instant::now();
            let paused_flag = rec.paused.clone();
            let text = format!("下载 {dep_count} 个必要前置");
            // 各文件 (done, total) 状态表（对应原版 downloadAll 聚合 fraction）
            let state: Mutex<Vec<(u64, u64)>> = Mutex::new(dependencies.iter().map(|(f, _)| (0u64, f.get("size").and_then(|v| v.as_u64()).unwrap_or(0))).collect());
            for (index, (file, dest)) in dependencies.iter().enumerate() {
                let progress = |done: u64, ftotal: u64, wire: u64| {
                    bytes.fetch_add(wire, Ordering::SeqCst);
                    let (done_sum, total_sum, all_known) = {
                        let mut st = state.lock().unwrap();
                        if let Some(slot) = st.get_mut(index) {
                            slot.0 = done.min(ftotal);
                            if ftotal > 0 {
                                slot.1 = ftotal;
                            }
                        }
                        let done_sum: u64 = st.iter().map(|(d, _)| *d).sum();
                        let total_sum: u64 = st.iter().map(|(_, t)| *t).sum();
                        let all_known = st.iter().all(|(_, t)| *t > 0);
                        (done_sum, total_sum, all_known)
                    };
                    let fraction = if all_known {
                        if total_sum > 0 { done_sum as f64 / total_sum as f64 } else { 1.0 }
                    } else {
                        0.0
                    };
                    let remaining = if total_sum > done_sum { Some(total_sum - done_sum) } else { None };
                    let snap = est.lock().unwrap().sample(bytes.load(Ordering::SeqCst), remaining, start.elapsed().as_millis() as f64, paused_flag.load(Ordering::SeqCst));
                    let mut ev = json!({
                        "stage": "download",
                        "progress": fraction,
                        "overall": fraction * 0.85,
                        "indeterminate": !all_known,
                        "text": text,
                        "speed": snap.speed_bps,
                        "bytesDone": done_sum,
                    });
                    if all_known && total_sum > 0 {
                        ev["bytesTotal"] = json!(total_sum);
                    }
                    if let Some(eta) = snap.eta_seconds {
                        ev["etaSeconds"] = json!(eta);
                    }
                    emit.emit(&ev);
                };
                let opts = DownloadFileOpts {
                    sha1: file.get("sha1").and_then(|v| v.as_str()),
                    sha512: None,
                    sha256: None,
                    size: file.get("size").and_then(|v| v.as_u64()),
                    alternatives: &[],
                    reuse_dirs: &[],
                    reuse_files: &[],
                    mirror: download::MirrorPref::Official,
                    max_attempts: 4,
                    source_pool: None,
                    progress: Some(&progress),
                    on_wait: None,
                };
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                download::download_file(app, rec, file.get("url").and_then(|v| v.as_str()).unwrap_or(""), dest, &opts)?;
            }
        }
        emit.emit(&json!({
            "stage": "mod-verify",
            "progress": 0,
            "overall": if !dependencies.is_empty() { json!(0.85) } else { json!(0) },
            "indeterminate": true,
            "text": "下载已完成，正在校验兼容性、哈希与前置…"
        }));
        for (file, dest) in &dependencies {
            check_cancel(rec)?;
            let m = parse_mod_file(&dest.to_string_lossy());
            if !mod_matches_instance(&m, &target) {
                let reasons = mod_mismatch_reasons(&m, &target).join("；");
                return Err(format!("{} 的 JAR 元数据不兼容目标实例：{}", file.get("fileName").and_then(|v| v.as_str()).unwrap_or(""), reasons));
            }
            let pinned_key = format!("{}:{}", file.get("source").and_then(|v| v.as_str()).unwrap_or(""), file.get("fileId").and_then(|v| v.as_str()).unwrap_or(""));
            if let Some(pinned) = plan.pinned_files.get(&pinned_key) {
                pinned_paths.insert(m.get("filePath").and_then(|v| v.as_str()).unwrap_or("").to_string(), pinned.clone());
            }
            // pinned 边要求精确文件；同 mod id 的既有提供者只能满足未钉定的兼容依赖
            let has_provider = installed.iter().any(|inst| provided(inst).iter().any(|p| p.get("id").and_then(|v| v.as_str()) == m.get("id").and_then(|v| v.as_str())));
            if pinned_files_contains(&plan.pinned_files, &pinned_key) || !has_provider {
                staged.push(m);
            }
        }
        let current = revalidate(&plan.view.get("target").cloned().unwrap_or(json!({})))?;
        if current.get("gameDirectory").and_then(|v| v.as_str()).unwrap_or("") != target.get("gameDirectory").and_then(|v| v.as_str()).unwrap_or("") {
            return Err("下载期间实例目录发生变化，未安装任何 MOD".into());
        }
        installed = installed_mods(&current);
        for m in &staged {
            if !mod_matches_instance(m, &current) {
                return Err(format!("{} 的兼容性已变化，请重新解析", m.get("name").and_then(|v| v.as_str()).unwrap_or("")));
            }
        }
        let mut staged_installed = staged.clone();
        staged_installed.extend(installed.iter().cloned());
        let missing = missing_requirements(&staged, &target, &staged_installed);
        if !missing.is_empty() {
            let list: Vec<String> = missing.iter().map(|d| format!("{} {}", d.get("id").and_then(|v| v.as_str()).unwrap_or(""), d.get("range").and_then(|v| v.as_str()).unwrap_or("*"))).collect();
            return Err(format!("未写入任何 MOD；仍缺少或版本不满足：{}。请补齐前置或处理旧版冲突后重试", list.join("、")));
        }
        // pinned 精确性复核（准备阶段可能省略已存在的同哈希文件）
        let mut hash_cache: HashMap<String, String> = HashMap::new();
        let hash_of = |file: &str, cache: &mut HashMap<String, String>| -> Result<String, String> {
            match cache.get(file) {
                Some(h) => Ok(h.clone()),
                None => {
                    let h = file_hash(Path::new(file))?;
                    cache.insert(file.to_string(), h.clone());
                    Ok(h)
                }
            }
        };
        for pinned in plan.pinned_files.values() {
            let pinned_key = format!("{}:{}", pinned.get("source").and_then(|v| v.as_str()).unwrap_or(""), pinned.get("fileId").and_then(|v| v.as_str()).unwrap_or(""));
            let downloaded = dependencies.iter().find(|(f, _)| format!("{}:{}", f.get("source").and_then(|v| v.as_str()).unwrap_or(""), f.get("fileId").and_then(|v| v.as_str()).unwrap_or("")) == pinned_key);
            let prepared = downloaded.map(|(_, dest)| dest.to_string_lossy().into_owned()).or_else(|| plan.root_files.get(&pinned_key).cloned());
            let expected_hash = pinned
                .get("sha1")
                .and_then(|v| v.as_str())
                .map(|s| s.to_lowercase())
                .or_else(|| prepared.as_deref().and_then(|p| hash_of(p, &mut hash_cache).ok()));
            let mut present = false;
            if let Some(expected) = &expected_hash {
                for m in staged.iter().chain(installed.iter()) {
                    let fp = m.get("filePath").and_then(|v| v.as_str()).unwrap_or("");
                    if hash_of(fp, &mut hash_cache)? == *expected {
                        present = true;
                        break;
                    }
                }
            }
            if !present {
                return Err(format!(
                    "未写入任何 MOD；必要前置 {} 要求精确版本 {}，已准备或已安装的文件已变化，请重新检测",
                    pinned.get("projectId").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).unwrap_or_else(|| pinned.get("fileName").and_then(|v| v.as_str()).unwrap_or("")),
                    pinned.get("fileId").and_then(|v| v.as_str()).unwrap_or("")
                ));
            }
        }
        let mut accepted: Vec<Value> = Vec::new();
        for m in &staged {
            let existing = installed.iter().chain(accepted.iter()).find(|inst| {
                provided(inst).iter().any(|p| p.get("id").and_then(|v| v.as_str()) == m.get("id").and_then(|v| v.as_str()))
            });
            if let Some(existing) = existing {
                let existing_hash = hash_of(existing.get("filePath").and_then(|v| v.as_str()).unwrap_or(""), &mut hash_cache)?;
                let staged_hash = hash_of(m.get("filePath").and_then(|v| v.as_str()).unwrap_or(""), &mut hash_cache)?;
                if existing_hash == staged_hash {
                    continue;
                }
                let pinned = pinned_paths.get(m.get("filePath").and_then(|v| v.as_str()).unwrap_or(""));
                if let Some(pinned) = pinned {
                    return Err(format!(
                        "前置版本冲突：{} 要求精确版本 {}，已有 {}；未写入任何 MOD，请先处理旧文件后重新检测",
                        m.get("id").and_then(|v| v.as_str()).unwrap_or(""),
                        pinned.get("fileId").and_then(|v| v.as_str()).unwrap_or(""),
                        existing.get("version").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).unwrap_or("其他版本")
                    ));
                }
                return Err(format!("已存在 {}，未覆盖或重复安装；请先处理旧文件", m.get("id").and_then(|v| v.as_str()).unwrap_or("")));
            }
            accepted.push(m.clone());
        }
        let mods_dir = Path::new(target.get("gameDirectory").and_then(|v| v.as_str()).unwrap_or("")).join("mods");
        std::fs::create_dir_all(&mods_dir).map_err(|e| e.to_string())?;
        emit.emit(&json!({ "stage": "mod-commit", "progress": 0, "overall": 0.95, "indeterminate": true, "text": format!("正在写入 {} 个 MOD…", accepted.len()) }));
        for m in &accepted {
            check_cancel(rec)?;
            let dest = mods_dir.join(installer::basename_of(m.get("fileName").and_then(|v| v.as_str()).unwrap_or("")));
            std::fs::OpenOptions::new().write(true).create_new(true).open(&dest).and_then(|mut o| {
                let mut inp = std::fs::File::open(m.get("filePath").and_then(|v| v.as_str()).unwrap_or(""))?;
                std::io::copy(&mut inp, &mut o)?;
                Ok(())
            }).map_err(|e| e.to_string())?;
            created.push(dest);
        }
        let message = format!("已安装 {} 个 MOD 到 {}", created.len(), mods_dir.to_string_lossy());
        emit.emit(&json!({ "stage": "done", "progress": 1, "overall": 1, "indeterminate": false, "text": message.clone() }));
        Ok(message)
    };
    let result = run();
    if result.is_err() {
        // 只回滚本次事务创建的文件，绝不动既有内容
        for file in &created {
            let _ = std::fs::remove_file(file);
        }
    }
    clean_plan(id, &plan);
    result
}

fn pinned_files_contains(pinned: &HashMap<String, Value>, key: &str) -> bool {
    pinned.contains_key(key)
}

// ---------------- modTargets.ts ----------------

/// scanModTargets()
pub fn scan_mod_targets(app: &tauri::AppHandle) -> Value {
    let settings = crate::settings::get(app);
    let folders: Vec<String> = settings
        .get("folders")
        .and_then(|f| f.as_array())
        .map(|a| a.iter().filter_map(|f| f.get("path").and_then(|p| p.as_str()).map(String::from)).collect())
        .unwrap_or_default();
    let mut versions: Vec<Value> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for folder in folders {
        let key = installer::path_identity(&folder);
        if !seen.insert(key) {
            continue;
        }
        match std::fs::metadata(&folder) {
            Ok(m) if m.is_dir() => {
                let canonical = installer::canonical_path(&folder).to_string_lossy().into_owned();
                versions.extend(crate::versions::scan_folder_public(&canonical, true));
            }
            Ok(_) => errors.push(format!("{folder}：不是文件夹")),
            Err(e) => errors.push(format!("{folder}：{}", e)),
        }
    }
    json!({ "versions": versions, "errors": errors })
}

/// selectModTarget()
pub fn select_mod_target(versions: &[Value], id: &str, folder: &str) -> Result<Value, String> {
    let target = versions.iter().find(|v| {
        v.get("id").and_then(|x| x.as_str()) == Some(id)
            && v.get("folder")
                .and_then(|x| x.as_str())
                .map(|f| installer::path_identity(f) == installer::path_identity(folder))
                .unwrap_or(false)
    });
    match target {
        Some(t) if t.get("incomplete").and_then(|x| x.as_bool()).unwrap_or(false) || t.get("failed").and_then(|x| x.as_bool()).unwrap_or(false) || t.get("gameDirectory").is_none() => {
            Err("目标实例已移除、不完整或未登记，请重新扫描".into())
        }
        Some(t) => Ok(t.clone()),
        None => Err("目标实例已移除、不完整或未登记，请重新扫描".into()),
    }
}

/// copyCompatibleMods()：拖入即装
pub fn copy_compatible_mods(files: &[String], target: &Value) -> Vec<Value> {
    let dir = Path::new(target.get("gameDirectory").and_then(|v| v.as_str()).unwrap_or("")).join("mods");
    let mut results: Vec<Value> = Vec::new();
    for file in files {
        let name = installer::basename_of(file);
        let run = || -> Result<(), String> {
            let m = parse_mod_file(file);
            if !mod_matches_instance(&m, target) {
                return Err(format!(
                    "不兼容：MC {} / {} {}",
                    target.get("mcVersion").and_then(|v| v.as_str()).unwrap_or(""),
                    target.get("loader").and_then(|v| v.as_str()).unwrap_or(""),
                    target.get("loaderVersion").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).unwrap_or("版本未知")
                ));
            }
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            std::fs::OpenOptions::new().write(true).create_new(true).open(dir.join(&name)).and_then(|mut o| {
                let mut inp = std::fs::File::open(file)?;
                std::io::copy(&mut inp, &mut o)?;
                Ok(())
            }).map_err(|e| e.to_string())?;
            Ok(())
        };
        match run() {
            Ok(()) => results.push(json!({ "fileName": name, "ok": true, "message": "已装入" })),
            Err(e) => {
                let message = if dir.join(&name).exists() {
                    "同名文件已存在，未覆盖".to_string()
                } else {
                    e
                };
                results.push(json!({ "fileName": name, "ok": false, "message": message }))
            }
        }
    }
    results
}

// ---------------- modIcons.ts ----------------

fn icons_cache_dir(app: &tauri::AppHandle) -> PathBuf {
    let dir = crate::settings::user_data_dir(app).join("cache").join("mod-icons-v1");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// matchModIcons()：精确文件身份（Modrinth 哈希反查 → CurseForge 指纹），绝不按文件名猜项目
fn match_mod_icons(app: &tauri::AppHandle, items: &[Value]) -> HashMap<String, String> {
    let mut icons: HashMap<String, String> = HashMap::new();
    let hashes: Vec<String> = items.iter().map(|i| i.get("sha1").and_then(|v| v.as_str()).unwrap_or("").to_string()).collect();
    let Ok(versions) = post_json("https://api.modrinth.com/v2/version_files", &json!({ "hashes": hashes, "algorithm": "sha1" }), UA_MGMT, 8_000, &[]) else {
        return cf_icon_fallback(app, items, icons);
    };
    let mut ids: Vec<String> = Vec::new();
    for i in items {
        let sha1 = i.get("sha1").and_then(|v| v.as_str()).unwrap_or("");
        if let Some(pid) = versions.get(sha1).and_then(|v| v.get("project_id")).and_then(|v| v.as_str()) {
            if !ids.contains(&pid.to_string()) {
                ids.push(pid.to_string());
            }
        }
    }
    if !ids.is_empty() {
        let Ok(projects) = get_json(&format!("https://api.modrinth.com/v2/projects?ids={}", community::enc(&serde_json::to_string(&ids).unwrap_or_default())), UA_MGMT, 8_000, &[]) else {
            return cf_icon_fallback(app, items, icons);
        };
        for item in items {
            let sha1 = item.get("sha1").and_then(|v| v.as_str()).unwrap_or("");
            let project_id = versions.get(sha1).and_then(|v| v.get("project_id")).and_then(|v| v.as_str()).unwrap_or("");
            if let Some(project) = projects.as_array().and_then(|a| a.iter().find(|p| p.get("id").and_then(|v| v.as_str()) == Some(project_id))) {
                if let Some(icon) = project.get("icon_url").and_then(|v| v.as_str()) {
                    icons.insert(item.get("fileName").and_then(|v| v.as_str()).unwrap_or("").to_string(), icon.to_string());
                }
            }
        }
    }
    cf_icon_fallback(app, items, icons)
}

fn cf_icon_fallback(app: &tauri::AppHandle, items: &[Value], mut icons: HashMap<String, String>) -> HashMap<String, String> {
    let missing: Vec<&Value> = items.iter().filter(|i| !icons.contains_key(i.get("fileName").and_then(|v| v.as_str()).unwrap_or("")) && i.get("fingerprint").is_some()).collect();
    if missing.is_empty() {
        return icons;
    }
    let ch = community::cf_channel(app);
    let headers: Vec<(&str, &str)> = if ch.official { vec![("x-api-key", &ch.key)] } else { vec![] };
    let fingerprints: Vec<u64> = missing.iter().filter_map(|i| i.get("fingerprint").and_then(|v| v.as_u64())).collect();
    let Ok(matches) = post_json(&format!("{}/fingerprints", ch.base), &json!({ "fingerprints": fingerprints }), UA_MGMT, 8_000, &headers) else {
        return icons;
    };
    let exact: Vec<Value> = matches
        .pointer("/data/exactMatches")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter(|m| {
                    missing.iter().any(|i| {
                        let sha1 = i.get("sha1").and_then(|v| v.as_str()).unwrap_or("");
                        let fp = i.get("fingerprint").and_then(|v| v.as_u64());
                        m.pointer("/file/fileFingerprint").and_then(|v| v.as_u64()) == fp
                            && m.pointer("/file/hashes").and_then(|v| v.as_array()).map(|hashes| {
                                hashes.iter().any(|h| h.get("algo").and_then(|v| v.as_u64()) == Some(1) && h.get("value").and_then(|v| v.as_str()).map(|s| s.to_lowercase() == sha1).unwrap_or(false))
                            }).unwrap_or(false)
                    })
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    let mut ids: Vec<String> = Vec::new();
    for m in &exact {
        if let Some(id) = m.pointer("/file/modId").and_then(|v| v.as_u64()).map(|v| v.to_string()) {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    if ids.is_empty() {
        return icons;
    }
    let Ok(projects) = post_json(&format!("{}/mods", ch.base), &json!({ "modIds": ids }), UA_MGMT, 8_000, &headers) else {
        return icons;
    };
    for item in &missing {
        let sha1 = item.get("sha1").and_then(|v| v.as_str()).unwrap_or("");
        let match_id = exact
            .iter()
            .find(|m| {
                m.pointer("/file/hashes").and_then(|v| v.as_array()).map(|hashes| {
                    hashes.iter().any(|h| h.get("algo").and_then(|v| v.as_u64()) == Some(1) && h.get("value").and_then(|v| v.as_str()).map(|s| s.to_lowercase() == sha1).unwrap_or(false))
                }).unwrap_or(false)
            })
            .and_then(|m| m.pointer("/file/modId").and_then(|v| v.as_u64()).map(|v| v.to_string()).or_else(|| m.get("id").and_then(|v| v.as_str()).map(String::from)));
        let project = projects.pointer("/data").and_then(|v| v.as_array()).and_then(|a| a.iter().find(|p| match_id.as_deref() == p.get("id").and_then(|v| v.as_str())));
        if let Some(icon) = project.and_then(|p| p.get("logo")).and_then(|l| l.get("thumbnailUrl")).and_then(|v| v.as_str()) {
            icons.insert(item.get("fileName").and_then(|v| v.as_str()).unwrap_or("").to_string(), icon.to_string());
        }
    }
    icons
}

/// imageData()：仅限可信 CDN 白名单；≤512KB → dataURL
fn icon_image_data(url: &str) -> Result<String, String> {
    let parsed = parse_url_simple(url)?;
    if parsed.0 != "https" || !["cdn.modrinth.com", "cdn-raw.modrinth.com", "media.forgecdn.net", "mediafilez.forgecdn.net"].contains(&parsed.1.as_str()) {
        return Ok(String::new());
    }
    let resp = net::client().get(url).timeout(Duration::from_secs(8)).header("User-Agent", UA_MGMT).send().map_err(|e| e.to_string())?;
    let ctype = resp.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").split(';').next().unwrap_or("").to_string();
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) || !["image/png", "image/jpeg", "image/webp", "image/gif"].contains(&ctype.as_str()) {
        return Ok(String::new());
    }
    let body = resp.bytes().map_err(|e| e.to_string())?;
    if body.len() > 512 * 1024 {
        return Ok(String::new());
    }
    use base64::Engine;
    Ok(format!("data:{ctype};base64,{}", base64::engine::general_purpose::STANDARD.encode(&body)))
}

fn parse_url_simple(url: &str) -> Result<(String, String), String> {
    let (scheme, rest) = url.split_once("://").ok_or_else(|| "url".to_string())?;
    let host = rest.split('/').next().unwrap_or("");
    Ok((scheme.to_string(), host.to_string()))
}

static ICON_QUEUE: Mutex<()> = Mutex::new(());

/// getModIcons()：缓存目录 userData/cache/mod-icons-v1；远程 7 天 / 内嵌 60 秒
pub fn get_mod_icons(app: &tauri::AppHandle, version: &str, folder: &str, requested: &[String], kind: &str) -> Value {
    let _guard = ICON_QUEUE.lock().unwrap();
    let kind = if kind.is_empty() { "mods" } else { kind };
    let names: Vec<String> = {
        let mut seen = HashSet::new();
        requested
            .iter()
            .filter(|n| {
                installer::basename_of(n.as_str()).as_str() == n.as_str() && !n.contains(['\\', '/', ':', '\0']) && regex::Regex::new(r"(?i)\.(?:jar(?:\.disabled)?|zip)$").unwrap().is_match(n)
            })
            .filter(|n| seen.insert((*n).clone()))
            .take(100)
            .cloned()
            .collect()
    };
    let result = (|| -> Result<Value, String> {
        let dir = resolve_resource_directory(app, folder, version, kind)?;
        let cache_dir = icons_cache_dir(app);
        let mut result: serde_json::Map<String, Value> = serde_json::Map::new();
        let mut todo: Vec<String> = Vec::new();
        let mut keys: HashMap<String, PathBuf> = HashMap::new();
        for name in &names {
            let path = dir.join(name);
            let Ok(stat) = std::fs::symlink_metadata(&path) else { continue };
            if !stat.is_file() {
                continue;
            }
            let mtime = stat.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64).unwrap_or(0);
            let ctime = stat.created().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64).unwrap_or(0);
            let key_src = serde_json::to_string(&json!([dir.to_string_lossy(), name, stat.len(), mtime, ctime])).unwrap_or_default();
            let key = hex::encode(Sha256::digest(key_src.as_bytes()));
            let cache_path = cache_dir.join(format!("{key}.json"));
            keys.insert(name.clone(), cache_path.clone());
            if let Ok(raw) = std::fs::read(&cache_path) {
                if let Ok(cache) = serde_json::from_slice::<Value>(&raw) {
                    if cache.get("expires").and_then(|v| v.as_u64()).unwrap_or(0) > now_ms() {
                        result.insert(name.clone(), cache.get("icon").cloned().unwrap_or(json!("")));
                        continue;
                    }
                }
            }
            todo.push(name.clone());
        }
        if todo.is_empty() {
            return Ok(Value::Object(result));
        }
        let identities: Vec<Value> = scan_mod_directory(&dir.to_string_lossy(), true, Some(&todo), "icons")?
            .into_iter()
            .filter(|i| i.get("sha1").and_then(|v| v.as_str()).map(|s| !s.is_empty()).unwrap_or(false))
            .collect();
        let matches = match_mod_icons(app, &identities);
        for info in &identities {
            let name = info.get("fileName").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let mut icon = String::new();
            if let Some(url) = matches.get(&name) {
                icon = icon_image_data(url).unwrap_or_default();
            }
            let remote = !icon.is_empty();
            if !remote {
                icon = info.get("iconDataUrl").and_then(|v| v.as_str()).unwrap_or("").to_string();
            }
            result.insert(name.clone(), json!(icon));
            if let Some(cache_path) = keys.get(&name) {
                let payload = json!({ "icon": icon, "expires": now_ms() + if remote { 7 * 86_400_000 } else { 60_000 } });
                let _ = std::fs::write(cache_path, payload.to_string());
            }
        }
        Ok(Value::Object(result))
    })();
    result.unwrap_or_else(|_| json!({}))
}

// ---------------- modFavorites.ts + shared/modFavorites.ts ----------------

fn favorites_file(app: &tauri::AppHandle) -> PathBuf {
    crate::settings::user_data_dir(app).join("mod-favorites.json")
}

/// modFavorites()
pub fn mod_favorites(app: &tauri::AppHandle) -> Result<Vec<Value>, String> {
    let raw = match std::fs::read(favorites_file(app)) {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err("收藏记录读取失败，未覆盖原记录".into()),
    };
    let value: Value = serde_json::from_slice(&raw).map_err(|_| "收藏记录读取失败，未覆盖原记录".to_string())?;
    let list = value.as_array().ok_or_else(|| "收藏记录读取失败，未覆盖原记录".to_string())?;
    Ok(list
        .iter()
        .map(|v| {
            let mut item = v.clone();
            if let Ok(key) = favorite_key(v) {
                item["key"] = json!(key);
            }
            item
        })
        .collect())
}

fn write_favorites(app: &tauri::AppHandle, list: Vec<Value>) -> Result<Vec<Value>, String> {
    let path = favorites_file(app);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string(&list).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    Ok(list)
}

/// favoriteKey()
pub fn favorite_key(value: &Value) -> Result<String, String> {
    static ID_RE: OnceLock<regex::Regex> = OnceLock::new();
    static SHA_RE: OnceLock<regex::Regex> = OnceLock::new();
    let source = value.get("source").and_then(|v| v.as_str()).unwrap_or("");
    let project_id = value.get("projectId").and_then(|v| v.as_str()).unwrap_or("");
    if (source == "modrinth" || source == "curseforge")
        && ID_RE.get_or_init(|| regex::Regex::new(r"^[a-zA-Z0-9_-]{1,100}$").unwrap()).is_match(project_id)
    {
        return Ok(format!("{source}:{project_id}"));
    }
    let sha1 = value.get("sha1").and_then(|v| v.as_str()).unwrap_or("");
    if SHA_RE.get_or_init(|| regex::Regex::new(r"^[a-f0-9]{40}$").unwrap()).is_match(sha1) {
        return Ok(format!("sha1:{}", sha1.to_lowercase()));
    }
    Err("收藏缺少可信项目或文件标识".into())
}

/// favoriteIconUrl() 在 community.rs；setFavorite()
pub fn set_favorite(app: &tauri::AppHandle, value: &Value, on: bool) -> Result<Vec<Value>, String> {
    let key = favorite_key(value)?;
    let hash = value.get("sha1").and_then(|v| v.as_str()).map(|s| s.to_lowercase());
    let mut list: Vec<Value> = mod_favorites(app)?
        .into_iter()
        .filter(|f| {
            f.get("key").and_then(|v| v.as_str()) != Some(key.as_str())
                && !(hash.as_ref().map(|h| f.get("key").and_then(|v| v.as_str()) == Some(&format!("sha1:{h}"))).unwrap_or(false))
        })
        .collect();
    if on {
        let name = value
            .get("name")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .or_else(|| value.get("projectId").and_then(|v| v.as_str()))
            .unwrap_or("本地模组")
            .chars()
            .take(200)
            .collect::<String>();
        let mut item = serde_json::Map::new();
        item.insert("key".into(), json!(key));
        item.insert("name".into(), json!(name));
        if let Some(source) = value.get("source") {
            if source.is_string() {
                item.insert("source".into(), source.clone());
            }
        }
        if let Some(pid) = value.get("projectId") {
            if pid.is_string() {
                item.insert("projectId".into(), pid.clone());
            }
        }
        if let Some(h) = &hash {
            item.insert("sha1".into(), json!(h));
        }
        if let Some(icon) = community::favorite_icon_url(value.get("iconUrl")) {
            item.insert("iconUrl".into(), json!(icon));
        }
        item.insert("added".into(), json!(now_ms()));
        list.push(Value::Object(item));
    }
    write_favorites(app, list)
}

/// removeFavoriteRecords()
pub fn remove_favorite_records(list: &[Value], keys: &[String]) -> Result<Vec<Value>, String> {
    if keys.is_empty() || keys.len() > 1000 || keys.iter().any(|k| !list.iter().any(|f| f.get("key").and_then(|v| v.as_str()) == Some(k.as_str()))) {
        return Err("收藏选择已失效，请刷新后重试".into());
    }
    let removed: HashSet<&String> = keys.iter().collect();
    Ok(list.iter().filter(|f| !removed.contains(&f.get("key").and_then(|v| v.as_str()).unwrap_or("").to_string())).cloned().collect())
}

/// linkFavoriteRecords()：合并为来源身份（保留全部文件哈希）
pub fn link_favorite_records(list: &[Value], old_key: &str, project: &Value) -> Result<Vec<Value>, String> {
    let original = list
        .iter()
        .find(|f| f.get("key").and_then(|v| v.as_str()) == Some(old_key))
        .ok_or_else(|| "该收藏已被取消，请刷新列表".to_string())?;
    let key = favorite_key(project)?;
    let existing = list.iter().find(|f| f.get("key").and_then(|v| v.as_str()) == Some(key.as_str()));
    let mut hashes: Vec<String> = Vec::new();
    for src in [Some(original), existing] {
        if let Some(f) = src {
            if let Some(h) = f.get("sha1").and_then(|v| v.as_str()) {
                hashes.push(h.to_string());
            }
            if let Some(list) = f.get("sha1s").and_then(|v| v.as_array()) {
                for h in list {
                    if let Some(s) = h.as_str() {
                        hashes.push(s.to_string());
                    }
                }
            }
        }
    }
    static SHA_RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = SHA_RE.get_or_init(|| regex::Regex::new(r"^[a-f0-9]{40}$").unwrap());
    let mut seen = HashSet::new();
    let hashes: Vec<String> = hashes
        .into_iter()
        .filter(|h| re.is_match(h))
        .map(|h| h.to_lowercase())
        .filter(|h| seen.insert(h.clone()))
        .collect();
    let added = {
        let a = original.get("added").and_then(|v| v.as_u64()).unwrap_or(0);
        let b = existing.and_then(|f| f.get("added")).and_then(|v| v.as_u64()).unwrap_or(a);
        a.min(b)
    };
    let mut merged = serde_json::Map::new();
    merged.insert("key".into(), json!(key));
    merged.insert("source".into(), project.get("source").cloned().unwrap_or(json!("")));
    merged.insert("projectId".into(), project.get("projectId").cloned().unwrap_or(json!("")));
    merged.insert(
        "name".into(),
        json!(project.get("name").and_then(|v| v.as_str()).unwrap_or("").chars().take(200).collect::<String>()),
    );
    merged.insert("added".into(), json!(added));
    if !hashes.is_empty() {
        merged.insert("sha1".into(), json!(hashes[0]));
        merged.insert("sha1s".into(), json!(hashes));
    }
    let icon = project.get("iconUrl").cloned().unwrap_or(Value::Null);
    let icon = if icon.is_null() { existing.and_then(|f| f.get("iconUrl")).cloned().unwrap_or(Value::Null) } else { icon };
    if let Some(icon_url) = community::favorite_icon_url(Some(&icon)) {
        merged.insert("iconUrl".into(), json!(icon_url));
    }
    let mut out: Vec<Value> = list
        .iter()
        .filter(|f| {
            let k = f.get("key").and_then(|v| v.as_str()).unwrap_or("");
            k != old_key && k != key
        })
        .cloned()
        .collect();
    out.push(Value::Object(merged));
    Ok(out)
}

/// validateFavoriteInstallIntent()
fn validate_favorite_install_intent(selections: &[Value], intent: Option<&Value>) -> Result<(usize, usize, Vec<Value>, bool), String> {
    if selections.len() > 100 {
        return Err("收藏模组过多（每次最多 100 项）".into());
    }
    let mut selected: HashSet<String> = HashSet::new();
    for selection in selections {
        if !selection.is_object() {
            return Err("收藏模组选择已失效，请重新检查".into());
        }
        let key = favorite_key(selection).map_err(|_| "收藏模组选择已失效，请重新检查".to_string())?;
        let file_id = selection.get("fileId").and_then(|v| v.as_str()).unwrap_or("");
        if file_id.is_empty() || file_id.len() > 300 || !selected.insert(key) {
            return Err("收藏模组选择重复或已失效，请重新检查".into());
        }
    }
    let Some(intent) = intent else {
        return Ok((selected.len(), selected.len(), Vec::new(), false));
    };
    if intent.get("enabled").and_then(|v| v.as_bool()) != Some(true)
        || intent.get("expected").and_then(|v| v.as_array()).map(|a| a.len() > 1000).unwrap_or(true)
        || intent.get("approvedSkips").and_then(|v| v.as_array()).map(|a| a.len() > 1000).unwrap_or(true)
    {
        return Err("收藏安装意图无效，请重新选择".into());
    }
    let mut expected: HashMap<String, String> = HashMap::new();
    static KEY_RE: OnceLock<regex::Regex> = OnceLock::new();
    let key_re = KEY_RE.get_or_init(|| regex::Regex::new(r"^(?:(?:modrinth|curseforge):[a-zA-Z0-9_-]{1,100}|sha1:[a-f0-9]{40})$").unwrap());
    for item in intent.get("expected").and_then(|v| v.as_array()).unwrap_or(&Vec::new()) {
        let key = item.get("key").and_then(|v| v.as_str()).unwrap_or("");
        let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if !key_re.is_match(key) || name.len() > 200 || expected.contains_key(key) {
            return Err("收藏项目快照无效，请重新选择".into());
        }
        expected.insert(key.to_string(), name.to_string());
    }
    for key in &selected {
        if !expected.contains_key(key) {
            return Err("所选模组不在本次收藏快照中".into());
        }
    }
    let mut skipped: HashMap<String, Value> = HashMap::new();
    let valid_reasons = ["incompatible", "unlinked", "unreliable", "query-error", "deselected", "base-only"];
    for item in intent.get("approvedSkips").and_then(|v| v.as_array()).unwrap_or(&Vec::new()) {
        let key = item.get("key").and_then(|v| v.as_str()).unwrap_or("");
        let reason = item.get("reason").and_then(|v| v.as_str()).unwrap_or("");
        if !expected.contains_key(key) || selected.contains(key) || skipped.contains_key(key) || !valid_reasons.contains(&reason) {
            return Err("收藏跳过决定无效，请重新检查".into());
        }
        if let Some(message) = item.get("message") {
            if !message.is_null() && (message.as_str().is_none() || message.as_str().unwrap_or("").len() > 1000) {
                return Err("收藏跳过决定无效，请重新检查".into());
            }
        }
        if reason == "base-only" && intent.get("baseOnly").and_then(|v| v.as_bool()) != Some(true) {
            return Err("尚未确认不安装收藏模组".into());
        }
        let mut skip = json!({ "key": key, "name": expected.get(key).cloned().unwrap_or_default(), "reason": reason });
        if let Some(message) = item.get("message").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
            skip["message"] = json!(message);
        }
        skipped.insert(key.to_string(), skip);
    }
    let base_only = intent.get("baseOnly").and_then(|v| v.as_bool()).unwrap_or(false);
    if base_only && !selected.is_empty() {
        return Err("仅安装基础实例与收藏模组选择冲突".into());
    }
    if selected.is_empty() && !base_only {
        return Err("没有已确认的收藏模组；请重试、逐项跳过并选择不安装收藏模组，或关闭此选项".into());
    }
    if expected.keys().any(|key| !selected.contains(key) && !skipped.contains_key(key)) {
        return Err("部分收藏模组尚未确认，请重试或明确跳过每一项".into());
    }
    Ok((expected.len(), selected.len(), skipped.into_values().collect(), base_only))
}

// ---------------- 收藏版本查询队列（favoriteVersionQueue.ts，4 并发 + 单飞共享） ----------------

struct FavJob {
    cancel: Arc<AtomicBool>,
    run: Mutex<Option<Box<dyn FnOnce(Arc<AtomicBool>) -> Result<Value, String> + Send>>>,
    result: Mutex<Option<Result<Value, String>>>,
    cv: Condvar,
    clients: Mutex<u32>,
    started: AtomicBool,
}

fn fav_jobs() -> &'static Mutex<(HashMap<String, Arc<FavJob>>, VecDeque<String>, u32)> {
    static J: OnceLock<Mutex<(HashMap<String, Arc<FavJob>>, VecDeque<String>, u32)>> = OnceLock::new();
    J.get_or_init(|| Mutex::new((HashMap::new(), VecDeque::new(), 0)))
}

static FAV_TICKETS: std::sync::LazyLock<Mutex<HashMap<String, Arc<AtomicBool>>>> = std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// 泵：并发上限 4；任务完成时续泵排队任务
fn fav_pump() {
    loop {
        let mut state = fav_jobs().lock().unwrap();
        if state.2 >= 4 {
            return;
        }
        let Some(next_key) = state.1.pop_front() else { return };
        let Some(next) = state.0.get(&next_key).cloned() else { continue };
        if *next.clients.lock().unwrap() == 0 || next.started.load(Ordering::SeqCst) || next.cancel.load(Ordering::SeqCst) {
            continue;
        }
        let Some(run) = next.run.lock().unwrap().take() else { continue };
        next.started.store(true, Ordering::SeqCst);
        state.2 += 1;
        drop(state);
        let key2 = next_key;
        std::thread::spawn(move || {
            let result = run(next.cancel.clone());
            {
                let mut state = fav_jobs().lock().unwrap();
                if state.0.get(&key2).map(|j| Arc::ptr_eq(j, &next)).unwrap_or(false) {
                    state.0.remove(&key2);
                }
                state.2 = state.2.saturating_sub(1);
            }
            *next.result.lock().unwrap() = Some(result);
            next.cv.notify_all();
            fav_pump();
        });
    }
}

fn fav_request(key: String, run: impl FnOnce(Arc<AtomicBool>) -> Result<Value, String> + Send + 'static, cancel: Arc<AtomicBool>) -> Result<Value, String> {
    let job = {
        let mut state = fav_jobs().lock().unwrap();
        if let Some(existing) = state.0.get(&key).cloned() {
            *existing.clients.lock().unwrap() += 1;
            existing
        } else {
            let job = Arc::new(FavJob {
                cancel: Arc::new(AtomicBool::new(false)),
                run: Mutex::new(Some(Box::new(run))),
                result: Mutex::new(None),
                cv: Condvar::new(),
                clients: Mutex::new(1),
                started: AtomicBool::new(false),
            });
            state.0.insert(key.clone(), job.clone());
            state.1.push_back(key.clone());
            job
        }
    };
    fav_pump();
    // 等待结果（可被自身取消；结果克隆共享给所有订阅者）
    let guard = job.result.lock().unwrap();
    let mut guard = guard;
    loop {
        if let Some(result) = guard.as_ref() {
            *job.clients.lock().unwrap() -= 1;
            return result.clone();
        }
        if cancel.load(Ordering::SeqCst) {
            let mut clients = job.clients.lock().unwrap();
            *clients -= 1;
            let last = *clients == 0;
            drop(clients);
            if last {
                job.cancel.store(true, Ordering::SeqCst);
                let mut state = fav_jobs().lock().unwrap();
                if state.0.get(&key).map(|j| Arc::ptr_eq(j, &job)).unwrap_or(false) {
                    state.0.remove(&key);
                }
            }
            return Err("收藏查询已取消".into());
        }
        let (g, _) = job.cv.wait_timeout(guard, Duration::from_millis(200)).unwrap();
        guard = g;
    }
}

/// favoriteVersionResult()：兼容 → 可靠（录像模组校验）→ 不可靠/不兼容 三态
pub fn favorite_version_result(app: &tauri::AppHandle, source: &str, project: &str, mc: &str, loader: &str, cancel: Arc<AtomicBool>) -> Result<Value, String> {
    favorite_key(&json!({ "source": source, "projectId": project }))?;
    if !["fabric", "quilt", "forge", "neoforge"].contains(&loader) || mc.trim().is_empty() || mc.len() > 100 {
        return Err("请选择游戏与加载器".into());
    }
    let mc = mc.trim().to_string();
    let key = serde_json::to_string(&json!([source, project, mc, loader])).unwrap_or_default();
    let app2 = app.clone();
    let source = source.to_string();
    let project = project.to_string();
    let loader = loader.to_string();
    fav_request(
        key,
        move |cancel: Arc<AtomicBool>| -> Result<Value, String> {
            if cancel.load(Ordering::SeqCst) {
                return Err("收藏查询已取消".into());
            }
            let compatible: Vec<Value> = community_favorite_candidates(&app2, &source, &project, &mc, &loader)?
                .into_iter()
                .filter(|f| {
                    f.get("projectId").and_then(|v| v.as_str()) == Some(project.as_str())
                        && f.get("gameVersions").and_then(|v| v.as_array()).map(|a| a.iter().any(|x| x.as_str() == Some(mc.as_str()))).unwrap_or(false)
                        && f.get("loaders").and_then(|v| v.as_array()).map(|a| a.iter().any(|x| x.as_str() == Some(loader.as_str()))).unwrap_or(false)
                })
                .collect();
            let files: Vec<Value> = compatible.iter().filter(|f| compatible_recording_mod(f, &mc, &loader)).cloned().collect();
            let status = if !files.is_empty() {
                "available"
            } else if !compatible.is_empty() {
                "unreliable"
            } else {
                "incompatible"
            };
            Ok(json!({ "files": files, "status": status }))
        },
        cancel,
    )
}

/// favoriteVersions()
pub fn favorite_versions(app: &tauri::AppHandle, source: &str, project: &str, mc: &str, loader: &str, cancel: Arc<AtomicBool>) -> Result<Vec<Value>, String> {
    let result = favorite_version_result(app, source, project, mc, loader, cancel)?;
    Ok(result.get("files").and_then(|v| v.as_array()).cloned().unwrap_or_default())
}

// ---------------- recordingMods.ts（shared/recordingMods.ts） ----------------

pub const RECORDING_PROJECTS: &[(&str, &str)] = &[("replaymod", "Nv2fQJo5"), ("flashback", "4das1Fjq")];

pub fn compatible_recording_mod(file: &Value, mc: &str, loader: &str) -> bool {
    let sha1 = file.get("sha1").and_then(|v| v.as_str()).unwrap_or("");
    let url = file.get("url").and_then(|v| v.as_str()).unwrap_or("");
    let file_name = file.get("fileName").and_then(|v| v.as_str()).unwrap_or("");
    file.get("gameVersions").and_then(|v| v.as_array()).map(|a| a.iter().any(|x| x.as_str() == Some(mc))).unwrap_or(false)
        && file.get("loaders").and_then(|v| v.as_array()).map(|a| a.iter().any(|x| x.as_str() == Some(loader))).unwrap_or(false)
        && !sha1.is_empty()
        && regex::Regex::new(r"^[a-f0-9]{40}$").unwrap().is_match(sha1)
        && url.starts_with("https://")
        && regex::Regex::new(r"^[^\\/:]+\.jar$").unwrap().is_match(file_name)
}

/// recordingModVersions()（recordings:modVersions 由 recordings 模块复用）
pub fn recording_mod_versions(app: &tauri::AppHandle, kind: &str, mc: &str, loader: &str) -> Result<Vec<Value>, String> {
    if !RECORDING_PROJECTS.iter().any(|(k, _)| *k == kind) || !["fabric", "quilt", "forge", "neoforge"].contains(&loader) || mc.is_empty() || mc.len() > 100 {
        return Err("请选择游戏版本、加载器和录像模组".into());
    }
    let project = RECORDING_PROJECTS.iter().find(|(k, _)| *k == kind).unwrap().1;
    let files = community_files(app, "modrinth", project, Some(&community::FileFilter { mc_version: Some(mc.to_string()), loader: Some(loader.to_string()), kind: Some("mod".into()) }))?;
    Ok(files
        .into_iter()
        .filter(|f| f.get("projectId").and_then(|v| v.as_str()) == Some(project) && compatible_recording_mod(f, mc, loader))
        .collect())
}

/// resolveRecordingDependencies()
fn resolve_recording_dependencies(app: &tauri::AppHandle, roots: &[Value], mc: &str, loader: &str) -> Result<Vec<Value>, String> {
    let mut chosen: HashMap<String, Value> = HashMap::new();
    let mut queue: VecDeque<Value> = roots.iter().cloned().collect();
    while let Some(file) = queue.pop_front() {
        if queue.len() > 100 {
            return Err("录像模组必要前置过多".into());
        }
        let project = file.get("projectId").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if project.is_empty() || !compatible_recording_mod(&file, mc, loader) {
            return Err("模组或必要前置没有当前游戏与加载器的兼容文件".into());
        }
        if let Some(existing) = chosen.get(&project) {
            if existing.get("fileId").and_then(|v| v.as_str()) != file.get("fileId").and_then(|v| v.as_str()) {
                return Err(format!("必要前置版本冲突，请调整模组版本：{project}"));
            }
            continue;
        }
        chosen.insert(project.clone(), file.clone());
        for dependency in file.get("dependencies").and_then(|v| v.as_array()).cloned().unwrap_or_default() {
            if !dependency.get("required").and_then(|v| v.as_bool()).unwrap_or(false) {
                continue;
            }
            let dep_project = dependency.get("projectId").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let dep_file = dependency.get("fileId").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let current = chosen.get(&dep_project).cloned().or_else(|| roots.iter().find(|r| r.get("projectId").and_then(|v| v.as_str()) == Some(dep_project.as_str())).cloned());
            if let Some(cur) = current {
                if dep_file.is_empty() || dep_file == cur.get("fileId").and_then(|v| v.as_str()).unwrap_or("") {
                    continue;
                }
            }
            let next = if !dep_file.is_empty() {
                let exact = community_exact_file(app, "modrinth", if dep_project.is_empty() { None } else { Some(&dep_project) }, &dep_file)?;
                if !dep_project.is_empty() && exact.get("projectId").and_then(|v| v.as_str()) != Some(dep_project.as_str()) {
                    return Err("前置项目标识不匹配".into());
                }
                exact
            } else if dep_project.is_empty() {
                return Err("必要前置缺少可信项目标识".into());
            } else {
                community_files(app, "modrinth", &dep_project, Some(&community::FileFilter { mc_version: Some(mc.to_string()), loader: Some(loader.to_string()), kind: Some("mod".into()) }))?
                    .into_iter()
                    .find(|f| compatible_recording_mod(f, mc, loader))
                    .ok_or_else(|| format!("必要前置没有兼容版本：{dep_project}"))?
            };
            queue.push_back(next);
        }
    }
    Ok(chosen.into_values().collect())
}

/// prepareRecordingMod()
pub fn prepare_recording_mod(app: &tauri::AppHandle, mc: &str, opts: &Value) -> Result<Vec<Value>, String> {
    let Some(recording) = opts.get("recordingMod").filter(|v| v.is_object()) else { return Ok(Vec::new()) };
    let loader = opts.get("loader").and_then(|v| v.as_str()).unwrap_or("");
    if loader.is_empty() {
        return Err("录像模组需要模组加载器".into());
    }
    let kind = recording.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    let file_id = recording.get("fileId").and_then(|v| v.as_str()).unwrap_or("");
    let selected = recording_mod_versions(app, kind, mc, loader)?
        .into_iter()
        .find(|f| f.get("fileId").and_then(|v| v.as_str()) == Some(file_id))
        .ok_or_else(|| "所选录像模组版本已不可用或不兼容，请重新选择".to_string())?;
    let mut roots = vec![selected];
    if loader == "fabric" {
        if let Some(fabric_api) = opts.get("fabricApi").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
            let api = community_files(app, "modrinth", "P7dR8mSH", Some(&community::FileFilter { mc_version: Some(mc.to_string()), loader: Some(loader.to_string()), kind: Some("mod".into()) }))?
                .into_iter()
                .find(|f| f.get("version").and_then(|v| v.as_str()) == Some(fabric_api))
                .ok_or_else(|| "所选 Fabric API 已不可用，请重新选择".to_string())?;
            roots.push(api);
        }
    }
    resolve_recording_dependencies(app, &roots, mc, loader)
}

/// installRecordingMods()
pub fn install_recording_mods(
    app: &tauri::AppHandle,
    dir: &Path,
    files: &[Value],
    rec: &Arc<TaskRecord>,
    progress: Option<&(dyn Fn(f64) + Sync)>,
) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    if std::fs::symlink_metadata(dir).map(|m| m.file_type().is_symlink()).unwrap_or(false) {
        return Err("模组目录不能是链接".into());
    }
    download::with_file_job(dir, Some(rec.as_ref()), || -> Result<(), String> {
        let mut needed: Vec<Value> = Vec::new();
        for f in files {
            let name = f.get("fileName").and_then(|v| v.as_str()).unwrap_or("");
            let dest = dir.join(name);
            if dest.exists() {
                if mod_hash(&dest)? != f.get("sha1").and_then(|v| v.as_str()).unwrap_or("") {
                    return Err(format!("同名模组已存在且内容不同，未覆盖：{name}"));
                }
            } else {
                needed.push(f.clone());
            }
        }
        if !needed.is_empty() {
            assert_instance_idle(app, dir.parent().unwrap_or(dir))?;
            let items: Vec<ModReplacement> = needed
                .iter()
                .map(|f| ModReplacement {
                    old_name: None,
                    old_sha1: None,
                    name: f.get("fileName").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    sha1: f.get("sha1").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    url: f.get("url").and_then(|v| v.as_str()).map(str::to_string),
                    size: f.get("size").and_then(|v| v.as_u64()),
                })
                .collect();
            replace_mod_files(app, rec, dir, &items, None, progress)?;
        }
        for f in files {
            if let (Some(source), Some(project), Some(sha1)) = (
                f.get("source").and_then(|v| v.as_str()),
                f.get("projectId").and_then(|v| v.as_str()),
                f.get("sha1").and_then(|v| v.as_str()),
            ) {
                remember_mod_identity(app, dir, sha1, &format!("{source}:{project}"));
            }
        }
        Ok(())
    })
}

// ---------------- prepareInstallMods / favoriteInstallResult（versions:install 收藏模组链路） ----------------

/// prepareInstallMods()
pub fn prepare_install_mods(app: &tauri::AppHandle, mc: &str, opts: &Value, rec: &Arc<TaskRecord>) -> Result<Vec<Value>, String> {
    check_cancel(rec)?;
    let selections: Vec<Value> = opts.get("favoriteMods").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let (_, _, _, _) = validate_favorite_install_intent(&selections, opts.get("favoriteInstallIntent"))?;
    let mut list = selections.clone();
    if list.len() > 100 {
        return Err("收藏模组过多".into());
    }
    if let Some(recording) = opts.get("recordingMod").filter(|v| v.is_object()) {
        let kind = recording.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        let project = RECORDING_PROJECTS.iter().find(|(k, _)| *k == kind).map(|(_, p)| p.to_string());
        if let Some(project) = project {
            list.push(json!({ "source": "modrinth", "projectId": project, "fileId": recording.get("fileId").cloned().unwrap_or(json!("")) }));
        }
    }
    if list.is_empty() {
        return Ok(Vec::new());
    }
    let loader = opts.get("loader").and_then(|v| v.as_str()).unwrap_or("");
    if loader.is_empty() {
        return Err("收藏模组需要模组加载器".into());
    }
    let mut roots: Vec<Value> = Vec::new();
    for selected in &list {
        let files = favorite_versions(
            app,
            selected.get("source").and_then(|v| v.as_str()).unwrap_or(""),
            selected.get("projectId").and_then(|v| v.as_str()).unwrap_or(""),
            mc,
            loader,
            rec.cancelled.clone(),
        )?;
        let file_id = selected.get("fileId").and_then(|v| v.as_str()).unwrap_or("");
        let exact = files.into_iter().find(|f| f.get("fileId").and_then(|v| v.as_str()) == Some(file_id)).ok_or_else(|| format!("所选模组版本已不可用或不兼容：{}", selected.get("projectId").and_then(|v| v.as_str()).unwrap_or("")))?;
        roots.push(exact);
    }
    if loader == "fabric" {
        if let Some(fabric_api) = opts.get("fabricApi").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
            let api = favorite_versions(app, "modrinth", "P7dR8mSH", mc, loader, rec.cancelled.clone())?
                .into_iter()
                .find(|f| f.get("version").and_then(|v| v.as_str()) == Some(fabric_api))
                .ok_or_else(|| "Fabric API 已不可用".to_string())?;
            roots.push(api);
        }
    }
    let target = json!({ "mcVersion": mc, "loader": loader });
    let graph = dependency_graph(app, &roots, &target, &mut HashMap::new())?;
    for f in &graph {
        if !compatible_recording_mod(f, mc, loader) {
            return Err(format!("模组缺少可校验下载文件：{}", f.get("fileName").and_then(|v| v.as_str()).unwrap_or("")));
        }
    }
    let mut names: HashMap<String, String> = HashMap::new();
    for f in &graph {
        let name = f.get("fileName").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let sha1 = f.get("sha1").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if let Some(old) = names.get(&name) {
            if old != &sha1 {
                return Err(format!("模组文件名冲突：{name}"));
            }
        }
        names.insert(name, sha1);
    }
    check_cancel(rec)?;
    Ok(graph)
}

/// favoriteInstallResult()：完成数以落盘校验回读为准，绝不用预览查询数
pub fn favorite_install_result(
    app: &tauri::AppHandle,
    opts: &Value,
    files: &[Value],
    mods_directory: &str,
    folder: &str,
    instance_id: &str,
    rec: &Arc<TaskRecord>,
) -> Result<Option<Value>, String> {
    let selections: Vec<Value> = opts.get("favoriteMods").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let intent = opts.get("favoriteInstallIntent").cloned().unwrap_or(Value::Null);
    if intent.is_null() && selections.is_empty() {
        return Ok(None);
    }
    let (requested, selected, skipped, base_only) = validate_favorite_install_intent(&selections, Some(&intent))?;
    let mut verified: Vec<Value> = Vec::new();
    let mut unique: Vec<Value> = Vec::new();
    {
        let mut map: HashMap<String, Value> = HashMap::new();
        let mut order: Vec<String> = Vec::new();
        for f in files {
            let key = f.get("fileName").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if map.insert(key.clone(), f.clone()).is_none() {
                order.push(key);
            }
        }
        for k in order {
            unique.push(map.remove(&k).unwrap());
        }
    }
    for file in &unique {
        check_cancel(rec)?;
        let name = file.get("fileName").and_then(|v| v.as_str()).unwrap_or("");
        let target = Path::new(mods_directory).join(name);
        let info = std::fs::symlink_metadata(&target).map_err(|e| e.to_string())?;
        if !info.is_file() || info.file_type().is_symlink() {
            return Err(format!("收藏模组落盘校验失败：{name}"));
        }
        let sha1 = installer::sha1_file_hex(&target)?;
        if file.get("sha1").and_then(|v| v.as_str()).map(|s| s.to_lowercase()) != Some(sha1.clone()) {
            return Err(format!("收藏模组落盘哈希不符：{name}"));
        }
        verified.push(json!({
            "fileName": name,
            "sha1": sha1,
            "source": file.get("source").cloned().unwrap_or(Value::Null),
            "projectId": file.get("projectId").cloned().unwrap_or(Value::Null),
        }));
    }
    for selected_item in &selections {
        let s = selected_item.get("source").and_then(|v| v.as_str()).unwrap_or("");
        let p = selected_item.get("projectId").and_then(|v| v.as_str()).unwrap_or("");
        let fid = selected_item.get("fileId").and_then(|v| v.as_str()).unwrap_or("");
        if !files.iter().any(|f| {
            f.get("source").and_then(|v| v.as_str()) == Some(s)
                && f.get("projectId").and_then(|v| v.as_str()) == Some(p)
                && f.get("fileId").and_then(|v| v.as_str()) == Some(fid)
        }) {
            return Err(format!("收藏模组未出现在实际安装结果中：{p}"));
        }
    }
    let selected_keys: HashSet<String> = selections.iter().map(|f| format!("{}:{}", f.get("source").and_then(|v| v.as_str()).unwrap_or(""), f.get("fileId").and_then(|v| v.as_str()).unwrap_or(""))).collect();
    let mut favorite_graph: HashSet<String> = HashSet::new();
    fn visit(file: &Value, files: &[Value], favorite_graph: &mut HashSet<String>) {
        let key = format!("{}:{}", file.get("source").and_then(|v| v.as_str()).unwrap_or(""), file.get("fileId").and_then(|v| v.as_str()).unwrap_or(""));
        if !favorite_graph.insert(key) {
            return;
        }
        for dep in file.get("dependencies").and_then(|v| v.as_array()).cloned().unwrap_or_default() {
            if !dep.get("required").and_then(|v| v.as_bool()).unwrap_or(false) {
                continue;
            }
            let dep_file = dep.get("fileId").and_then(|v| v.as_str()).unwrap_or("");
            let dep_project = dep.get("projectId").and_then(|v| v.as_str()).unwrap_or("");
            let next = files.iter().find(|candidate| {
                candidate.get("source").and_then(|v| v.as_str()) == file.get("source").and_then(|v| v.as_str())
                    && (!dep_file.is_empty() && candidate.get("fileId").and_then(|v| v.as_str()) == Some(dep_file)
                        || candidate.get("projectId").and_then(|v| v.as_str()) == Some(dep_project))
            });
            if let Some(next) = next {
                visit(next, files, favorite_graph);
            }
        }
    }
    for root in files {
        let key = format!("{}:{}", root.get("source").and_then(|v| v.as_str()).unwrap_or(""), root.get("fileId").and_then(|v| v.as_str()).unwrap_or(""));
        if selected_keys.contains(&key) {
            visit(root, files, &mut favorite_graph);
        }
    }
    let dependencies = favorite_graph.iter().filter(|key| !selected_keys.contains(*key)).count();
    check_cancel(rec)?;
    let folder_real = std::fs::canonicalize(folder).unwrap_or_else(|_| PathBuf::from(folder));
    let mods_real = if Path::new(mods_directory).exists() {
        std::fs::canonicalize(mods_directory).unwrap_or_else(|_| PathBuf::from(mods_directory))
    } else {
        std::path::absolute(mods_directory).unwrap_or_else(|_| PathBuf::from(mods_directory))
    };
    Ok(Some(json!({
        "requested": requested,
        "selected": selected,
        "installed": selected,
        "dependencies": dependencies,
        "skipped": skipped,
        "baseOnly": base_only,
        "folder": folder_real.to_string_lossy(),
        "instanceId": instance_id,
        "modsDirectory": mods_real.to_string_lossy(),
        "verifiedFiles": verified,
    })))
}

// ---------------- supplementalMods.ts ----------------

fn supplemental_file(app: &tauri::AppHandle) -> PathBuf {
    crate::settings::user_data_dir(app).join("supplemental-mod-retries.json")
}

fn supplemental_rows(app: &tauri::AppHandle) -> Result<Vec<Value>, String> {
    let raw = match std::fs::read(supplemental_file(app)) {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err("附加模组重试记录无法读取，已保留原文件".into()),
    };
    let value: Value = serde_json::from_slice(&raw).map_err(|_| "附加模组重试记录无法读取，已保留原文件".to_string())?;
    let list = value.as_array().ok_or_else(|| "附加模组重试记录无法读取，已保留原文件".to_string())?;
    if list.len() > 100 {
        return Err("附加模组重试记录无法读取，已保留原文件".into());
    }
    Ok(list
        .iter()
        .filter(|v| {
            v.get("id").and_then(|x| x.as_str()).is_some()
                && v.pointer("/target/folder").and_then(|x| x.as_str()).is_some()
                && v.pointer("/target/id").and_then(|x| x.as_str()).is_some()
                && v.get("versionId").and_then(|x| x.as_str()).is_some()
                && v.get("options").map(|o| o.is_object()).unwrap_or(false)
        })
        .cloned()
        .collect())
}

fn supplemental_visible(rows: Vec<Value>) -> Vec<Value> {
    rows.into_iter()
        .map(|mut v| {
            if let Some(obj) = v.as_object_mut() {
                obj.remove("options");
            }
            v
        })
        .collect()
}

fn supplemental_save(app: &tauri::AppHandle, list: Vec<Value>) -> Result<Vec<Value>, String> {
    let path = supplemental_file(app);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension(format!("{}.tmp", crate::accounts::uuid_v4()));
    std::fs::write(&tmp, serde_json::to_string(&list).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    let visible = supplemental_visible(list);
    installer::emit_event(app, "mods:supplementalPending", &json!(visible));
    Ok(visible)
}

fn supplemental_running() -> &'static Mutex<HashSet<String>> {
    static R: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashSet::new()))
}

/// recordSupplementalFailure()（versions:install 收藏模组失败记录）
pub fn record_supplemental_failure(app: &tauri::AppHandle, target: &Value, version_id: &str, options: &Value, error: &str) -> Result<Vec<Value>, String> {
    let mut rows = supplemental_rows(app)?;
    rows.retain(|r| {
        r.pointer("/target/id").and_then(|v| v.as_str()) != target.get("id").and_then(|v| v.as_str())
            || r.pointer("/target/folder").and_then(|v| v.as_str()) != target.get("folder").and_then(|v| v.as_str())
    });
    rows.push(json!({
        "id": crate::accounts::uuid_v4(),
        "target": target,
        "versionId": version_id,
        "options": options,
        "message": error,
    }));
    supplemental_save(app, rows)
}

// ---------------- modMigration.ts ----------------

fn migration_plans() -> &'static Mutex<HashMap<String, Value>> {
    static P: OnceLock<Mutex<HashMap<String, Value>>> = OnceLock::new();
    P.get_or_init(|| Mutex::new(HashMap::new()))
}

fn migration_running() -> &'static Mutex<HashSet<String>> {
    static R: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashSet::new()))
}

/// compatibleFile()
fn proj_id_str(v: &Value) -> String {
    match v.as_str() {
        Some(s) => s.to_string(),
        None => v.as_i64().map(|n| n.to_string()).unwrap_or_default(),
    }
}

pub fn compatible_migration_file(file: &Value, mc: &str, loader: &str) -> bool {
    let url = file.get("url").and_then(|v| v.as_str()).unwrap_or("");
    let sha1 = file.get("sha1").and_then(|v| v.as_str()).unwrap_or("");
    let file_name = file.get("fileName").and_then(|v| v.as_str()).unwrap_or("");
    !url.is_empty() && url.starts_with("https://")
        && regex::Regex::new(r"^[a-f0-9]{40}$").unwrap().is_match(sha1)
        && file.get("gameVersions").and_then(|v| v.as_array()).map(|a| a.iter().any(|x| x.as_str() == Some(mc))).unwrap_or(false)
        && file.get("loaders").and_then(|v| v.as_array()).map(|a| a.iter().any(|x| x.as_str() == Some(loader))).unwrap_or(false)
        && regex::Regex::new(r"(?i)\.jar$").unwrap().is_match(file_name)
        && installer::basename_of(file_name) == file_name
        && !file_name.contains(['\\', '/', ':', '\0'])
}

/// planModMigration()
pub fn plan_mod_migration(app: &tauri::AppHandle, source_id: &str, folder: &str, mc_version: &str, loader: &str) -> Result<Value, String> {
    require_resource_version(source_id)?;
    let mc_ok = regex::Regex::new(r"^[a-zA-Z0-9._-]{1,40}$").unwrap().is_match(mc_version);
    if !mc_ok || !["fabric", "forge", "neoforge", "quilt"].contains(&loader) {
        return Err("请选择有效的游戏版本与加载器".into());
    }
    let dir = resolve_resource_directory(app, folder, source_id, "mods")?;
    let names: Vec<String> = match std::fs::read_dir(&dir) {
        Ok(rd) => rd
            .flatten()
            .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| regex::Regex::new(r"(?i)\.jar(?:\.disabled)?$").unwrap().is_match(n))
            .collect(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(e.to_string()),
    };
    if names.len() > 500 {
        return Err("单次最多迁移500个模组，请先整理模组列表".into());
    }
    if names.is_empty() {
        return Err("当前版本没有可迁移的模组".into());
    }
    let scanned = scan_mod_directory(&dir.to_string_lossy(), true, Some(&names), "analysis")?;
    let loaders = installer::list_loader_versions(loader, mc_version)?;
    if loaders.first().map(|s| s.is_empty()).unwrap_or(true) {
        return Err(format!("{loader} 没有适配 {mc_version} 的加载器"));
    }
    let mut entries: Vec<Value> = Vec::new();
    for i in &scanned {
        let sha1 = i.get("sha1").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let file_name = i.get("fileName").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let sha1_value = if sha1.is_empty() {
            json!(file_hash(&dir.join(&file_name))?)
        } else {
            json!(sha1)
        };
        let disabled = regex::Regex::new(r"(?i)\.disabled$").unwrap().is_match(&file_name);
        let reason = if i.get("error").is_some() { "文件无法解析，保留为禁用文件" } else { "未匹配社区来源" };
        entries.push(json!({
            "fileName": file_name,
            "name": i.get("name").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(Value::from).unwrap_or_else(|| i.get("fileName").cloned().unwrap_or(json!(""))),
            "currentVersion": i.get("version").and_then(|v| v.as_str()).unwrap_or(""),
            "sha1": sha1_value,
            "disabled": disabled,
            "status": "unavailable",
            "reason": reason,
        }));
    }
    let hashes: Vec<String> = entries.iter().map(|e| e.get("sha1").and_then(|v| v.as_str()).unwrap_or("").to_string()).collect();
    let originals = mr_post(app, "/version_files", &json!({ "hashes": hashes, "algorithm": "sha1" }), UA_MGMT, 20_000);
    let updates = mr_post(app, "/version_files/update", &json!({ "hashes": hashes, "algorithm": "sha1", "game_versions": [mc_version], "loaders": [loader] }), UA_MGMT, 20_000);
    let mr_error = match (&originals, &updates) {
        (Ok(_), Ok(_)) => String::new(),
        (Err(e), _) => e.clone(),
        (_, Err(e)) => e.clone(),
    };
    let originals = originals.unwrap_or(json!({}));
    let updates = updates.unwrap_or(json!({}));
    let missing: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter(|(_, e)| originals.get(e.get("sha1").and_then(|v| v.as_str()).unwrap_or("")).is_none())
        .map(|(i, _)| i)
        .collect();
    let mut cf_ids: HashMap<String, String> = HashMap::new();
    if !missing.is_empty() {
        let ch = community::cf_channel(app);
        let fingerprints: Vec<u64> = scanned
            .iter()
            .filter(|i| missing.iter().any(|idx| entries[*idx].get("fileName").and_then(|v| v.as_str()) == i.get("fileName").and_then(|v| v.as_str())))
            .filter_map(|i| i.get("fingerprint").and_then(|v| v.as_u64()))
            .collect();
        let headers: Vec<(&str, &str)> = if ch.official { vec![("x-api-key", &ch.key)] } else { vec![] };
        match post_json(&format!("{}/fingerprints", ch.base), &json!({ "fingerprints": fingerprints }), UA_MGMT, 20_000, &headers) {
            Ok(result) => {
                for idx in &missing {
                    let sha1 = entries[*idx].get("sha1").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let matched = result
                        .pointer("/data/exactMatches")
                        .and_then(|v| v.as_array())
                        .and_then(|list| {
                            list.iter().find(|m| {
                                m.pointer("/file/hashes").and_then(|v| v.as_array()).map(|hashes| {
                                    hashes.iter().any(|h| h.get("algo").and_then(|v| v.as_u64()) == Some(1) && h.get("value").and_then(|v| v.as_str()).map(|s| s.to_lowercase() == sha1).unwrap_or(false))
                                }).unwrap_or(false)
                            })
                        });
                    if let Some(m) = matched {
                        let id = m.pointer("/file/modId").and_then(|v| v.as_u64()).map(|v| v.to_string()).unwrap_or_else(|| m.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string());
                        cf_ids.insert(sha1, id);
                    }
                }
            }
            Err(e) => return Err(format!("无法完成社区来源识别，请检查网络后重试。{}{}", mr_error, e)),
        }
    }
    if !mr_error.is_empty() {
        return Err(format!("Modrinth 查询失败，请重试：{mr_error}"));
    }
    // 目标版本匹配（4 路并发 → 顺序等价）
    let mut versions_map: HashMap<String, Value> = HashMap::new();
    for idx in 0..entries.len() {
        let sha1 = entries[idx].get("sha1").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let project = originals.get(&sha1).and_then(|v| v.get("project_id")).and_then(|v| v.as_str()).map(str::to_string);
        if let Some(project) = project {
            entries[idx]["reason"] = json!("该项目没有适配目标版本与加载器的文件");
            if let Some(v) = updates.get(&sha1) {
                let files = v.get("files").and_then(|f| f.as_array()).cloned().unwrap_or_default();
                let f = files.iter().find(|x| x.get("primary").and_then(|p| p.as_bool()).unwrap_or(false)).or_else(|| files.first());
                if let Some(f) = f {
                    let file = json!({
                        "source": "modrinth",
                        "projectId": v.get("project_id").cloned().unwrap_or(json!("")),
                        "fileId": v.get("id").cloned().unwrap_or(json!("")),
                        "fileName": f.get("filename").cloned().unwrap_or(json!("")),
                        "url": f.get("url").cloned().unwrap_or(json!("")),
                        "sha1": f.pointer("/hashes/sha1").cloned().unwrap_or(Value::Null),
                        "size": f.get("size").cloned().unwrap_or(json!(0)),
                        "releaseType": v.get("version_type").cloned().unwrap_or(json!("release")),
                        "version": v.get("version_number").cloned().unwrap_or(json!("")),
                        "date": v.get("date_published").cloned().unwrap_or(json!("")),
                        "gameVersions": v.get("game_versions").cloned().unwrap_or(json!([])),
                        "loaders": v.get("loaders").cloned().unwrap_or(json!([])),
                        "dependencies": v.get("dependencies").and_then(|d| d.as_array()).map(|list| list.iter().map(|d| json!({
                            "required": d.get("dependency_type").and_then(|x| x.as_str()) == Some("required"),
                            "projectId": d.get("project_id").cloned().unwrap_or(Value::Null),
                            "fileId": d.get("version_id").cloned().unwrap_or(Value::Null),
                        })).collect::<Vec<Value>>()).unwrap_or_default(),
                    });
                    if compatible_migration_file(&file, mc_version, loader) {
                        entries[idx]["target"] = file.clone();
                        entries[idx]["status"] = json!("compatible");
                        let key = format!("{}:{}", file.get("source").and_then(|x| x.as_str()).unwrap_or(""), file.get("projectId").map(proj_id_str).unwrap_or_default());
                        versions_map.insert(key, file);
                    }
                }
            }
        } else if let Some(cf_id) = cf_ids.get(&sha1).cloned() {
            entries[idx]["reason"] = json!("该项目没有适配目标版本与加载器的可下载文件");
            let list = community_files(app, "curseforge", &cf_id, Some(&community::FileFilter { mc_version: Some(mc_version.to_string()), loader: Some(loader.to_string()), kind: Some("mod".into()) }))?;
            if let Some(file) = list.into_iter().find(|f| compatible_migration_file(f, mc_version, loader)) {
                entries[idx]["target"] = file.clone();
                entries[idx]["status"] = json!("compatible");
                let key = format!("{}:{}", file.get("source").and_then(|x| x.as_str()).unwrap_or(""), file.get("projectId").map(proj_id_str).unwrap_or_default());
                versions_map.insert(key, file);
            }
        }
    }
    // 锁定警告 + 必要前置补齐
    let mut warnings: Vec<String> = Vec::new();
    for e in &entries {
        let sha1 = e.get("sha1").and_then(|v| v.as_str()).unwrap_or("");
        if !sha1.is_empty() && is_mod_locked(app, &dir, sha1) {
            warnings.push(format!("锁定模组 {} 将按目标游戏重新匹配；确认后目标文件仍锁定", e.get("fileName").and_then(|v| v.as_str()).unwrap_or("")));
        }
    }
    let mut visited: HashSet<String> = HashSet::new();
    let mut n = 0usize;
    while n < entries.len() {
        if entries.len() > 500 {
            return Err("前置依赖超过500项，请检查模组依赖关系".into());
        }
        let entry = entries[n].clone();
        n += 1;
        let has_target = entry.get("target").map(|t| t.is_object()).unwrap_or(false);
        if !has_target || entry.get("disabled").and_then(|v| v.as_bool()).unwrap_or(false) {
            continue;
        }
        let deps = entry.pointer("/target/dependencies").and_then(|v| v.as_array()).cloned().unwrap_or_default();
        for dep in deps {
            if !dep.get("required").and_then(|v| v.as_bool()).unwrap_or(false) {
                continue;
            }
            let source = entry.pointer("/target/source").and_then(|v| v.as_str()).unwrap_or("modrinth").to_string();
            let dep_project = dep.get("projectId").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let dep_file = dep.get("fileId").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let key = format!("{}:{}:{}", source, dep_project, dep_file);
            if !visited.insert(key) {
                continue;
            }
            let existing = versions_map.get(&format!("{}:{}", source, dep_project)).cloned();
            if let Some(existing) = existing {
                let all_disabled = entries.iter().filter(|x| {
                    x.pointer("/target/source").and_then(|v| v.as_str()) == Some(source.as_str())
                        && x.pointer("/target/projectId").and_then(|v| v.as_str()) == Some(dep_project.as_str())
                }).all(|x| x.get("disabled").and_then(|v| v.as_bool()).unwrap_or(false));
                if all_disabled {
                    entries.push(json!({
                        "fileName": "",
                        "name": existing.get("fileName").cloned().unwrap_or(json!("")),
                        "currentVersion": "",
                        "sha1": "",
                        "disabled": false,
                        "status": "dependency",
                        "target": existing.clone(),
                    }));
                }
                if !dep_file.is_empty() && existing.get("fileId").and_then(|v| v.as_str()) != Some(dep_file.as_str()) {
                    warnings.push(format!(
                        "{} 需要特定前置版本 {}，当前目标版本为 {}，请确认兼容性",
                        entry.get("fileName").and_then(|v| v.as_str()).unwrap_or(""),
                        dep_file,
                        existing.get("fileName").and_then(|v| v.as_str()).unwrap_or("")
                    ));
                }
                continue;
            }
            let outcome: Result<Value, String> = (|| {
                let file = if !dep_file.is_empty() {
                    community_exact_file(app, &source, if dep_project.is_empty() { None } else { Some(&dep_project) }, &dep_file)?
                } else {
                    community_files(app, &source, &dep_project, Some(&community::FileFilter { mc_version: Some(mc_version.to_string()), loader: Some(loader.to_string()), kind: Some("mod".into()) }))?
                        .into_iter()
                        .find(|f| compatible_migration_file(f, mc_version, loader))
                        .ok_or_else(|| "无兼容可下载版本".to_string())?
                };
                if !compatible_migration_file(&file, mc_version, loader) {
                    return Err("无兼容可下载版本".into());
                }
                Ok(file)
            })();
            match outcome {
                Ok(file) => {
                    versions_map.insert(format!("{}:{}", source, file.get("projectId").and_then(|v| v.as_str()).unwrap_or("")), file.clone());
                    entries.push(json!({
                        "fileName": "",
                        "name": file.get("fileName").cloned().unwrap_or(json!("")),
                        "currentVersion": "",
                        "sha1": "",
                        "disabled": false,
                        "status": "dependency",
                        "target": file,
                    }));
                }
                Err(e) => warnings.push(format!(
                    "{} 的必要前置 {} 未能匹配：Error: {}",
                    entry.get("fileName").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).unwrap_or_else(|| entry.get("name").and_then(|v| v.as_str()).unwrap_or("")),
                    if dep_project.is_empty() { dep_file.as_str() } else { dep_project.as_str() },
                    e
                )),
            }
        }
    }
    let plan_id = crate::accounts::uuid_v4();
    let plan = json!({
        "id": plan_id,
        "sourceId": source_id,
        "folder": folder,
        "mcVersion": mc_version,
        "loader": loader,
        "loaderVersion": loaders[0],
        "entries": entries,
        "warnings": warnings,
        "createdAt": now_ms(),
    });
    {
        let mut store = migration_plans().lock().unwrap();
        store.retain(|_, p| now_ms() - p.get("createdAt").and_then(|v| v.as_u64()).unwrap_or(0) <= 1_800_000);
        store.insert(plan_id, plan.clone());
    }
    Ok(plan)
}

/// validateMigrationSource()
fn validate_migration_source(plan: &Value, dir: &Path) -> Result<(), String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| regex::Regex::new(r"(?i)\.jar(?:\.disabled)?$").unwrap().is_match(n))
        .collect();
    names.sort();
    let mut expected: Vec<String> = plan
        .get("entries")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter(|e| e.get("fileName").and_then(|v| v.as_str()).map(|s| !s.is_empty()).unwrap_or(false)).map(|e| e.get("fileName").and_then(|v| v.as_str()).unwrap_or("").to_string()).collect())
        .unwrap_or_default();
    expected.sort();
    if names != expected {
        return Err("检查后模组列表发生变化，请重新检查".into());
    }
    for e in plan.get("entries").and_then(|v| v.as_array()).unwrap_or(&Vec::new()) {
        if let Some(name) = e.get("fileName").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
            let stat = std::fs::symlink_metadata(dir.join(name)).map_err(|e| e.to_string())?;
            if !stat.is_file() || stat.file_type().is_symlink() || installer::sha1_file_hex(&dir.join(name))? != e.get("sha1").and_then(|v| v.as_str()).unwrap_or("") {
                return Err(format!("检查后模组文件已变化，请重新检查：{name}"));
            }
        }
    }
    Ok(())
}

/// migrationInstanceId()
pub fn migration_instance_id(source: &str, mc: &str, id: &str) -> String {
    let budget = 50usize.saturating_sub(mc.len());
    let mut prefix: String = source.chars().take(budget).collect();
    // 去掉被截断的半截代理对
    if prefix.chars().next_back().is_some_and(|c| (char::from_u32(0xD800).unwrap()..=char::from_u32(0xDBFF).unwrap()).contains(&c)) {
        prefix.pop();
    }
    format!("{prefix} - {mc} - {}", &id[..id.len().min(8)])
}

/// 目标实例安装接线点（modMigration → versions.installVersion 完整流程）
pub type MigrationInstallHook = Arc<dyn Fn(&tauri::AppHandle, &Arc<TaskRecord>, &(dyn Fn(&Value) + Send + Sync), &str, &str, &str, &str, &str) -> Result<(), String> + Send + Sync>;
static MIGRATION_HOOK: Mutex<Option<MigrationInstallHook>> = Mutex::new(None);

pub fn set_migration_install_hook(hook: MigrationInstallHook) {
    *MIGRATION_HOOK.lock().unwrap() = Some(hook);
}

#[allow(clippy::too_many_arguments)]
fn run_migration_install(
    app: &tauri::AppHandle,
    rec: &Arc<TaskRecord>,
    emit: &(dyn Fn(&Value) + Send + Sync),
    folder: &str,
    mc_version: &str,
    loader: &str,
    loader_version: &str,
    instance_name: &str,
) -> Result<(), String> {
    let hook = MIGRATION_HOOK.lock().unwrap().clone();
    match hook {
        Some(hook) => hook(app, rec, emit, folder, mc_version, loader, loader_version, instance_name),
        None => Err("版本安装模块尚未接入迁移流程，请更新启动器后重试".into()),
    }
}

/// applyModMigration()
pub fn apply_mod_migration(
    app: &tauri::AppHandle,
    rec: &Arc<TaskRecord>,
    id: &str,
    confirm_missing: bool,
    emit: &(dyn Fn(&Value) + Send + Sync),
) -> Result<Value, String> {
    let plan = {
        let store = migration_plans().lock().unwrap();
        store.get(id).cloned().filter(|p| now_ms() - p.get("createdAt").and_then(|v| v.as_u64()).unwrap_or(0) <= 1_800_000)
    };
    let Some(plan) = plan else { return Err("迁移预览已过期，请重新检查".into()) };
    let has_unavailable = plan
        .get("entries")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().any(|e| e.get("status").and_then(|x| x.as_str()) == Some("unavailable")))
        .unwrap_or(false);
    let warnings = plan.get("warnings").and_then(|v| v.as_array()).map(|a| !a.is_empty()).unwrap_or(false);
    if (has_unavailable || warnings) && !confirm_missing {
        return Err("请先确认未匹配模组与依赖提醒".into());
    }
    if !migration_running().lock().unwrap().insert(id.to_string()) {
        return Err("该迁移正在进行".into());
    }
    let folder = plan.get("folder").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let source_id = plan.get("sourceId").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let mc_version = plan.get("mcVersion").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let loader = plan.get("loader").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let loader_version = plan.get("loaderVersion").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let entries = plan.get("entries").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let mut stage: Option<PathBuf> = None;
    let result = (|| -> Result<Value, String> {
        let dir = resolve_resource_directory(app, &folder, &source_id, "mods")?;
        emit(&json!({ "stage": "download", "progress": 0, "overall": 0, "text": "正在校验迁移源文件" }));
        validate_migration_source(&plan, &dir)?;
        if !confirm_missing {
            for e in &entries {
                if let (Some(_name), Some(sha1)) = (e.get("fileName").and_then(|v| v.as_str()).filter(|s| !s.is_empty()), e.get("sha1").and_then(|v| v.as_str())) {
                    if is_mod_locked(app, &dir, sha1) {
                        return Err("模组已锁定，请重新检查并确认迁移".into());
                    }
                }
            }
        }
        check_cancel(rec)?;
        let stage_dir = {
            let mut created: Option<PathBuf> = None;
            for _ in 0..100 {
                let candidate = Path::new(&folder).join(format!(".kamucl-migration-{}", crate::accounts::uuid_v4()));
                if std::fs::create_dir(&candidate).is_ok() {
                    created = Some(candidate);
                    break;
                }
            }
            created.ok_or_else(|| "迁移临时目录创建失败".to_string())?
        };
        stage = Some(stage_dir.clone());
        let mut used: HashMap<String, String> = HashMap::new();
        for (i, e) in entries.iter().enumerate() {
            check_cancel(rec)?;
            let file_name = e.get("fileName").and_then(|v| v.as_str()).unwrap_or("");
            let target_name = e.pointer("/target/fileName").and_then(|v| v.as_str()).unwrap_or("");
            let base = if target_name.is_empty() {
                regex::Regex::new(r"(?i)\.disabled$").unwrap().replace(file_name, "").to_string()
            } else {
                target_name.to_string()
            };
            let disabled = e.get("disabled").and_then(|v| v.as_bool()).unwrap_or(false);
            let no_target = !e.get("target").map(|t| t.is_object()).unwrap_or(false);
            let name = if disabled || no_target { format!("{base}.disabled") } else { base };
            let digest = e.pointer("/target/sha1").and_then(|v| v.as_str()).unwrap_or_else(|| e.get("sha1").and_then(|v| v.as_str()).unwrap_or("")).to_string();
            if let Some(prev) = used.get(&name.to_lowercase()) {
                if prev != &digest {
                    return Err(format!("迁移文件名冲突：{name}"));
                }
                continue;
            }
            used.insert(name.to_lowercase(), digest);
            let dest = stage_dir.join(&name);
            if e.get("target").map(|t| t.is_object()).unwrap_or(false) {
                let total = e.pointer("/target/size").and_then(|v| v.as_u64());
                let mirror = download::mirror_from_settings(app);
                let bytes = std::sync::atomic::AtomicU64::new(0);
                let display = e.get("fileName").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).unwrap_or_else(|| e.get("name").and_then(|v| v.as_str()).unwrap_or("")).to_string();
                let entries_len = entries.len();
                let progress = move |done: u64, total_cb: u64, wire: u64| {
                    bytes.fetch_add(wire, Ordering::SeqCst);
                    let frac = if total_cb > 0 { done as f64 / total_cb as f64 } else { 0.0 };
                    let overall = 0.65 * ((i as f64 + frac) / entries_len as f64);
                    let mut ev = json!({
                        "stage": "download",
                        "progress": frac,
                        "overall": overall,
                        "text": format!("迁移模组 {display}"),
                    });
                    if total_cb > 0 {
                        ev["bytesDone"] = json!(done);
                        ev["bytesTotal"] = json!(total_cb);
                    }
                    emit(&ev);
                };
                let opts = DownloadFileOpts {
                    sha1: e.pointer("/target/sha1").and_then(|v| v.as_str()),
                    sha512: None,
                    sha256: None,
                    size: total,
                    alternatives: &[],
                    reuse_dirs: &[],
                    reuse_files: &[],
                    mirror,
                    max_attempts: 4,
                    source_pool: None,
                    progress: Some(&progress),
                    on_wait: None,
                };
                download::download_file(app, rec, e.pointer("/target/url").and_then(|v| v.as_str()).unwrap_or(""), &dest, &opts)?;
            } else {
                std::fs::copy(dir.join(file_name), &dest).map_err(|e| e.to_string())?;
            }
        }
        validate_migration_source(&plan, &dir)?;
        check_cancel(rec)?;
        let plan_uuid = plan.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let target_id = migration_instance_id(&source_id, &mc_version, plan_uuid);
        require_resource_version(&target_id)?;
        if installer::version_dir_in(&folder, &target_id).exists() {
            return Err("目标迁移实例已存在，请重新检查生成新实例".into());
        }
        // 目标实例安装（0.65–0.95 泳道）
        {
            let install_emit = move |event: &Value| {
                let mut mapped = event.clone();
                if let Some(obj) = mapped.as_object_mut() {
                    let base = obj.get("overall").and_then(|v| v.as_f64()).or_else(|| obj.get("progress").and_then(|v| v.as_f64())).unwrap_or(0.0);
                    obj.insert("overall".into(), json!(0.65 + 0.3 * base));
                }
                if let Some(obj) = mapped.as_object_mut() {
                    let text = obj.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    obj.insert("text".into(), json!(format!("准备目标游戏：{text}")));
                }
                emit(&mapped);
            };
            run_migration_install(app, rec, &install_emit, &folder, &mc_version, &loader, &loader_version, &target_id)?;
            installer::set_new_instance_isolation(&folder, &target_id, true)?;
            let marker = installer::install_mark_path_in(&folder, &target_id);
            std::fs::write(&marker, "版本迁移尚未完成").map_err(|e| e.to_string())?;
            check_cancel(rec)?;
            let mods_dir = resolve_resource_directory(app, &folder, &target_id, "mods")?;
            if mods_dir.exists() {
                return Err("目标模组目录已存在，未覆盖，请重新迁移".into());
            }
            for e in &entries {
                if let Some(sha1) = e.get("sha1").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
                    if e.get("fileName").and_then(|v| v.as_str()).map(|s| !s.is_empty()).unwrap_or(false) && is_mod_locked(app, &dir, sha1) {
                        if !confirm_missing {
                            return Err("模组已锁定，请重新确认后迁移".into());
                        }
                        let target_sha = e.pointer("/target/sha1").and_then(|v| v.as_str()).unwrap_or(sha1).to_string();
                        set_mod_locked(app, &mods_dir, &target_sha, true);
                    }
                }
            }
            std::fs::rename(&stage_dir, &mods_dir).map_err(|e| e.to_string())?;
            stage = None;
            let report_path = installer::version_dir_in(&folder, &target_id).join("migration-report.json");
            let _ = std::fs::write(&report_path, serde_json::to_string_pretty(&plan).unwrap_or_default());
            let _ = std::fs::remove_file(&marker);
        }
        migration_plans().lock().unwrap().remove(id);
        emit(&json!({ "stage": "done", "progress": 1, "overall": 1, "text": format!("版本迁移完成：{target_id}") }));
        Ok(json!({ "versionId": target_id, "folder": folder }))
    })();
    migration_running().lock().unwrap().remove(id);
    if let Some(stage_dir) = stage {
        let root = std::path::absolute(&folder).unwrap_or_else(|_| PathBuf::from(&folder));
        let resolved = std::path::absolute(&stage_dir).unwrap_or(stage_dir);
        let ok = resolved.starts_with(&root) && resolved.file_name().map(|n| n.to_string_lossy().starts_with(".kamucl-migration-")).unwrap_or(false);
        if !ok {
            return Err("迁移临时目录校验失败".into());
        }
        let _ = std::fs::remove_dir_all(&resolved);
    }
    result
}

// ---------------- IPC 入口 ----------------

fn with_game_folder_of(app: &tauri::AppHandle, version_id: &str, folder: Option<&str>) -> String {
    folder.filter(|f| !f.is_empty()).map(str::to_string).unwrap_or_else(|| installer::folder_of_version(app, version_id))
}

pub fn invoke(app: &tauri::AppHandle, channel: &str, args: &[Value]) -> Option<Result<Value, String>> {
    match channel {
        "mods:parse" => Some(
            arg(args, 0)
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect::<Vec<String>>())
                .unwrap_or_default(),
        )
        .map(|paths| {
            let (files, skipped) = expand_jar_paths(&paths);
            let mut list: Vec<Value> = files.iter().map(|f| parse_mod_file(f)).collect();
            for s in skipped {
                list.push(json!({
                    "filePath": s,
                    "fileName": s,
                    "id": "",
                    "name": "",
                    "version": "",
                    "loader": Value::Null,
                    "mcRange": "",
                    "dependencies": [],
                    "error": "不支持的文件类型（仅支持 .jar 或包含 .jar 的文件夹）",
                }));
            }
            Ok(json!(list))
        }),
        "mods:duplicates" => Some((|| -> Result<Value, String> {
            let version_id = arg_str(args, 0).unwrap_or("").to_string();
            let folder = with_game_folder_of(app, &version_id, arg_str(args, 1));
            let dir = resolve_resource_directory(app, &folder, &version_id, "mods")?;
            let infos = scan_mod_directory(&dir.to_string_lossy(), false, None, "analysis")?;
            let mut groups: HashMap<String, Value> = HashMap::new();
            let mut order: Vec<String> = Vec::new();
            for info in infos {
                if info.get("error").is_some() {
                    continue;
                }
                let id = info.get("id").and_then(|v| v.as_str()).unwrap_or("");
                if id.is_empty() {
                    continue;
                }
                let key = id.to_lowercase();
                let entry = groups.entry(key.clone()).or_insert_with(|| json!({ "modId": id, "name": info.get("name").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).unwrap_or(id), "files": [] }));
                if !order.contains(&key) {
                    order.push(key);
                }
                entry["files"].as_array_mut().unwrap().push(json!({ "fileName": info.get("fileName").cloned().unwrap_or(json!("")), "version": info.get("version").cloned().unwrap_or(json!("")), "latest": false }));
            }
            let mut out: Vec<Value> = Vec::new();
            for key in order {
                let g = groups.get(&key).cloned().unwrap();
                let files = g.get("files").and_then(|v| v.as_array()).cloned().unwrap_or_default();
                if files.len() < 2 {
                    continue;
                }
                let mut sorted = files;
                sorted.sort_by(|a, b| {
                    let av = a.get("version").and_then(|v| v.as_str()).unwrap_or("");
                    let bv = b.get("version").and_then(|v| v.as_str()).unwrap_or("");
                    compare_mod_version(bv, av)
                });
                let mut g = g;
                let arr: Vec<Value> = sorted.into_iter().enumerate().map(|(i, mut f)| {
                    f["latest"] = json!(i == 0);
                    f
                }).collect();
                g["files"] = json!(arr);
                out.push(g);
            }
            out.sort_by(|a, b| {
                let an = a.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let bn = b.get("name").and_then(|v| v.as_str()).unwrap_or("");
                an.cmp(bn)
            });
            Ok(json!(out))
        })()),
        "mods:crossDuplicates" => Some((|| -> Result<Value, String> {
            let version_ids: Vec<String> = arg(args, 0)
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();
            let mut map: HashMap<String, Value> = HashMap::new();
            let mut order: Vec<String> = Vec::new();
            for vid in &version_ids {
                let dir = mods_dir_of(app, vid)?;
                let infos = scan_mod_directory(&dir.to_string_lossy(), false, None, "analysis")?;
                for info in infos {
                    if info.get("error").is_some() {
                        continue;
                    }
                    let id = info.get("id").and_then(|v| v.as_str()).unwrap_or("");
                    if id.is_empty() {
                        continue;
                    }
                    let key = id.to_lowercase();
                    let entry = map.entry(key.clone()).or_insert_with(|| {
                        order.push(key.clone());
                        json!({ "modId": id, "name": info.get("name").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).unwrap_or(id), "presentIn": [] })
                    });
                    let present_in = entry["presentIn"].as_array_mut().unwrap();
                    if !present_in.iter().any(|p| p.get("versionId").and_then(|v| v.as_str()) == Some(vid.as_str())) {
                        present_in.push(json!({ "versionId": vid, "fileName": info.get("fileName").cloned().unwrap_or(json!("")) }));
                    }
                }
            }
            let mut out: Vec<Value> = order.iter().map(|k| map[k].clone()).collect();
            out.retain(|g| g.get("presentIn").and_then(|v| v.as_array()).map(|a| a.len() >= 2).unwrap_or(false));
            out.sort_by_key(|g| std::cmp::Reverse(g.get("presentIn").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0)));
            Ok(json!(out))
        })()),
        "mods:icons" => Some({
            let version_id = arg_str(args, 0).unwrap_or("").to_string();
            let folder = with_game_folder_of(app, &version_id, arg_str(args, 2));
            let names: Vec<String> = arg(args, 1)
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();
            let kind = arg_str(args, 3).unwrap_or("mods").to_string();
            Ok::<Value, String>(get_mod_icons(app, &version_id, &folder, &names, &kind))
        }.map_err(|e| e)),
        "mods:migrationPlan" => Some(installer::run_blocking(|| {
            let source_id = arg_str(args, 0).unwrap_or("").to_string();
            let folder = arg_str(args, 1).unwrap_or("").to_string();
            let mc = arg_str(args, 2).unwrap_or("").to_string();
            let loader = arg_str(args, 3).unwrap_or("").to_string();
            plan_mod_migration(app, &source_id, &folder, &mc, &loader)
        })),
        "mods:migrationApply" => Some((|| -> Result<Value, String> {
            let plan_id = arg_str(args, 0).unwrap_or("").to_string();
            let confirmed = arg(args, 1).and_then(|v| v.as_bool()).unwrap_or(false);
            let rec = tasks::register_task("版本迁移", "version");
            let task_id = rec.id.clone();
            let returned_id = task_id.clone();
            let app2 = app.clone();
            std::thread::spawn(move || {
                let emit = TaskEmit::new(&app2, &task_id, "版本迁移", None);
                let emit_ref = &emit;
                let result = apply_mod_migration(&app2, &rec, &plan_id, confirmed, &|event: &Value| emit_ref.emit(event));
                match result {
                    Ok(r) => {
                        let version_id = r.get("versionId").cloned().unwrap_or(json!(""));
                        tasks::task_done(&app2, &task_id, true, None, false, None);
                        installer::emit_event(&app2, "event:installDone", &json!({
                            "taskId": task_id,
                            "versionId": version_id,
                            "installedId": version_id,
                            "ok": true,
                        }));
                    }
                    Err(e) => {
                        let cancelled = is_cancel(&e);
                        tasks::task_done(&app2, &task_id, false, Some(e.clone()), cancelled, Some(emit.last_stage()));
                    }
                }
            });
            Ok(json!(returned_id))
        })()),
        "mods:checkUpdates" => Some(installer::run_blocking(|| {
            let version_id = arg_str(args, 0).unwrap_or("").to_string();
            let folder = with_game_folder_of(app, &version_id, arg_str(args, 1));
            let dir = resolve_resource_directory(app, &folder, &version_id, "mods")?;
            check_mod_updates(app, &dir, &folder, &version_id)
        })),
        "mods:applyUpdates" => Some(installer::run_blocking(|| {
            let version_id = arg_str(args, 0).unwrap_or("").to_string();
            let folder = with_game_folder_of(app, &version_id, arg_str(args, 2));
            let dir = resolve_resource_directory(app, &folder, &version_id, "mods")?;
            let items: Vec<Value> = arg(args, 1).and_then(|v| v.as_array()).cloned().unwrap_or_default();
            apply_mod_updates(app, &dir, &items)
        })),
        "mods:catalog" => Some((|| -> Result<Value, String> {
            let id = arg_str(args, 0).unwrap_or("").to_string();
            let folder = arg_str(args, 1).unwrap_or("").to_string();
            let (_dir, list) = mod_catalog(app, &id, &folder)?;
            Ok(json!(list))
        })()),
        "mods:setEnabled" => Some(installer::run_blocking(|| {
            let id = arg_str(args, 0).unwrap_or("").to_string();
            let folder = arg_str(args, 1).unwrap_or("").to_string();
            let names: Vec<String> = arg(args, 2).and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
            let enabled = arg(args, 3).and_then(|v| v.as_bool()).unwrap_or(false);
            set_mods_enabled(app, &id, &folder, &names, enabled)
        })),
        "mods:setLocked" => Some(installer::run_blocking(|| {
            let id = arg_str(args, 0).unwrap_or("").to_string();
            let folder = arg_str(args, 1).unwrap_or("").to_string();
            let names: Vec<String> = arg(args, 2).and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
            let locked = arg(args, 3).and_then(|v| v.as_bool()).unwrap_or(false);
            lock_mods(app, &id, &folder, &names, locked)
        })),
        "mods:versionChoices" => Some(installer::run_blocking(|| {
            let id = arg_str(args, 0).unwrap_or("").to_string();
            let folder = arg_str(args, 1).unwrap_or("").to_string();
            let name = arg_str(args, 2).unwrap_or("").to_string();
            mod_version_choices(app, &id, &folder, &name)
        })),
        "mods:versionPlan" => Some(installer::run_blocking(|| {
            let id = arg_str(args, 0).unwrap_or("").to_string();
            let file_id = arg_str(args, 1).unwrap_or("").to_string();
            plan_mod_version_change(app, &id, &file_id)
        })),
        "mods:versionApply" => Some(installer::run_blocking(|| {
            let id = arg_str(args, 0).unwrap_or("").to_string();
            let confirmed = arg(args, 1).and_then(|v| v.as_bool()).unwrap_or(false);
            apply_mod_version_change(app, &id, confirmed)
        })),
        "mods:versionDiscard" => Some({
            if let Some(id) = arg_str(args, 0) {
                discard_mod_version_plan(id);
            }
            Ok::<Value, String>(Value::Null)
        }),
        "mods:targets" => Some(Ok(scan_mod_targets(app))),
        "mods:prepare" => Some((|| -> Result<Value, String> {
            let target_ref = arg(args, 0).cloned().unwrap_or(json!({}));
            let input = arg(args, 1).cloned().unwrap_or(json!({}));
            let operation_id = arg(args, 2)
                .and_then(|v| v.as_str())
                .filter(|s| regex::Regex::new(r"^[a-zA-Z0-9:_-]{1,96}$").unwrap().is_match(s))
                .map(str::to_string);
            let targets = scan_mod_targets(app);
            let versions = targets.get("versions").and_then(|v| v.as_array()).cloned().unwrap_or_default();
            let target = select_mod_target(&versions, target_ref.get("id").and_then(|v| v.as_str()).unwrap_or(""), target_ref.get("folder").and_then(|v| v.as_str()).unwrap_or(""))?;
            let title = format!("准备 MOD {}（尚未安装）", input.pointer("/file/fileName").and_then(|v| v.as_str()).unwrap_or("本地文件"));
            let rec = tasks::register_task(&title, "download");
            let task_id = rec.id.clone();
            let task_title = rec.title.clone();
            let app2 = app.clone();
            let (tx, rx) = std::sync::mpsc::channel::<Result<Value, String>>();
            std::thread::spawn(move || {
                let emit = TaskEmit::new(&app2, &task_id, &task_title, operation_id);
                emit.emit(&json!({ "stage": "mod-prepare", "progress": 0, "indeterminate": true, "text": "读取 MOD 来源及兼容性，准备下载（尚未安装）…" }));
                let result = prepare_mod_install(&app2, &rec, &target, &input, &emit);
                match result {
                    Ok(plan) => {
                        tasks::task_done(&app2, &task_id, true, None, false, None);
                        let _ = tx.send(Ok(plan));
                    }
                    Err(e) => {
                        let cancelled = is_cancel(&e);
                        if !cancelled {
                            emit.emit(&json!({ "stage": "error", "progress": 0, "text": format!("MOD 准备失败：{e}") }));
                        }
                        tasks::task_done(&app2, &task_id, false, Some(e.clone()), cancelled, Some(emit.last_stage()));
                        let _ = tx.send(Err(e));
                    }
                }
            });
            rx.recv().map_err(|_| "后台任务异常退出".to_string())?
        })()),
        "mods:discard" => Some({
            if let Some(id) = arg_str(args, 0) {
                discard_mod_plan(id);
            }
            Ok::<Value, String>(Value::Null)
        }),
        "mods:commit" => Some((|| -> Result<Value, String> {
            let id = arg_str(args, 0).unwrap_or("").to_string();
            let include_dependencies = arg(args, 1).and_then(|v| v.as_bool()).unwrap_or(false);
            let operation_id = arg(args, 2)
                .and_then(|v| v.as_str())
                .filter(|s| regex::Regex::new(r"^[a-zA-Z0-9:_-]{1,96}$").unwrap().is_match(s))
                .map(str::to_string);
            let rec = tasks::register_task("安装 MOD 与前置依赖", "download");
            let task_id = rec.id.clone();
            let task_title = rec.title.clone();
            let app2 = app.clone();
            let (tx, rx) = std::sync::mpsc::channel::<Result<String, String>>();
            std::thread::spawn(move || {
                let emit = TaskEmit::new(&app2, &task_id, &task_title, operation_id);
                emit.emit(&json!({ "stage": "mod-verify", "progress": 0, "indeterminate": true, "text": "准备下载前置及校验 MOD…" }));
                let revalidate = |target: &Value| -> Result<Value, String> {
                    let target_id = target.get("id").and_then(|v| v.as_str()).unwrap_or("");
                    if running_version_ids().contains(target_id) {
                        return Err("目标实例正在运行，未安装任何 MOD；请退出该游戏后重试".into());
                    }
                    let targets = scan_mod_targets(&app2);
                    let versions = targets.get("versions").and_then(|v| v.as_array()).cloned().unwrap_or_default();
                    select_mod_target(&versions, target_id, target.get("folder").and_then(|v| v.as_str()).unwrap_or(""))
                };
                let result = execute_mod_plan(&app2, &rec, &id, include_dependencies, &revalidate, &emit);
                match result {
                    Ok(message) => {
                        tasks::task_done(&app2, &task_id, true, None, false, None);
                        let _ = tx.send(Ok(message));
                    }
                    Err(e) => {
                        let cancelled = is_cancel(&e);
                        if !cancelled {
                            emit.emit(&json!({ "stage": "error", "progress": 0, "text": format!("MOD 安装失败：{e}") }));
                        }
                        tasks::task_done(&app2, &task_id, false, Some(e.clone()), cancelled, Some(emit.last_stage()));
                        let _ = tx.send(Err(e));
                    }
                }
            });
            rx.recv().map_err(|_| "后台任务异常退出".to_string())?
                .map(|msg| json!(msg))
        })()),
        "mods:install" => Some((|| -> Result<Value, String> {
            let files: Vec<String> = arg(args, 0)
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();
            let target_version_id = arg(args, 1).map(|v| v.as_str().map(String::from).unwrap_or_else(|| v.to_string())).unwrap_or_default();
            let folder = arg(args, 2).and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(String::from).unwrap_or_else(|| installer::folder_of_version(app, &target_version_id));
            let targets = scan_mod_targets(app);
            let versions = targets.get("versions").and_then(|v| v.as_array()).cloned().unwrap_or_default();
            let target = select_mod_target(&versions, &target_version_id, &folder)?;
            Ok(json!(copy_compatible_mods(&files, &target)))
        })()),
        "mods:favorites" => Some(mod_favorites(app).map(|list| json!(list))),
        "mods:favorite" => Some((|| -> Result<Value, String> {
            let value = arg(args, 0).cloned().unwrap_or(json!({}));
            let on = arg(args, 1).and_then(|v| v.as_bool()).unwrap_or(false);
            set_favorite(app, &value, on).map(|list| json!(list))
        })()),
        "mods:favoriteRemove" => Some((|| -> Result<Value, String> {
            let keys: Vec<String> = arg(args, 0)
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();
            let list = mod_favorites(app)?;
            let remaining = remove_favorite_records(&list, &keys)?;
            write_favorites(app, remaining).map(|l| json!(l))
        })()),
        "mods:favoriteLink" => Some(installer::run_blocking(|| {
            let key = arg_str(args, 0).unwrap_or("").to_string();
            let source = arg_str(args, 1).unwrap_or("").to_string();
            let project_id = arg(args, 2).map(|v| v.as_str().map(String::from).unwrap_or_else(|| v.to_string())).unwrap_or_default();
            if !mod_favorites(app)?.iter().any(|f| f.get("key").and_then(|v| v.as_str()) == Some(key.as_str())) {
                return Err("该收藏已被取消，请刷新列表".into());
            }
            let project = community::community_mod_project(app, &source, &project_id)?;
            let list = mod_favorites(app)?;
            let merged = link_favorite_records(&list, &key, &project)?;
            write_favorites(app, merged).map(|l| json!(l))
        })),
        "mods:favoriteVersions" => Some((|| -> Result<Value, String> {
            let source = arg_str(args, 0).unwrap_or("").to_string();
            let project = arg(args, 1).map(|v| v.as_str().map(String::from).unwrap_or_else(|| v.to_string())).unwrap_or_default();
            let mc = arg_str(args, 2).unwrap_or("").to_string();
            let loader = arg_str(args, 3).unwrap_or("").to_string();
            let ticket = arg(args, 4).and_then(|v| v.as_str()).map(str::to_string);
            match ticket {
                None => favorite_version_result(app, &source, &project, &mc, &loader, Arc::new(AtomicBool::new(false))),
                Some(ticket) => {
                    if !regex::Regex::new(r"^[a-zA-Z0-9_-]{1,100}$").unwrap().is_match(&ticket) {
                        return Err("收藏查询订阅无效".into());
                    }
                    let cancel = Arc::new(AtomicBool::new(false));
                    {
                        let mut map = FAV_TICKETS.lock().unwrap();
                        if map.contains_key(&ticket) {
                            return Err("收藏查询订阅重复".into());
                        }
                        map.insert(ticket.clone(), cancel.clone());
                    }
                    let result = favorite_version_result(app, &source, &project, &mc, &loader, cancel.clone());
                    FAV_TICKETS.lock().unwrap().remove(&ticket);
                    result
                }
            }
        })()),
        "mods:favoriteVersionsCancel" => Some({
            if let Some(ticket) = arg_str(args, 0) {
                if let Some(cancel) = FAV_TICKETS.lock().unwrap().get(ticket) {
                    cancel.store(true, Ordering::SeqCst);
                }
            }
            Ok::<Value, String>(Value::Null)
        }),
        "mods:favoriteLocal" => Some(installer::run_blocking(|| {
            let version = arg_str(args, 0).unwrap_or("").to_string();
            let folder = arg_str(args, 1).unwrap_or("").to_string();
            let name = arg_str(args, 2).unwrap_or("").to_string();
            let on = arg(args, 3).and_then(|v| v.as_bool()).unwrap_or(false);
            let link = arg(args, 4).filter(|v| v.is_object());
            let (dir, mods) = mod_catalog(app, &version, &folder)?;
            let mod_item = mods
                .iter()
                .find(|m| m.get("fileName").and_then(|v| v.as_str()) == Some(name.as_str()))
                .cloned()
                .ok_or_else(|| "无法校验该模组".to_string())?;
            let sha1 = mod_item.get("sha1").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if sha1.is_empty() {
                return Err("无法校验该模组".into());
            }
            validate_mod_file(&dir, &name, Some(&sha1))?;
            if let Some(link) = link {
                let key = favorite_key(link)?;
                if key.starts_with("sha1:") {
                    return Err("请选择来源平台和项目".into());
                }
                let source = link.get("source").and_then(|v| v.as_str()).unwrap_or("");
                let project = link.get("projectId").and_then(|v| v.as_str()).unwrap_or("");
                community_files(app, source, project, Some(&community::FileFilter { kind: Some("mod".into()), ..Default::default() }))?;
                remember_mod_identity(app, &dir, &sha1, &key);
            } else {
                let _ = identify(app, &dir, std::slice::from_ref(&mod_item));
            }
            let identity = mod_identity(app, &dir, &sha1);
            let (source, project_id) = identity.split_once(':').unwrap_or(("sha1", ""));
            let mut value = json!({ "name": mod_item.get("name").cloned().unwrap_or(json!("")), "sha1": sha1 });
            if source != "sha1" {
                value["source"] = json!(source);
                value["projectId"] = json!(project_id);
            }
            let unknown = format!("sha1:{sha1}");
            if link.is_some() {
                let list = mod_favorites(app)?;
                if list.iter().any(|f| f.get("key").and_then(|v| v.as_str()) == Some(unknown.as_str())) {
                    write_favorites(app, list.into_iter().filter(|f| f.get("key").and_then(|v| v.as_str()) != Some(unknown.as_str())).collect())?;
                }
            }
            set_favorite(app, &value, on).map(|list| json!(list))
        })),
        "mods:supplementalList" => Some(supplemental_rows(app).map(|rows| json!(supplemental_visible(rows)))),
        "mods:supplementalKeep" => Some((|| -> Result<Value, String> {
            let id = arg_str(args, 0).unwrap_or("").to_string();
            if supplemental_running().lock().unwrap().contains(&id) {
                return Err("正在重试，请等待任务结束".into());
            }
            let rows = supplemental_rows(app)?;
            let remaining: Vec<Value> = rows.into_iter().filter(|r| r.get("id").and_then(|v| v.as_str()) != Some(id.as_str())).collect();
            supplemental_save(app, remaining).map(Value::Array)
        })()),
        "mods:supplementalRetry" => Some((|| -> Result<Value, String> {
            let id = arg_str(args, 0).unwrap_or("").to_string();
            let with_result = arg(args, 1).and_then(|v| v.as_bool()).unwrap_or(false);
            let entry = supplemental_rows(app)?
                .into_iter()
                .find(|r| r.get("id").and_then(|v| v.as_str()) == Some(id.as_str()))
                .ok_or_else(|| "重试记录已过期".to_string())?;
            if !supplemental_running().lock().unwrap().insert(id.clone()) {
                return Err("该模组任务正在重试".into());
            }
            let rec = tasks::register_task(&format!("重试附加模组 · {}", entry.pointer("/target/id").and_then(|v| v.as_str()).unwrap_or("")), "download");
            let task_id = rec.id.clone();
            let task_title = rec.title.clone();
            let app2 = app.clone();
            let (tx, rx) = std::sync::mpsc::channel::<Result<Value, String>>();
            std::thread::spawn(move || {
                let outcome = (|| -> Result<Value, String> {
                    let target_folder = entry.pointer("/target/folder").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let target_id = entry.pointer("/target/id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let version_id = entry.get("versionId").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let options = entry.get("options").cloned().unwrap_or(json!({}));
                    let version_json = installer::read_version_json(&app2, &target_folder, &target_id)?;
                    let dir = installer::instance_directory_state(&app2, &target_folder, &target_id, &version_json)
                        .get("path")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    assert_instance_idle(&app2, Path::new(&dir))?;
                    let meta = installer::resolve_instance_metadata(&version_json, &|pid: &str| installer::read_version_json(&app2, &installer::folder_of_version(&app2, pid), pid).ok());
                    let broken = meta.mc_version.is_empty();
                    let loader_opt = entry.pointer("/options/loader").and_then(|v| v.as_str()).map(str::to_string);
                    if broken || meta.mc_version != version_id || meta.loader.map(str::to_string) != loader_opt {
                        return Err("实例版本或加载器已变化，请重新选择兼容模组".into());
                    }
                    let files = prepare_install_mods(&app2, &version_id, &options, &rec)?;
                    let mods_dir = Path::new(&dir).join("mods");
                    {
                        let emit_ref = |fraction: f64| {
                            installer::emit_event(&app2, "event:progress", &json!({
                                "taskId": task_id,
                                "taskTitle": task_title,
                                "stage": "download",
                                "progress": fraction,
                                "text": "下载并校验所选模组与必要前置",
                            }));
                        };
                        install_recording_mods(&app2, &mods_dir, &files, &rec, Some(&emit_ref))?;
                    }
                    let result = favorite_install_result(&app2, &options, &files, &mods_dir.to_string_lossy(), &target_folder, &target_id, &rec)?;
                    let rows = supplemental_rows(&app2)?;
                    let remaining: Vec<Value> = rows.into_iter().filter(|r| r.get("id").and_then(|v| v.as_str()) != Some(id.as_str())).collect();
                    let pending = supplemental_save(&app2, remaining)?;
                    Ok(if with_result {
                        json!({ "pending": pending, "result": result })
                    } else {
                        json!(pending)
                    })
                })();
                let ok = outcome.is_ok();
                if let Err(e) = &outcome {
                    if let Ok(rows) = supplemental_rows(&app2) {
                        let updated: Vec<Value> = rows
                            .into_iter()
                            .map(|mut r| {
                                if r.get("id").and_then(|v| v.as_str()) == Some(id.as_str()) {
                                    r["message"] = json!(e);
                                }
                                r
                            })
                            .collect();
                        let _ = supplemental_save(&app2, updated);
                    }
                }
                installer::emit_event(&app2, "event:taskDone", &json!({
                    "taskId": task_id,
                    "ok": ok,
                    "cancelled": rec.cancelled.load(Ordering::SeqCst),
                }));
                tasks::finish_task(&task_id);
                supplemental_running().lock().unwrap().remove(&id);
                let _ = tx.send(outcome);
            });
            rx.recv().map_err(|_| "后台任务异常退出".to_string())?
        })()),
        _ => None,
    }
}

/// compareModVersion()：MOD 版本号比较（去 +/_ 后缀，按 .- 分段数值比较）
fn compare_mod_version(a: &str, b: &str) -> std::cmp::Ordering {
    let norm = |s: &str| -> Vec<i64> {
        let stripped = regex::Regex::new(r"[+_].*$").unwrap().replace(s, "").to_string();
        stripped
            .split(['.', '-'])
            .map(|x| x.trim().parse::<i64>().unwrap_or(0))
            .collect()
    };
    let pa = norm(a);
    let pb = norm(b);
    for i in 0..pa.len().max(pb.len()) {
        let d = pa.get(i).copied().unwrap_or(0) - pb.get(i).copied().unwrap_or(0);
        if d != 0 {
            return d.cmp(&0);
        }
    }
    std::cmp::Ordering::Equal
}
