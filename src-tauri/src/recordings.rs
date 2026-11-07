//! 录像子系统（对应原版 core/recordingsIpc.ts + core/recordingFiles.ts + core/recordingMods.ts
//! 与 shared/recordings.ts 的返回形状）。
//!
//! 通道：recordings:list / operate / import / open / modVersions（invoke）、
//! recordings:drag（send）。
//!
//! 与原版的差异（语义保持一致的前提下）：
//! 1. recordings:list 按合同登记为后台任务（与原版 projectionsIpc 同型）：逐条目推
//!    event:progress，收尾 event:taskDone + finishTask；catalog 条目形状不变。
//! 2. yauzl 懒读目录 → zip crate 中央目录（by_index_raw，不整包解压）；
//!    AbortSignal → tasks::TaskRecord.cancelled 轮询 + wait_while_paused 暂停门。
//! 3. webContents.startDrag 无 Tauri 等价物：recordings:drag 完成全部校验后向渲染层
//!    推 files:dragError 字符串事件（与 fs:drag 失败通路同形）。
//! 4. shell.trashItem → PowerShell VisualBasic 回收站删除；shell.showItemInFolder →
//!    explorer /select；shell.openPath → explorer/xdg-open。
//! 5. Windows 无 inode 语义：复制后一致性校验改用 file_index/creation_time + size + mtime。
//! 6. validateRecording 返回解析后的元数据（原版返回 void），供后续子系统复用。

use crate::accounts::uuid_v4;
use crate::download::with_file_job;
use crate::installer::{self, read_version_json, same_path};
use crate::mods::{assert_instance_idle, recording_mod_versions};
use crate::settings;
use crate::tasks::{self, TaskRecord};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};

pub(crate) const SCAN_DIR_LIMIT: usize = 20000;
pub(crate) const SCAN_ENTRY_LIMIT: usize = 10000;
const META_LIMIT: u64 = 1024 * 1024;
const ZIP_ENTRY_LIMIT: usize = 100000;
pub(crate) const DRAG_LIMIT: usize = 1000;
pub(crate) const COLON: &str = "：";

static CATALOG: LazyLock<Mutex<HashMap<String, Stored>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static SCAN_GENERATION: AtomicU64 = AtomicU64::new(0);
/// 列目录是读取操作：串行化 + 短 TTL 结果缓存，吸收进页面瞬间的并发调用。
static SCAN_LOCK: Mutex<()> = Mutex::new(());
static SCAN_CACHE: Mutex<Option<(String, i64, Value)>> = Mutex::new(None);
const SCAN_CACHE_TTL_MS: i64 = 2_500;

fn now_ms_i64() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn cached_scan(key: &str) -> Option<Value> {
    SCAN_CACHE.lock().unwrap().as_ref().and_then(|(k, at, value)| {
        (k == key && now_ms_i64() - at < SCAN_CACHE_TTL_MS).then(|| value.clone())
    })
}

/// 最小化自整理：清空扫描目录与短 TTL 缓存（recordings:list 会按需重扫回填）。
pub fn release_catalog() {
    if let Ok(mut map) = CATALOG.lock() {
        map.clear();
    }
    if let Ok(mut cache) = SCAN_CACHE.lock() {
        *cache = None;
    }
}

#[derive(Clone)]
pub(crate) struct Source {
    pub(crate) folder: Option<PathBuf>,
    pub(crate) root: PathBuf,
    pub(crate) label: String,
    pub(crate) library: bool,
    /// 命中该目录的实例 id（实例目录才有；收藏/共享目录为 None）
    pub(crate) instance_id: Option<String>,
}

#[derive(Clone)]
pub(crate) struct Stored {
    pub(crate) entry: Value,
    pub(crate) source: Source,
    pub(crate) rel: String,
}

pub(crate) struct ScanErr {
    pub(crate) not_found: bool,
    pub(crate) message: String,
}

impl ScanErr {
    pub(crate) fn msg(message: impl Into<String>) -> Self {
        Self {
            not_found: false,
            message: message.into(),
        }
    }
}

impl From<std::io::Error> for ScanErr {
    fn from(e: std::io::Error) -> Self {
        Self {
            not_found: e.kind() == std::io::ErrorKind::NotFound,
            message: e.to_string(),
        }
    }
}

// ---------------- 路径工具（对应原版 backupStore.ts safePath/safeRelative） ----------------

pub(crate) fn resolve_abs(input: &Path) -> PathBuf {
    let absolute = input.is_absolute();
    let mut out = if absolute {
        PathBuf::new()
    } else {
        std::env::current_dir().unwrap_or_default()
    };
    let mut stack: Vec<std::ffi::OsString> = Vec::new();
    for comp in input.components() {
        match comp {
            Component::Prefix(p) => out = PathBuf::from(p.as_os_str()),
            Component::RootDir => out.push(std::path::MAIN_SEPARATOR_STR),
            Component::CurDir => {}
            Component::ParentDir => {
                stack.pop();
            }
            Component::Normal(name) => stack.push(name.to_os_string()),
        }
    }
    for name in stack {
        out.push(name);
    }
    out
}

pub(crate) fn key_of(input: &Path) -> String {
    let value = resolve_abs(input).to_string_lossy().into_owned();
    if cfg!(windows) {
        value.to_lowercase()
    } else {
        value
    }
}

pub(crate) fn key_of_str(input: &str) -> String {
    key_of(Path::new(input))
}

pub(crate) fn safe_relative(value: &str) -> Result<(), String> {
    let bad = value.is_empty()
        || value.contains('\\')
        || value.contains('\0')
        || value.starts_with('/')
        || value.split('/').any(|p| {
            p.is_empty() || p == "." || p == ".." || p.chars().any(|c| ":*?\"<>|".contains(c))
        });
    if bad {
        return Err(format!("无效的相对路径：{value}"));
    }
    Ok(())
}

