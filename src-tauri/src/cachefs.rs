//! 统一的启动器缓存层（对应 PCL 的 `Cache\` 目录 + 体积上限）。
//!
//! 设计口径（参照 PCL2 的实现）：
//! 1. **一处登记**：所有可重建的缓存都落在 `{数据目录}/cache/<name>/`，由本模块统一
//!    记录根名与上限；新增缓存必须在这里登记，否则不受体积控制。
//! 2. **必有上限**：PCL 会整目录删掉下载缓存，并对列表类缓存做失效；我们选择
//!    「按目录记录最近使用时间、超过上限淘汰最旧一半」，避免用户磁盘被缓存吃掉。
//! 3. **用 mtime+size 做失效指纹**：PCL 的 ModHash.ini 就是这么做的
//!    （键 = `文件全名-mtime-size`），改文件即失效，不必重算哈希。
//!
//! 本模块只做文件系统层面的编排（统计/淘汰/清空），不含任何业务语义。

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

/// 单个缓存目录的配额。`None` = 不按字节限制，只看条数。
pub struct Quota {
    /// 缓存根下的子目录名（相对 `{数据目录}/cache/`，或 `launcher-managed/` 里的相对路径）
    pub name: &'static str,
    /// 落在哪个大根下：true = `cache/`，false = `launcher-managed/`
    pub under_cache: bool,
    /// 最多保留的文件数
    pub max_files: usize,
    /// 最多占用的字节数
    pub max_bytes: u64,
}

/// 全部受管缓存。新增缓存请加在这里，否则设置页看不到、也不会被淘汰。
pub const QUOTAS: &[Quota] = &[
    // 远程 MOD 图标：每个文件是一张图（约 20 KB），实测 566 个已到 11 MB
    Quota { name: "mod-icons-v1", under_cache: true, max_files: 800, max_bytes: 12 * 1024 * 1024 },
    // 本地文件指纹（sha1 / CurseForge 指纹，按 mtime+size 失效）
    Quota { name: "mod-fingerprints-v1", under_cache: true, max_files: 8, max_bytes: 8 * 1024 * 1024 },
    // 投影列表元数据（方块数/版本）
    Quota { name: "projection-meta-v1.json", under_cache: true, max_files: 1, max_bytes: 4 * 1024 * 1024 },
    // 截图/世界图标缩略图
    Quota { name: "screenshot-cache", under_cache: false, max_files: 6000, max_bytes: 320 * 1024 * 1024 },
    // 世界地图瓦片
    Quota { name: "map-tile-cache", under_cache: false, max_files: 4000, max_bytes: 512 * 1024 * 1024 },
    // 反馈附件（贴图预览）
    Quota { name: "feedback-cache", under_cache: false, max_files: 200, max_bytes: 64 * 1024 * 1024 },
];

fn cache_root(app: &tauri::AppHandle) -> PathBuf {
    crate::settings::user_data_dir(app).join("cache")
}

fn managed_root(app: &tauri::AppHandle) -> PathBuf {
    crate::settings::user_data_dir(app).join("launcher-managed")
}

fn quota_root(app: &tauri::AppHandle, quota: &Quota) -> PathBuf {
    if quota.under_cache {
        cache_root(app).join(quota.name)
    } else {
        managed_root(app).join(quota.name)
    }
}

fn quota_for(name: &str) -> Option<&'static Quota> {
    QUOTAS.iter().find(|q| q.name == name)
}

/// 递归收集普通文件（不跟随符号链接）。目录本身不参与配额。
/// `path` 本身是文件时直接计入（单文件型缓存，如 mod-fingerprints-v1）。
fn collect_files(path: &Path, out: &mut Vec<(PathBuf, u64, SystemTime)>) {
    let Ok(meta) = std::fs::metadata(path) else {
        return;
    };
    if meta.is_file() {
        let used = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        out.push((path.to_path_buf(), meta.len(), used));
        return;
    }
    if !meta.is_dir() {
        return;
    }
    let Ok(reader) = std::fs::read_dir(path) else {
        return;
    };
    for entry in reader.flatten() {
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.is_dir() {
            collect_files(&entry.path(), out);
        } else if meta.is_file() {
            let used = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            out.push((entry.path(), meta.len(), used));
        }
    }
}

/// 统计一个受管缓存的体积与条数。
pub fn quota_usage(app: &tauri::AppHandle, quota: &Quota) -> (usize, u64) {
    let mut files = Vec::new();
    collect_files(&quota_root(app, quota), &mut files);
    (files.len(), files.iter().map(|(_, len, _)| *len).sum())
}