pub(crate) fn safe_path(root: &Path, rel: &str, missing: bool) -> Result<PathBuf, ScanErr> {
    safe_relative(rel).map_err(ScanErr::msg)?;
    let base = resolve_abs(root);
    let base_stat = std::fs::symlink_metadata(&base).map_err(|e| ScanErr {
        not_found: e.kind() == std::io::ErrorKind::NotFound,
        message: "目录不是普通目录".into(),
    })?;
    if !base_stat.is_dir() || base_stat.is_symlink() {
        return Err(ScanErr::msg("目录不是普通目录"));
    }
    let mut current = base;
    for part in rel.split('/') {
        current = current.join(part);
        match std::fs::symlink_metadata(&current) {
            Ok(meta) if meta.is_symlink() => {
                return Err(ScanErr::msg(format!("不跟随符号链接：{rel}")))
            }
            Ok(_) => {}
            Err(e) => {
                let not_found = e.kind() == std::io::ErrorKind::NotFound;
                if missing && not_found {
                    continue;
                }
                return Err(ScanErr {
                    not_found,
                    message: e.to_string(),
                });
            }
        }
    }
    Ok(current)
}

pub(crate) fn real_path(input: &Path) -> Result<PathBuf, String> {
    let canonical = std::fs::canonicalize(input).map_err(|e| e.to_string())?;
    let text = canonical.to_string_lossy().into_owned();
    let mut out = text.clone();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        out = format!("\\{rest}");
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        out = rest.to_string();
    }
    Ok(PathBuf::from(out))
}

pub(crate) fn mtime_ms(meta: &std::fs::Metadata) -> f64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}

#[cfg(unix)]
pub(crate) fn file_id(meta: &std::fs::Metadata) -> u64 {
    std::os::unix::fs::MetadataExt::ino(meta)
}

#[cfg(windows)]
pub(crate) fn file_id(meta: &std::fs::Metadata) -> u64 {
    std::os::windows::fs::MetadataExt::creation_time(meta)
}

pub(crate) fn stat_of(path: &Path) -> Result<std::fs::Metadata, String> {
    std::fs::symlink_metadata(path).map_err(|e| e.to_string())
}

// ---------------- 目录常量 / 实例定位 ----------------

fn recording_dir(kind: &str) -> &'static str {
    if kind == "replaymod" {
        "replay_recordings"
    } else {
        "flashback/replays"
    }
}

pub(crate) fn active_root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let s = settings::get(app);
    let root = s["activeFolder"]
        .as_str()
        .filter(|v| !v.is_empty())
        .or_else(|| s["gameDir"].as_str().filter(|v| !v.is_empty()))
        .unwrap_or("")
        .to_string();
    let bound = s["folders"]
        .as_array()
        .map(|list| {
            list.iter().any(|f| {
                f["path"]
                    .as_str()
                    .map(|p| key_of_str(p) == key_of_str(&root))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false);
    if root.is_empty() || !bound {
        return Err("请先选择已登记的游戏文件夹".into());
    }
    Ok(resolve_abs(Path::new(&root)))
}

fn library_dir(root: &Path) -> Result<PathBuf, String> {
    let dir = safe_path(root, "recordings", true).map_err(|e| e.message)?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn recording_directory(root: &Path, kind: &str, create: bool) -> Result<PathBuf, String> {
    let dir = safe_path(root, recording_dir(kind), true).map_err(|e| e.message)?;
    if create {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    }
    Ok(dir)
}

fn validate_instance_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.len() > 80
        || name == "."
        || name == ".."
        || name
            .chars()
            .any(|c| "\\/:*?\"<>|".contains(c) || (c as u32) < 0x20)
    {
        return Err("实例名称无效".into());
    }
    Ok(())
}

// 对应原版 instanceCenter.ts centerTarget()；同时给出版本隔离状态（口径 = launch.rs effective_game_dir）
pub(crate) fn center_target_dir_state(
    app: &tauri::AppHandle,
    target: &Value,
) -> Result<(PathBuf, bool), String> {
    let id = target["id"].as_str().unwrap_or("");
    validate_instance_name(id)?;
    let folder = target["folder"].as_str().unwrap_or("");
    let s = settings::get(app);
    let registered = s["folders"]
        .as_array()
        .map(|list| {
            list.iter().any(|f| {
                f["path"]
                    .as_str()
                    .map(|p| same_path(p, folder))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false);
    if !registered {
        return Err("游戏文件夹尚未登记".into());
    }
    let real_folder = real_path(Path::new(folder)).map_err(|_| "游戏文件夹尚未登记".to_string())?;
    let folder_s = real_folder.to_string_lossy().into_owned();
    let json = read_version_json(app, &folder_s, id)?;
    let state = installer::instance_directory_state(app, &folder_s, id, &json);
    let declared = state["path"].as_str().unwrap_or("").to_string();
    let isolated = state["isolated"].as_bool().unwrap_or(false);
    if declared.is_empty() || !Path::new(&declared).exists() {
        return Err("实例目录不存在".into());
    }
    let dir = real_path(Path::new(&declared)).map_err(|_| "实例目录不存在".to_string())?;
    if !same_path(&declared, &dir.to_string_lossy()) {
        return Err("实例目录不能是符号链接".into());
    }
    if !dir.is_dir() {
        return Err("实例目录不存在".into());
    }
    Ok((dir, isolated))
}

pub(crate) fn center_target_dir(app: &tauri::AppHandle, target: &Value) -> Result<PathBuf, String> {
    center_target_dir_state(app, target).map(|(dir, _)| dir)
}

pub(crate) fn installed_version_ids(root: &Path) -> (Vec<String>, Vec<String>) {
    let dir = root.join("versions");
    if !dir.exists() {
        return (Vec::new(), Vec::new());
    }
    let rd = match std::fs::read_dir(&dir) {
        Ok(rd) => rd,
        Err(e) => return (Vec::new(), vec![format!("无法读取 versions：{e}")]),
    };
    let mut ids = Vec::new();
    let mut errors = Vec::new();
    for entry in rd.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if !entry.path().join(format!("{name}.json")).exists() {
            errors.push(format!("{name}：缺少版本描述 {name}.json"));
        }
        ids.push(name);
    }
    (ids, errors)
}

// ---------------- catalog / 选择项 ----------------

fn selected_items(app: &tauri::AppHandle, ids: &Value) -> Result<Vec<Stored>, String> {
    let arr = ids.as_array().ok_or("请选择录像文件".to_string())?;
    if arr.is_empty() || arr.len() > SCAN_ENTRY_LIMIT {
        return Err("请选择录像文件".into());
    }
    let s = settings::get(app);
    let bound: HashSet<String> = s["folders"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|f| f["path"].as_str())
        .map(key_of_str)
        .collect();
    let catalog = CATALOG.lock().unwrap();
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for raw in arr {
        let id = raw.as_str().unwrap_or("").to_string();
        if !seen.insert(id.clone()) {
            continue;
        }
        let matched = catalog.get(&id).filter(|stored| {
            stored
                .source
                .folder
                .as_ref()
                .map(|f| bound.contains(&key_of(f)))
                .unwrap_or(false)
        });
        match matched {
            Some(stored) => out.push(stored.clone()),
            None => return Err("录像所属文件夹已解除绑定，请刷新列表".into()),
        }
    }
    Ok(out)
}

pub(crate) fn current_file(item: &Stored) -> Result<PathBuf, String> {
    let file = safe_path(&item.source.root, &item.rel, false).map_err(|e| e.message)?;
    let meta = stat_of(&file)?;
    let size = item.entry["size"].as_u64().unwrap_or(0);
    let modified = item.entry["modified"].as_f64().unwrap_or(0.0);
    if !meta.is_file() || meta.is_symlink() || meta.len() != size || mtime_ms(&meta) != modified {
        return Err("录像已变化，请刷新列表后重试".into());
    }
    Ok(file)
}

pub(crate) fn drag_path(root: &Path, rel: &str) -> Result<PathBuf, String> {
    safe_relative(rel)?;
    let base = resolve_abs(root);
    let base_stat = stat_of(&base)?;
    if !base_stat.is_dir() || base_stat.is_symlink() {
        return Err("目录不是普通目录".into());
    }
    let mut current = base;
    for part in rel.split('/') {
        current = current.join(part);
        if stat_of(&current)?.is_symlink() {
            return Err("不跟随符号链接".into());
        }
    }
    let meta = stat_of(&current)?;
    if !meta.is_file() && !meta.is_dir() {
        return Err("不支持拖出特殊文件".into());
    }
    Ok(current)
}

// ---------------- 回收站（recycleFile.ts 等价） ----------------

fn recycle_bin_delete(target: &Path) -> Result<(), String> {
    // shell.trashItem 等价：trash crate 跨平台（Windows IFileOperation / macOS Finder / Linux freedesktop）。
    // trash crate 不支持 Android：Android 沙箱内无系统回收站，降级为直接删除。
    #[cfg(not(target_os = "android"))]
    {
        trash::delete(target).map_err(|e| e.to_string())
    }
    #[cfg(target_os = "android")]
    {
        if target.is_dir() {
            std::fs::remove_dir_all(target).map_err(|e| e.to_string())
        } else {
            std::fs::remove_file(target).map_err(|e| e.to_string())
        }
    }
}

// 对应原版 recycleFile.ts recycleFile()
pub(crate) fn recycle_file(directory: &Path, name: &str) -> Result<(), String> {
    let bad_name = name.is_empty()
        || name == "."
        || name == ".."
        || name
            .chars()
            .any(|c| "\\/:*?\"<>|\0".contains(c) || (c as u32) < 0x20)
        || name.ends_with('.')
        || name.ends_with(' ');
    if bad_name {
        return Err("文件名称无效，未删除任何文件".into());
    }
    let parent = resolve_abs(directory);
    let target = parent.join(name);
    if target.parent() != Some(parent.as_path()) {
        return Err("文件路径越界".into());
    }
    let parent_stat = stat_of(&parent)?;
    if !parent_stat.is_dir() || parent_stat.is_symlink() {
        return Err("资源目录是链接或不是文件夹，请先确认实际位置".into());
    }
    let stat = stat_of(&target)?;
    if stat.is_symlink() {
        return Err("文件是链接，请在系统文件管理器中确认实际位置".into());
    }
    let real_parent = real_path(&parent)?;
    let real_target = real_path(&target)?;
    if real_target.parent() != Some(real_parent.as_path()) {
        return Err("文件实际路径越界".into());
    }
    let current = stat_of(&target)?;
    #[cfg(unix)]
    let moved = current.is_symlink()
        || std::os::unix::fs::MetadataExt::ino(&current)
            != std::os::unix::fs::MetadataExt::ino(&stat)
        || std::os::unix::fs::MetadataExt::dev(&current)
            != std::os::unix::fs::MetadataExt::dev(&stat);
    #[cfg(not(unix))]
    let moved = current.is_symlink() || file_id(&current) != file_id(&stat);
    if moved {
        return Err("文件已变化，请刷新后重试".into());
    }
    let failure = match recycle_bin_delete(&real_target) {
        Ok(()) => match std::fs::symlink_metadata(&real_target) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => e.to_string(),
            Ok(_) => "系统未移除该文件".to_string(),
        },
        Err(e) => e,
    };
    Err(format!("无法移入系统回收站，未执行永久删除。请检查文件占用、权限及磁盘的回收站支持：{name}{COLON}{failure}"))
}

// ---------------- recordingFiles.ts ----------------

fn hash_file(path: &Path, rec: Option<&TaskRecord>) -> Result<String, String> {
    if let Some(task) = rec {
        tasks::wait_while_paused(task);
        if task.cancelled.load(Ordering::SeqCst) {
            return Err("已取消".into());
        }
    }
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut reader = std::io::BufReader::new(file);
    let mut digest = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer).map_err(|e| e.to_string())?;
        if read == 0 {
            break;
        }
        if let Some(task) = rec {
            tasks::wait_while_paused(task);
            if task.cancelled.load(Ordering::SeqCst) {
                return Err("已取消".into());
            }
        }
        digest.update(&buffer[..read]);
    }
    Ok(hex::encode(digest.finalize()))
}

// 对应原版 recordingFiles.ts validateRecording()
pub fn validate_recording(file: &Path, kind: &str) -> Result<Value, String> {
    const CORRUPT: &str = "录像压缩包损坏或尚未保存完成";
    let handle = std::fs::File::open(file).map_err(|_| CORRUPT.to_string())?;
    let mut archive = zip::ZipArchive::new(handle).map_err(|_| CORRUPT.to_string())?;
    let meta_name = if kind == "replaymod" {
        "metaData.json"
    } else {
        "metadata.json"
    };
    let mut names: HashSet<String> = HashSet::new();
    let mut count = 0usize;
    let mut meta_index: Option<usize> = None;
    for index in 0..archive.len() {
        count += 1;
        if count > ZIP_ENTRY_LIMIT {
            return Err("录像条目过多".into());
        }
        let entry = archive
            .by_index_raw(index)
            .map_err(|_| CORRUPT.to_string())?;
        let name = entry.name().to_string();
        names.insert(name.clone());
        if name == meta_name {
            if meta_index.is_some() || entry.size() > META_LIMIT {
                return Err("录像元数据无效或过大".into());
            }
            meta_index = Some(index);
        }
    }
    let metadata: Value = match meta_index {
        None => Value::Null,
        Some(index) => {
            let entry = archive
                .by_index(index)
                .map_err(|_| "无法读取录像元数据".to_string())?;
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut entry.take(META_LIMIT + 1), &mut bytes)
                .map_err(|_| "无法读取录像元数据".to_string())?;
            if bytes.len() as u64 > META_LIMIT {
                return Err("录像元数据过大".into());
            }
            let parsed: Value =
                serde_json::from_slice(&bytes).map_err(|_| "录像元数据损坏".to_string())?;
            if !parsed.is_object() {
                return Err("录像元数据损坏".into());
            }
            parsed
        }
    };
    let valid = if kind == "replaymod" {
        metadata.is_object() && names.contains("recording.tmcpr")
    } else {
        let chunks_ok = match metadata.get("chunks") {
            Some(Value::Object(map)) => {
                !map.is_empty() && map.keys().all(|name| names.contains(name))
            }
            _ => false,
        };
        metadata.is_object()
            && metadata.get("uuid").map(|v| v.is_string()).unwrap_or(false)
            && chunks_ok
    };
    if !valid {
        return Err(format!(
            "不是完整的 {} 录像",
            if kind == "replaymod" {
                "ReplayMod"
            } else {
                "Flashback"
            }
        ));
    }
    Ok(metadata)
}