/// 按「最近修改时间」淘汰，直到回到配额以内。返回删除的文件数。
///
/// 选 mtime 而不是 atime：Windows 默认关闭 last-access 更新，读它只会得到一个
/// 永远不变的值；mtime 至少能反映「上次被写入」，对缓存场景足够。
pub fn enforce(app: &tauri::AppHandle, quota: &Quota) -> usize {
    let mut files = Vec::new();
    collect_files(&quota_root(app, quota), &mut files);
    let over_count = files.len().saturating_sub(quota.max_files);
    let total: u64 = files.iter().map(|(_, len, _)| *len).sum();
    if over_count == 0 && total <= quota.max_bytes {
        return 0;
    }
    // 最旧的先删；一次删到配额的 50%，避免每次写一个就触发一次全量排序
    files.sort_by(|a, b| a.2.cmp(&b.2));
    let mut removed = 0usize;
    let mut kept_bytes = total;
    let target_bytes = quota.max_bytes / 2;
    for (path, len, _) in &files {
        let over = files.len() - removed > quota.max_files;
        let too_big = kept_bytes > target_bytes;
        if !over && !too_big {
            break;
        }
        if std::fs::remove_file(path).is_ok() {
            removed += 1;
            kept_bytes = kept_bytes.saturating_sub(*len);
        }
    }
    removed
}

/// 某一路缓存是否已经超配额（写入后调用，便宜地判断要不要淘汰）。
pub fn needs_enforce(app: &tauri::AppHandle, quota: &Quota) -> bool {
    let (count, bytes) = quota_usage(app, quota);
    count > quota.max_files || bytes > quota.max_bytes
}

/// 全部受管缓存的体积清单（设置页展示 / 「清理缓存」）。
pub fn stats(app: &tauri::AppHandle) -> Value {
    let rows: Vec<Value> = QUOTAS
        .iter()
        .map(|q| {
            let (files, bytes) = quota_usage(app, q);
            json!({
                "name": q.name,
                "files": files,
                "bytes": bytes,
                "maxFiles": q.max_files,
                "maxBytes": q.max_bytes,
                "path": quota_root(app, q).to_string_lossy(),
            })
        })
        .collect();
    let total_bytes: u64 = QUOTAS
        .iter()
        .map(|q| quota_usage(app, q).1)
        .sum();
    json!({ "entries": rows, "totalBytes": total_bytes })
}

/// 清空指定缓存（`name` 为空 = 全部）。返回释放的字节数。
/// 只删受管缓存目录的内容，不碰存档/模组/配置。
pub fn clear(app: &tauri::AppHandle, name: &str) -> Result<u64, String> {
    let targets: Vec<&Quota> = match name.is_empty() {
        true => QUOTAS.iter().collect(),
        false => vec![quota_for(name).ok_or_else(|| format!("未知缓存：{name}"))?],
    };
    let mut freed = 0u64;
    for quota in targets {
        let root = quota_root(app, quota);
        let mut files = Vec::new();
        collect_files(&root, &mut files);
        for (path, len, _) in &files {
            if std::fs::remove_file(path).is_ok() {
                freed += len;
            }
        }
        // 文件删完的目录留着（下回写入不用重建），只清空内容
    }
    Ok(freed)
}

// ---------------- 文件指纹缓存（mods 用） ----------------
//
// PCL 的 ModHash.ini 用 `文件全名-mtime-size` 做键，命中就直接返回，不再读文件。
// 这里用同样的口径，但存成一份 JSON（跨启动有效），避免每次打开模组页
// 把 500+ 个 jar（合计 1 GB）整个读进内存再算 SHA1 / CurseForge 指纹。

const FINGERPRINT_FILE: &str = "mod-fingerprints-v1";

/// 指纹缓存的一条记录。
#[derive(Clone)]
pub struct Fingerprint {
    /// SHA1（hex 小写）
    pub sha1: String,
    /// CurseForge MurmurHash2 指纹
    pub fingerprint: u32,
    /// 完整解析结果（模组归档元数据）。命中时连 jar 都不用打开——
    /// PCL 的 `Cache\LocalMod.json` 就是这个思路：把解析结果整个存下来。
    pub info: Option<Value>,
}

fn fingerprint_path(app: &tauri::AppHandle) -> PathBuf {
    cache_root(app).join(FINGERPRINT_FILE)
}

/// 内存副本：避免每次扫描都重新解析整份指纹 JSON（实测 124 条 ≈ 300ms 里的大头）。
/// 以文件的 mtime+len 作为「磁盘被外部改动」的判据，跨进程启动时首次读盘一次。
static FINGERPRINT_MEM: Mutex<Option<(u64, u64, serde_json::Map<String, Value>)>> =
    Mutex::new(None);