struct StageGuard(PathBuf);

impl Drop for StageGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

// 对应原版 recordingFiles.ts copyRecording()
fn copy_recording_core(
    source: &Path,
    destination: &Path,
    rec: Option<&TaskRecord>,
    mut on_bytes: &mut dyn FnMut(u64),
) -> Result<PathBuf, String> {
    const CHANGED: &str = "录像在复制期间发生变化，请停止录制后重试";
    let before = stat_of(source)?;
    if !before.is_file() || before.is_symlink() {
        return Err("录像不是普通文件".into());
    }
    let root = real_path(destination)?;
    if stat_of(destination)?.is_symlink() {
        return Err("目标目录不能是链接".into());
    }
    let stage = root.join(format!(".kamucl-recording-{}.part", uuid_v4()));
    let _guard = StageGuard(stage.clone());
    let mut reader = std::fs::File::open(source).map_err(|e| e.to_string())?;
    let mut writer = std::fs::File::create_new(&stage).map_err(|e| e.to_string())?;
    let mut digest = Sha256::new();
    let mut transferred = 0u64;
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        if let Some(task) = rec {
            tasks::wait_while_paused(task);
            if task.cancelled.load(Ordering::SeqCst) {
                return Err("已取消".into());
            }
        }
        let read = reader.read(&mut buffer).map_err(|e| e.to_string())?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
        writer
            .write_all(&buffer[..read])
            .map_err(|e| e.to_string())?;
        transferred += read as u64;
        on_bytes(transferred);
    }
    writer.flush().map_err(|e| e.to_string())?;
    drop(writer);
    let after = stat_of(source)?;
    if after.is_symlink()
        || file_id(&after) != file_id(&before)
        || after.len() != before.len()
        || mtime_ms(&after) != mtime_ms(&before)
    {
        return Err(CHANGED.into());
    }
    let sha = hex::encode(digest.finalize());
    if sha != hash_file(source, rec)? || sha != hash_file(&stage, rec)? {
        return Err(CHANGED.into());
    }
    if let Some(task) = rec {
        if task.cancelled.load(Ordering::SeqCst) {
            return Err("已取消".into());
        }
    }
    let stem = source
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = source
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let source_abs = resolve_abs(source);
    for index in 0..10000usize {
        let name = if index == 0 {
            format!("{stem}{ext}")
        } else {
            format!("{stem} ({index}){ext}")
        };
        let dest = safe_path(&root, &name, true).map_err(|e| e.message)?;
        if resolve_abs(&dest) == source_abs {
            return Ok(source.to_path_buf());
        }
        match std::fs::File::create_new(&dest) {
            Ok(mut out) => {
                let mut from = std::fs::File::open(&stage).map_err(|e| e.to_string())?;
                std::io::copy(&mut from, &mut out).map_err(|e| e.to_string())?;
                out.flush().map_err(|e| e.to_string())?;
                return Ok(dest);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.to_string()),
        }
    }
    Err("同名录像过多，请整理目标目录".into())
}

pub fn copy_recording(
    app: &tauri::AppHandle,
    source: &Path,
    destination: &Path,
    task_id: &str,
) -> Result<PathBuf, String> {
    let rec = tasks::get_task(task_id);
    let size = std::fs::symlink_metadata(source)
        .map(|m| m.len())
        .unwrap_or(0);
    let title = rec.as_ref().map(|t| t.title.clone()).unwrap_or_default();
    let name = source
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut on_bytes = move |bytes: u64| {
        let progress = if size > 0 {
            (bytes as f64 / size as f64).min(1.0)
        } else {
            1.0
        };
        tasks::emit_progress(
            app,
            json!({ "taskId": task_id, "taskTitle": title, "stage": "world", "progress": progress, "text": name }),
        );
    };
    copy_recording_core(source, destination, rec.as_deref(), &mut on_bytes)
}

// ---------------- recordingsIpc.ts scan() ----------------

fn visit_source(
    source: &Source,
    kind: &str,
    rel: &str,
    depth: usize,
    visited: &mut usize,
    found: &mut HashMap<String, Stored>,
    order: &mut Vec<String>,
) -> Result<(), ScanErr> {
    *visited += 1;
    if *visited > SCAN_DIR_LIMIT {
        return Err(ScanErr::msg(
            "扫描已达到 20000 个目录/文件上限，请分目录整理",
        ));
    }
    let dir = safe_path(&source.root, rel, true)?;
    let reader = match std::fs::read_dir(&dir) {
        Ok(reader) => reader,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(ScanErr::from(e)),
    };
    let mut listing = Vec::new();
    for entry in reader {
        let entry = entry.map_err(ScanErr::from)?;
        let file_type = entry.file_type().map_err(ScanErr::from)?;
        listing.push((
            entry.file_name().to_string_lossy().into_owned(),
            file_type.is_dir(),
            file_type.is_file(),
            file_type.is_symlink(),
        ));
    }
    let suffix = if kind == "replaymod" { ".mcpr" } else { ".zip" };
    for (name, is_dir, is_file, is_link) in listing {
        if is_link || name.starts_with('.') {
            continue;
        }
        let next = format!("{rel}/{name}");
        if is_dir {
            if depth < 4 {
                let lower = name.to_lowercase();
                if !lower.ends_with(".cache")
                    && !lower.ends_with(".tmp")
                    && !lower.ends_with(".del")
                {
                    visit_source(source, kind, &next, depth + 1, visited, found, order)?;
                }
            }
            continue;
        }
        if !is_file || !name.to_lowercase().ends_with(suffix) {
            continue;
        }
        if found.len() >= SCAN_ENTRY_LIMIT {
            return Err(ScanErr::msg("仅显示前 10000 个录像文件"));
        }
        let full = safe_path(&source.root, &next, false)?;
        let meta = std::fs::symlink_metadata(&full).map_err(ScanErr::from)?;
        let id = hex::encode(Sha256::digest(key_of(&full).as_bytes()));
        let mut entry = Map::new();
        if let Some(folder) = &source.folder {
            entry.insert(
                "folder".into(),
                json!(folder.to_string_lossy().into_owned()),
            );
        }
        if let Some(instance_id) = &source.instance_id {
            entry.insert("instanceId".into(), json!(instance_id));
        }
        entry.insert("id".into(), json!(id.clone()));
        entry.insert("name".into(), json!(name));
        entry.insert("kind".into(), json!(kind));
        entry.insert("size".into(), json!(meta.len()));
        entry.insert("modified".into(), json!(mtime_ms(&meta)));
        entry.insert("source".into(), json!(source.label));
        entry.insert(
            "directory".into(),
            json!(full
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default()),
        );
        entry.insert("library".into(), json!(source.library));
        if !found.contains_key(&id) {
            order.push(id.clone());
        }
        found.insert(
            id,
            Stored {
                entry: Value::Object(entry),
                source: source.clone(),
                rel: next,
            },
        );
    }
    Ok(())
}