fn fingerprint_load(app: &tauri::AppHandle) -> serde_json::Map<String, Value> {
    let path = fingerprint_path(app);
    let stamp = std::fs::metadata(&path)
        .ok()
        .map(|m| {
            (
                m.modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0),
                m.len(),
            )
        })
        .unwrap_or((0, 0));
    {
        let cache = FINGERPRINT_MEM.lock().unwrap();
        if let Some((mtime, len, entries)) = cache.as_ref() {
            if (*mtime, *len) == stamp && stamp != (0, 0) {
                return entries.clone();
            }
        }
    }
    let entries = std::fs::read(&path)
        .ok()
        .and_then(|raw| serde_json::from_slice::<Value>(&raw).ok())
        .and_then(|v| {
            (v["version"] == 1)
                .then(|| v["entries"].as_object().cloned())
                .flatten()
        })
        .unwrap_or_default();
    *FINGERPRINT_MEM.lock().unwrap() = Some((stamp.0, stamp.1, entries.clone()));
    entries
}

/// 写入后刷新内存副本，让紧随其后的读命中新内容（写入会改 mtime，下次读也能自洽）。
fn fingerprint_remember(app: &tauri::AppHandle, entries: serde_json::Map<String, Value>) {
    let path = fingerprint_path(app);
    let stamp = std::fs::metadata(&path)
        .ok()
        .map(|m| {
            (
                m.modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0),
                m.len(),
            )
        })
        .unwrap_or((0, 0));
    *FINGERPRINT_MEM.lock().unwrap() = Some((stamp.0, stamp.1, entries));
}

/// 路径 → 键前缀（FNV-1a 64 位）。只依赖路径，与文件状态无关。
fn fingerprint_hash(path: &Path) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in path.to_string_lossy().as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

/// 缓存键：路径哈希 + 大小 + mtime（毫秒）。大小或 mtime 一变即视为新文件。
pub fn fingerprint_key(path: &Path, len: u64, mtime_ms: u64) -> String {
    format!("{}-{len}-{mtime_ms}", fingerprint_hash(path))
}

/// 读一批文件的指纹；未命中的返回 None，由调用方补算后再 `fingerprint_put`。
pub fn fingerprint_get(
    app: &tauri::AppHandle,
    keys: &[String],
) -> std::collections::HashMap<String, Fingerprint> {
    let entries = fingerprint_load(app);
    let mut out = std::collections::HashMap::new();
    for key in keys {
        let Some(item) = entries.get(key) else {
            continue;
        };
        let (Some(sha1), Some(fp)) = (
            item["sha1"].as_str(),
            item["fingerprint"].as_u64(),
        ) else {
            continue;
        };
        out.insert(
            key.clone(),
            Fingerprint {
                sha1: sha1.to_string(),
                fingerprint: fp as u32,
                info: item.get("info").cloned().filter(|v| !v.is_null()),
            },
        );
    }
    out
}

/// 写入若干指纹（读改写整份文件；条数有配额上限，文件不会无限增长）。
pub fn fingerprint_put(
    app: &tauri::AppHandle,
    rows: &[(String, Fingerprint)],
) {
    if rows.is_empty() {
        return;
    }
    let mut entries = fingerprint_load(app);
    for (key, value) in rows {
        let mut row = json!({ "sha1": value.sha1, "fingerprint": value.fingerprint });
        if let Some(info) = &value.info {
            row["info"] = info.clone();
        }
        entries.insert(key.clone(), row);
    }
    // 软上限：超出后按插入顺序丢弃老的（serde_json Map 保留插入序）
    const KEEP: usize = 4000;
    while entries.len() > KEEP {
        let Some(first) = entries.keys().next().cloned() else {
            break;
        };
        entries.remove(&first);
    }
    let payload = json!({ "version": 1, "entries": entries.clone() });
    let path = fingerprint_path(app);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, payload.to_string()).is_ok() && std::fs::rename(&tmp, &path).is_ok() {
        fingerprint_remember(app, entries);
    }
}

/// 删掉某个路径的所有指纹（文件被替换/删除后调用，防止缓存漂移）。
pub fn fingerprint_forget(app: &tauri::AppHandle, path: &Path) {
    let mut entries = fingerprint_load(app);
    let prefix = format!("{}-", fingerprint_hash(path));
    let stale: Vec<String> = entries
        .keys()
        .filter(|k| k.starts_with(&prefix))
        .cloned()
        .collect();
    if stale.is_empty() {
        return;
    }
    for key in stale {
        entries.remove(&key);
    }
    let payload = json!({ "version": 1, "entries": entries.clone() });
    if let Ok(text) = serde_json::to_string(&payload) {
        if std::fs::write(fingerprint_path(app), text).is_ok() {
            fingerprint_remember(app, entries);
        }
    }
}

// ---------------- IPC ----------------

pub fn invoke(
    app: &tauri::AppHandle,
    channel: &str,
    args: &[Value],
) -> Option<Result<Value, String>> {
    match channel {
        "cache:stats" => Some(Ok(stats(app))),
        "cache:clear" => {
            let name = args.first().and_then(|v| v.as_str()).unwrap_or("");
            Some(clear(app, name).map(|bytes| json!({ "freedBytes": bytes })))
        }
        _ => None,
    }
}