fn scan_inner(
    app: &tauri::AppHandle,
    rec: &TaskRecord,
    scope: &str,
    target: &Value,
) -> Result<Value, String> {
    let target_folder: Option<String> = target["folder"].as_str().map(str::to_string);
    let target_id: Option<String> = target["id"].as_str().map(str::to_string);
    let generation = SCAN_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let root = active_root(app)?;
    let collection = root.join("recordings");
    let settings_value = settings::get(app);
    let registered: Vec<Value> = {
        let mut seen: HashSet<String> = HashSet::new();
        settings_value["folders"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|f| seen.insert(key_of_str(f["path"].as_str().unwrap_or(""))))
            .collect()
    };
    let mut sources: Vec<Source> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut instances: Vec<Value> = Vec::new();
    // 实例视图（scope=instance）：只看选定实例 + 它所在文件夹的收藏/共享目录；
    // scope=folder：整个版本文件夹。scope=all：全部文件夹（旧行为，保留给“全库”）。

    for folder in &registered {
        let root_path = resolve_abs(Path::new(folder["path"].as_str().unwrap_or("")));
        let root_text = root_path.to_string_lossy().into_owned();
        let label = folder["name"]
            .as_str()
            .filter(|v| !v.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| root_text.clone());
        if scope == "instance" || scope == "folder" {
            // 只处理当前实例所在的文件夹（folder_text 与选定 folder 同路径）
            let active = target_folder.as_deref().unwrap_or("");
            if !same_path(&root_text, active) {
                continue;
            }
        }
        sources.push(Source {
            folder: Some(root_path.clone()),
            root: root_path.join("recordings"),
            label: format!("{label} · 集中收藏"),
            library: true,
            instance_id: None,
        });
        let (ids, errors) = installed_version_ids(&root_path);
        for error in errors {
            warnings.push(format!("{root_text}{COLON}{error}"));
        }
        // 分类讨论（版本隔离）：隔离开 = 录像在 versions/<id>/ 下；隔离关 = 游戏工作目录就是
        // 文件夹根，录像落在根级 replay_recordings / flashback，且为所有未隔离实例共用——
        // 只有这种情况下根级目录才成其为一个来源，文件夹视图里统一标「未隔离实例共用」。
        let mut shared_needed = false;
        for id in ids {
            if scope == "instance" {
                // 只保留当前实例
                if Some(id.as_str()) != target_id.as_deref() {
                    continue;
                }
            }
            match center_target_dir_state(app, &json!({ "folder": root_text, "id": id })) {
                Ok((dir, isolated)) => {
                    if isolated {
                        sources.push(Source {
                            folder: Some(root_path.clone()),
                            root: dir,
                            label: id.clone(),
                            library: false,
                            instance_id: Some(id.clone()),
                        });
                    } else if scope == "instance" {
                        // 实例视图：选定实例未隔离，根级目录就是它的录像目录，挂它的名字
                        sources.push(Source {
                            folder: Some(root_path.clone()),
                            root: root_path.clone(),
                            label: id.clone(),
                            library: false,
                            instance_id: Some(id.clone()),
                        });
                    } else {
                        shared_needed = true;
                    }
                    instances.push(json!({ "folder": root_text, "id": id, "name": format!("{id} · {root_text}"), "isolated": isolated }));
                }
                Err(_) => warnings.push(format!("无法读取实例：{root_text}{COLON}{id}")),
            }
        }
        if scope == "folder" && shared_needed {
            sources.push(Source {
                folder: Some(root_path.clone()),
                root: root_path.clone(),
                label: format!("{label} · 未隔离实例共用"),
                library: false,
                instance_id: None,
            });
        }
    }
    let mut found: HashMap<String, Stored> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut visited = 0usize;
    for source in &sources {
        if !seen.insert(key_of(&source.root)) {
            continue;
        }
        for kind in ["replaymod", "flashback"] {
            if let Err(e) = visit_source(
                source,
                kind,
                recording_dir(kind),
                0,
                &mut visited,
                &mut found,
                &mut order,
            ) {
                if !e.not_found {
                    warnings.push(format!("{}{COLON}{}", source.label, e.message));
                }
            }
        }
    }
    // 列目录本身没有逐文件工作：这里只收尾（旧写法按条目数空转推 N 条进度事件，
    // 除了把下载中心刷成"扫描中"以外没有任何作用）。
    tasks::wait_while_paused(rec);
    if rec.cancelled.load(Ordering::SeqCst) {
        return Err("已取消".into());
    }
    if SCAN_GENERATION.load(Ordering::SeqCst) == generation {
        let mut catalog = CATALOG.lock().unwrap();
        catalog.clear();
        for id in &order {
            if let Some(stored) = found.get(id) {
                catalog.insert(id.clone(), stored.clone());
            }
        }
    }
    let mut entries: Vec<Value> = order
        .iter()
        .filter_map(|id| found.get(id))
        .map(|stored| stored.entry.clone())
        .collect();
    entries.sort_by(|a, b| {
        b["modified"]
            .as_f64()
            .unwrap_or(0.0)
            .total_cmp(&a["modified"].as_f64().unwrap_or(0.0))
    });
    Ok(json!({
        "entries": entries,
        "warnings": warnings,
        "library": collection.to_string_lossy(),
        "instances": instances
    }))
}

// 对应原版 recordingsIpc.ts scan()
/// recordings:list —— 列目录是「读取」，不注册后台任务（否则每次进页面都在下载中心
/// 凭空多出一条「扫描全部目录」）；短 TTL 缓存吸收进页面瞬间的重复调用。
fn list(app: &tauri::AppHandle, scope: &str, target: &Value, force: bool) -> Result<Value, String> {
    let cache_key = format!(
        "{scope}\u{0}{}\u{0}{}",
        target["folder"].as_str().unwrap_or(""),
        target["id"].as_str().unwrap_or("")
    );
    if !force {
        if let Some(value) = cached_scan(&cache_key) {
            return Ok(value);
        }
    }
    let _guard = SCAN_LOCK.lock().unwrap();
    if !force {
        if let Some(value) = cached_scan(&cache_key) {
            return Ok(value);
        }
    }
    let rec = tasks::register_quiet_task("录像 · 读取目录", "world");
    let outcome = scan_inner(app, &rec, scope, target);
    if let Ok(value) = &outcome {
        *SCAN_CACHE.lock().unwrap() = Some((cache_key, now_ms_i64(), value.clone()));
    }
    outcome
}

// ---------------- 操作（collect / export / dispatch / trash） ----------------

fn action_title(action: &str) -> &'static str {
    match action {
        "collect" => "收集",
        "export" => "提取",
        "dispatch" => "复制到实例",
        _ => "移入回收站",
    }
}

fn emit_run_progress(
    app: &tauri::AppHandle,
    task_id: &str,
    task_title: &str,
    total: u64,
    base: u64,
    bytes: u64,
    processed: usize,
    count: usize,
    name: &str,
) {
    let progress = if total > 0 {
        ((base + bytes) as f64 / total as f64).min(1.0)
    } else {
        0.0
    };
    tasks::emit_progress(
        app,
        json!({
            "taskId": task_id,
            "taskTitle": task_title,
            "stage": "world",
            "progress": progress,
            "text": format!("{processed}/{count} · {name}")
        }),
    );
}

fn run_one(
    app: &tauri::AppHandle,
    rec: &TaskRecord,
    item: &Stored,
    action: &str,
    destination: Option<&Path>,
    task_id: &str,
    task_title: &str,
    total: u64,
    base: u64,
    processed: usize,
    count: usize,
) -> Result<Option<PathBuf>, String> {
    tasks::wait_while_paused(rec);
    if rec.cancelled.load(Ordering::SeqCst) {
        return Err("已取消".into());
    }
    let name = item.entry["name"].as_str().unwrap_or("").to_string();
    let kind = item.entry["kind"]
        .as_str()
        .unwrap_or("replaymod")
        .to_string();
    emit_run_progress(
        app, task_id, task_title, total, base, 0, processed, count, &name,
    );
    let file = current_file(item)?;
    if action == "trash" {
        let inner = item.clone();
        let library_source = inner.source.library;
        with_file_job(&inner.source.root, Some(rec), || -> Result<(), String> {
            if !library_source {
                assert_instance_idle(app, &inner.source.root)?;
            }
            current_file(&inner)?;
            let full = safe_path(&inner.source.root, &inner.rel, false).map_err(|e| e.message)?;
            let parent = full.parent().map(|p| p.to_path_buf()).unwrap_or_default();
            let leaf = full
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            recycle_file(&parent, &leaf)
        })?;
        return Ok(None);
    }
    validate_recording(&file, &kind)?;
    let root = destination
        .ok_or("未指定录像目标目录".to_string())?
        .to_path_buf();
    let mut on_bytes = {
        let (name, app_ref, tid, title) = (name.clone(), app, task_id, task_title);
        move |bytes: u64| {
            emit_run_progress(
                app_ref, tid, title, total, base, bytes, processed, count, &name,
            )
        }
    };
    let mut output: Option<PathBuf> = None;
    {
        let action_owned = action.to_string();
        let kind_for_dir = kind.clone();
        let file_for_copy = file.clone();
        let root_for_lock = root.clone();
        let rec_ref: Option<&TaskRecord> = Some(rec);
        with_file_job(&root_for_lock, rec_ref, || -> Result<(), String> {
            if action_owned == "dispatch" {
                assert_instance_idle(app, &root_for_lock)?;
            }
            let dir = if action_owned == "export" {
                root_for_lock.clone()
            } else {
                recording_directory(&root_for_lock, &kind_for_dir, true)?
            };
            let copied = copy_recording_core(&file_for_copy, &dir, rec_ref, &mut on_bytes)?;
            output = Some(copied);
            Ok(())
        })?;
    }
    Ok(output)
}

fn run(
    app: &tauri::AppHandle,
    items: Vec<Stored>,
    action: &str,
    destination: Option<&Path>,
) -> Result<Value, String> {
    let rec = tasks::register_task(&format!("录像文件 · {}", action_title(action)), "world");
    let task_id = rec.id.clone();
    let task_title = rec.title.clone();
    let total: u64 = items
        .iter()
        .map(|item| item.entry["size"].as_u64().unwrap_or(0))
        .sum();
    let count = items.len();
    let mut results: Vec<Value> = Vec::new();
    let mut done = 0u64;
    for item in &items {
        let id = item.entry["id"].as_str().unwrap_or("").to_string();
        let name = item.entry["name"].as_str().unwrap_or("").to_string();
        let size = item.entry["size"].as_u64().unwrap_or(0);
        let processed = results.len();
        let outcome = run_one(
            app,
            &rec,
            item,
            action,
            destination,
            &task_id,
            &task_title,
            total,
            done,
            processed,
            count,
        );
        match outcome {
            Ok(path) => {
                let mut row = json!({ "id": id, "name": name, "ok": true });
                if let Some(p) = path {
                    row["path"] = json!(p.to_string_lossy().into_owned());
                }
                results.push(row);
            }
            Err(e) => {
                let message = if rec.cancelled.load(Ordering::SeqCst) {
                    "任务已取消，原录像保留".to_string()
                } else {
                    e
                };
                results.push(json!({ "id": id, "name": name, "ok": false, "error": message }));
            }
        }
        done += size;
    }
    let failed = results
        .iter()
        .filter(|row| row["ok"].as_bool() != Some(true))
        .count();
    let cancelled = rec.cancelled.load(Ordering::SeqCst);
    let error = if failed > 0 {
        Some(format!("{failed} 个录像处理失败，请查看录像页结果"))
    } else {
        None
    };
    tasks::task_done(app, &task_id, failed == 0, error, cancelled, None);
    Ok(Value::Array(results))
}

fn operate(app: &tauri::AppHandle, args: &[Value]) -> Result<Value, String> {
    let request = args.first().cloned().unwrap_or(Value::Null);
    let action = request["action"].as_str().unwrap_or("").to_string();
    let ids = request["ids"].clone();
    let valid_action = matches!(action.as_str(), "collect" | "export" | "dispatch" | "trash");
    let id_len = ids.as_array().map(|a| a.len()).unwrap_or(0);
    if !request.is_object()
        || !valid_action
        || !ids.is_array()
        || id_len < 1
        || id_len > SCAN_ENTRY_LIMIT
    {
        return Err("录像操作参数无效".into());
    }
    let root = active_root(app)?;
    let items = selected_items(app, &ids)?;
    let mut destination: Option<PathBuf> = None;
    match action.as_str() {
        "collect" => destination = Some(library_dir(&root)?),
        "dispatch" => {
            let target = request
                .get("target")
                .filter(|v| v.is_object())
                .ok_or("请选择目标实例".to_string())?;
            destination = Some(center_target_dir(app, target)?);
        }
        "export" => {
            // 移动端 SAF 无系统文件夹选择器，按用户取消处理
            #[cfg(mobile)]
            {
                return Ok(Value::Null);
            }
            #[cfg(desktop)]
            {
                use tauri_plugin_dialog::DialogExt;
                let picked = app
                    .dialog()
                    .file()
                    .set_title("选择录像提取文件夹")
                    .blocking_pick_folder();
                let Some(picked) = picked else {
                    return Ok(Value::Null);
                };
                destination = Some(picked.into_path().map_err(|e| e.to_string())?);
            }
        }
        _ => {}
    }
    run(app, items, &action, destination.as_deref())
}

fn import(app: &tauri::AppHandle) -> Result<Value, String> {
    use tauri_plugin_dialog::DialogExt;
    let picked = app
        .dialog()
        .file()
        .set_title("导入录像到集中收藏")
        .add_filter("ReplayMod / Flashback", &["mcpr", "zip"])
        .blocking_pick_files();
    let Some(picked) = picked else {
        return Ok(Value::Null);
    };
    let files = picked
        .into_iter()
        .map(|raw| raw.into_path())
        .collect::<Result<Vec<PathBuf>, _>>()
        .map_err(|e| e.to_string())?;
    import_paths(app, files)
}

/// importAt：拖入/外部传入的文件路径，落点与 import 一致（集中收藏）
fn import_at(app: &tauri::AppHandle, args: &[Value]) -> Result<Value, String> {
    let files = args
        .first()
        .and_then(|v| v.as_array())
        .ok_or("请拖入 .mcpr 或 Flashback .zip 录像".to_string())?
        .iter()
        .filter_map(|v| v.as_str())
        .map(PathBuf::from)
        .collect::<Vec<PathBuf>>();
    if files.is_empty() {
        return Err("请拖入 .mcpr 或 Flashback .zip 录像".into());
    }
    import_paths(app, files)
}

fn import_paths(app: &tauri::AppHandle, files: Vec<PathBuf>) -> Result<Value, String> {
    let root = active_root(app)?;
    let destination = library_dir(&root)?;
    let mut items: Vec<Stored> = Vec::new();
    for file in files {
        let meta = stat_of(&file)?;
        let kind = if file.to_string_lossy().to_lowercase().ends_with(".mcpr") {
            "replaymod"
        } else {
            "flashback"
        };
        let rel = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let parent = file.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        let mut entry = Map::new();
        entry.insert("id".into(), json!(file.to_string_lossy().into_owned()));
        entry.insert("name".into(), json!(rel.clone()));
        entry.insert("kind".into(), json!(kind));
        entry.insert("size".into(), json!(meta.len()));
        entry.insert("modified".into(), json!(mtime_ms(&meta)));
        entry.insert("source".into(), json!("导入"));
        entry.insert(
            "directory".into(),
            json!(parent.to_string_lossy().into_owned()),
        );
        entry.insert("library".into(), json!(false));
        items.push(Stored {
            entry: Value::Object(entry),
            source: Source {
                folder: None,
                root: parent,
                label: "导入".into(),
                library: false,
                instance_id: None,
            },
            rel,
        });
    }
    run(app, items, "collect", Some(&destination))
}

fn open_directory(dir: &Path) -> Result<(), String> {
    crate::platform::open_path(&dir.to_string_lossy())
}

fn reveal_item(path: &Path) -> Result<(), String> {
    crate::platform::show_in_folder(&path.to_string_lossy())
}

fn open(app: &tauri::AppHandle, args: &[Value]) -> Result<Value, String> {
    let id = args
        .first()
        .and_then(|v| v.as_str())
        .filter(|v| !v.is_empty())
        .map(str::to_string);
    match id {
        Some(id) => {
            let item = selected_items(app, &json!([id]))?
                .into_iter()
                .next()
                .ok_or("请选择录像文件".to_string())?;
            reveal_item(&current_file(&item)?)?;
        }
        None => {
            let root = active_root(app)?;
            open_directory(&library_dir(&root)?)?;
        }
    }
    Ok(Value::Null)
}

fn mod_versions(app: &tauri::AppHandle, args: &[Value]) -> Result<Value, String> {
    let kind = args
        .first()
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let mc = args
        .get(1)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let loader = args
        .get(2)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    installer::run_blocking(|| recording_mod_versions(app, &kind, &mc, &loader)).map(Value::Array)
}

// ---------------- recordings:drag（ipcMain.on 等价） ----------------

fn handle_drag(app: &tauri::AppHandle, args: &[Value]) {
    let outcome: Result<(), String> = (|| {
        let items = selected_items(app, args.first().unwrap_or(&Value::Null))?;
        if items.len() > DRAG_LIMIT {
            return Err(format!("每次最多拖出 {DRAG_LIMIT} 个录像，请分批选择"));
        }
        let mut paths: Vec<String> = Vec::with_capacity(items.len());
        for item in &items {
            let file = drag_path(&item.source.root, &item.rel)?;
            let meta = stat_of(&file)?;
            let size = item.entry["size"].as_u64().unwrap_or(0);
            let modified = item.entry["modified"].as_f64().unwrap_or(0.0);
            if !meta.is_file() || meta.len() != size || mtime_ms(&meta) != modified {
                return Err("录像已变化，请刷新后重试".into());
            }
            paths.push(file.to_string_lossy().to_string());
        }
        crate::dragout::validate_paths(&paths)?;
        // DoDragDrop 模态循环：阻塞到玩家松手
        crate::dragout::start_drag_out(&paths).map(|_| ())
    })();
    if let Err(e) = outcome {
        installer::emit_event(app, "files:dragError", &Value::String(e));
    }
}

pub fn invoke(
    app: &tauri::AppHandle,
    channel: &str,
    args: &[Value],
) -> Option<Result<Value, String>> {
    match channel {
        "recordings:list" => {
            let target = args.get(1).cloned().unwrap_or(json!({}));
            let force = args.get(2).and_then(|v| v.as_bool()).unwrap_or(false);
            Some(list(
                app,
                args.first().and_then(|v| v.as_str()).unwrap_or("instance"),
                &target,
                force,
            ))
        }
        "recordings:operate" => Some(operate(app, args)),
        "recordings:import" => Some(import(app)),
        "recordings:importAt" => Some(import_at(app, args)),
        "recordings:open" => Some(open(app, args)),
        "recordings:modVersions" => Some(mod_versions(app, args)),
        _ => None,
    }
}

pub fn send(app: &tauri::AppHandle, channel: &str, args: &[Value]) -> Option<Result<(), String>> {
    match channel {
        "recordings:drag" => {
            handle_drag(app, args);
            Some(Ok(()))
        }
        _ => None,
    }
}
