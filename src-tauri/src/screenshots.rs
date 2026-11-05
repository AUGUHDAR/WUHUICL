//! 截图管理子系统（资源管理组「截图」页后端，架构与 recordings.rs 同型）。
//!
//! 通道：screenshots:list / thumbs / operate / import / importAt / open（invoke）、
//! screenshots:drag（send）。
//!
//! 管理口径：截图位于游戏工作目录的 screenshots/，工作目录用启动器权威解析
//! （launch.rs effective_game_dir = installer::instance_directory_state），按版本隔离分类：
//! 隔离开 = versions/<id>/screenshots（每实例一份）；
//! 隔离关 = 游戏目录就是版本文件夹根，截图在 <根>/screenshots，且为该文件夹内
//!          所有未隔离实例共用——只有此时根级目录才成其为一个来源，统一标
//!          「未隔离实例共用」，不虚构一个永远为空的"共享截图目录"。
//!
//! 与录像子系统的差异：
//! 1. 无集中收藏库：共享截图目录本身就是"文件夹级收藏"，操作只有 copy/export/trash。
//! 2. 无压缩包校验：截图是普通图片文件，导入/复制只校验 PNG/JPEG 魔数与文件稳定性。
//! 3. 缩略图落盘缓存（launcher-managed/screenshot-cache/）：列表页与灯箱都经
//!    kamucl-asset 协议读缓存文件，不把原图/大 dataURL 塞进 IPC（参照 centercmd.rs
//!    small_image 的 OOM 教训：24 张原图直传会打爆 IPC 缓冲）。

use crate::mods::assert_instance_idle;
use crate::recordings::{
    active_root, center_target_dir, center_target_dir_state, current_file, drag_path, file_id,
    installed_version_ids, key_of, key_of_str, mtime_ms, real_path, recycle_file, resolve_abs,
    safe_path, stat_of, ScanErr, Source, Stored, COLON, DRAG_LIMIT, SCAN_DIR_LIMIT,
    SCAN_ENTRY_LIMIT,
};
use crate::settings;
use crate::tasks::{self, TaskRecord};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};

/// 缩略图缓存文件数上限；超过后清掉最旧的一半（缩略图随时可再生，删除无损）
const THUMB_CACHE_LIMIT: usize = 6000;
/// 原图读取上限：MC 截图最大不过数 MB，64MB 已远超正常范围
const IMAGE_LIMIT: u64 = 64 * 1024 * 1024;

static CATALOG: LazyLock<Mutex<HashMap<String, Stored>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static SCAN_GENERATION: AtomicU64 = AtomicU64::new(0);

/// 最小化自整理：清空扫描目录。screenshots:list 每次调用都会整体重扫回填，无功能损失。
pub fn release_catalog() {
    if let Ok(mut map) = CATALOG.lock() {
        map.clear();
    }
}

fn image_suffix(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.ends_with(".png") || lower.ends_with(".jpg") || lower.ends_with(".jpeg")
}

fn magic_is_image(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0x89, b'P', b'N', b'G']) || bytes.starts_with(&[0xFF, 0xD8, 0xFF])
}

fn is_image_file(path: &Path) -> Result<(), String> {
    let meta = stat_of(path)?;
    if !meta.is_file() || meta.is_symlink() {
        return Err("不是普通图片文件".into());
    }
    if meta.len() > IMAGE_LIMIT {
        return Err("图片文件过大".into());
    }
    let mut head = [0u8; 8];
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let read = file.read(&mut head).map_err(|e| e.to_string())?;
    if read < 8 || !magic_is_image(&head[..read]) {
        return Err("不是 PNG / JPEG 图片".into());
    }
    Ok(())
}

// ---------------- 扫描（只读各实例 screenshots 目录本身，平铺不递归） ----------------

/// 本功能只是「聚合显示 + 统一管理」：严格只列 screenshots 目录顶层的图片文件，
/// 不进入任何子目录（mods/工具在 screenshots/debug 等子目录写的产物不属于游戏截图，
/// 递归会把这些杂项连同计数一起带偏）。
fn visit_source(
    source: &Source,
    rel: &str,
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
            file_type.is_file(),
            file_type.is_symlink(),
        ));
    }
    for (name, is_file, is_link) in listing {
        // 平铺：目录（debug/ 等子夹）一律不进；链接与点文件跳过
        if is_link || !is_file || name.starts_with('.') || !image_suffix(&name) {
            continue;
        }
        if found.len() >= SCAN_ENTRY_LIMIT {
            return Err(ScanErr::msg("仅显示前 10000 张截图"));
        }
        let full = safe_path(&source.root, &rel, false)?.join(&name);
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
        entry.insert("shared".into(), json!(source.instance_id.is_none()));
        if !found.contains_key(&id) {
            order.push(id.clone());
        }
        found.insert(
            id,
            Stored {
                entry: Value::Object(entry),
                source: source.clone(),
                rel: format!("{rel}/{name}"),
            },
        );
    }
    Ok(())
}

fn scan_inner(
    app: &tauri::AppHandle,
    rec: &TaskRecord,
    task_id: &str,
    task_title: &str,
    scope: &str,
    target: &Value,
) -> Result<Value, String> {
    let target_folder: Option<String> = target["folder"].as_str().map(str::to_string);
    let target_id: Option<String> = target["id"].as_str().map(str::to_string);
    let generation = SCAN_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let root = active_root(app)?;
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
    // scope=instance：只看选定实例的截图目录（未隔离实例 = 文件夹根 screenshots）；
    // scope=folder：聚合文件夹内全部实例各自的截图目录。
    for folder in &registered {
        let root_path = resolve_abs(Path::new(folder["path"].as_str().unwrap_or("")));
        let root_text = root_path.to_string_lossy().into_owned();
        if scope == "instance" || scope == "folder" {
            let active = target_folder.as_deref().unwrap_or("");
            if !crate::installer::same_path(&root_text, active) {
                continue;
            }
        }
        let (ids, errors) = installed_version_ids(&root_path);
        for error in errors {
            warnings.push(format!("{root_text}{COLON}{error}"));
        }
        let mut shared_needed = false;
        for id in ids {
            if scope == "instance" && Some(id.as_str()) != target_id.as_deref() {
                continue;
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
                        // 实例视图：选定实例未隔离，文件夹根 screenshots 就是它的截图目录
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
                label: "未隔离实例共用".into(),
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
        if let Err(e) = visit_source(source, "screenshots", &mut visited, &mut found, &mut order) {
            if !e.not_found {
                warnings.push(format!("{}{COLON}{}", source.label, e.message));
            }
        }
    }
    let total = order.len();
    for read in 1..=total {
        tasks::wait_while_paused(rec);
        if rec.cancelled.load(Ordering::SeqCst) {
            return Err("已取消".into());
        }
        tasks::emit_progress(
            app,
            json!({
                "taskId": task_id,
                "taskTitle": task_title,
                "stage": "world",
                "progress": (read as f64 / total as f64).min(1.0),
                "text": format!("读取截图 {read}/{total}")
            }),
        );
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
        "folderRoot": root.to_string_lossy(),
        "instances": instances
    }))
}

fn list(app: &tauri::AppHandle, scope: &str, target: &Value) -> Result<Value, String> {
    let rec = tasks::register_task("截图 · 扫描截图目录", "world");
    let task_id = rec.id.clone();
    let task_title = rec.title.clone();
    let outcome = scan_inner(app, &rec, &task_id, &task_title, scope, target);
    let cancelled = rec.cancelled.load(Ordering::SeqCst);
    let error = outcome.as_ref().err().cloned();
    tasks::task_done(app, &task_id, outcome.is_ok(), error, cancelled, None);
    outcome
}

// ---------------- 选中项解析（同 recordings.rs selected_items 的截图口径） ----------------

fn selected_items(app: &tauri::AppHandle, ids: &Value) -> Result<Vec<Stored>, String> {
    let arr = ids.as_array().ok_or("请选择截图".to_string())?;
    if arr.is_empty() || arr.len() > SCAN_ENTRY_LIMIT {
        return Err("请选择截图".into());
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
            None => return Err("截图所属文件夹已解除绑定，请刷新列表".into()),
        }
    }
    Ok(out)
}

// ---------------- 缩略图（落盘缓存 + kamucl-asset 协议展示） ----------------

fn fnv64_pair(s: &str) -> (u64, u64) {
    let mut h1: u64 = 0xcbf29ce484222325;
    let mut h2: u64 = 0x9e3779b97f4a7c15;
    for b in s.as_bytes() {
        h1 = (h1 ^ *b as u64).wrapping_mul(0x100000001b3);
        h2 = (h2 ^ *b as u64).wrapping_mul(0x100000001b3).rotate_left(7);
    }
    (h1, h2)
}

fn thumb_cache_dir(app: &tauri::AppHandle) -> PathBuf {
    let dir = settings::user_data_dir(app)
        .join("launcher-managed")
        .join("screenshot-cache");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// 缓存超限时清理最旧的一半（缩略图随时可重建，删除无损）
fn prune_thumb_cache(dir: &Path) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut rows: Vec<(std::time::SystemTime, PathBuf)> = rd
        .flatten()
        .filter(|e| e.path().extension().map(|x| x == "jpg").unwrap_or(false))
        .filter_map(|e| {
            let modified = e.metadata().ok()?.modified().ok()?;
            Some((modified, e.path()))
        })
        .collect();
    if rows.len() < THUMB_CACHE_LIMIT {
        return;
    }
    rows.sort_by_key(|(t, _)| *t);
    let drop_to = rows.len() / 2;
    for (_, path) in rows.iter().take(drop_to) {
        let _ = std::fs::remove_file(path);
    }
}

/// 生成或复用单张缩略图，返回缓存文件路径（经 managedImageUrl 在渲染层展示）
pub(crate) fn thumb_of(app: &tauri::AppHandle, source: &Path, width: u32) -> Result<PathBuf, String> {
    let meta = stat_of(source)?;
    if !meta.is_file() || meta.is_symlink() || meta.len() > IMAGE_LIMIT {
        return Err("不是普通图片文件".into());
    }
    let (h1, h2) = fnv64_pair(&format!(
        "{}\u{0}{}\u{0}{}\u{0}{width}",
        source.to_string_lossy(),
        mtime_ms(&meta) as u64,
        meta.len()
    ));
    let dir = thumb_cache_dir(app);
    let cached = dir.join(format!("{h1:016x}{h2:016x}.jpg"));
    if cached.is_file() {
        return Ok(cached);
    }
    let bytes = std::fs::read(source).map_err(|e| e.to_string())?;
    if !magic_is_image(&bytes) {
        return Err("不是 PNG / JPEG 图片".into());
    }
    let decoded = image::load_from_memory(&bytes).map_err(|_| "图片解码失败".to_string())?;
    let thumb = decoded.thumbnail(width, width * 10);
    let rgb = thumb.to_rgb8();
    let staged = dir.join(format!(".{}.part", h1));
    {
        let file = std::fs::File::create(&staged).map_err(|e| e.to_string())?;
        let mut writer = std::io::BufWriter::new(file);
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut writer, 85);
        rgb.write_with_encoder(encoder)
            .map_err(|_| "缩略图编码失败".to_string())?;
        writer.flush().map_err(|e| e.to_string())?;
    }
    // 同 key 并发生成：已有人写好就直接复用，避免覆盖半截文件
    match std::fs::rename(&staged, &cached) {
        Ok(()) => {}
        Err(_) if cached.is_file() => {
            let _ = std::fs::remove_file(&staged);
        }
        Err(e) => {
            let _ = std::fs::remove_file(&staged);
            return Err(e.to_string());
        }
    }
    prune_thumb_cache(&dir);
    Ok(cached)
}

fn thumbs(app: &tauri::AppHandle, ids: &Value, width: u32) -> Result<Value, String> {
    let items = selected_items(app, ids)?;
    let mut map = Map::new();
    for item in &items {
        let id = item.entry["id"].as_str().unwrap_or("").to_string();
        let file = match current_file(item) {
            Ok(file) => file,
            Err(_) => continue,
        };
        match thumb_of(app, &file, width) {
            Ok(path) => {
                map.insert(id.clone(), json!(path.to_string_lossy().into_owned()));
            }
            Err(e) => {
                map.insert(id.clone(), json!({ "error": e }));
            }
        }
    }
    Ok(json!({ "thumbs": map }))
}

// ---------------- 复制 / 提取 / 回收站 ----------------

fn action_title(action: &str) -> &'static str {
    match action {
        "copy" => "复制到实例",
        "export" => "提取",
        _ => "移入回收站",
    }
}

/// 截图落位目录 = 实例游戏工作目录（权威解析）/screenshots：
/// 隔离实例落 versions/<id>/screenshots，未隔离实例落版本文件夹根 screenshots
fn destination_dir(app: &tauri::AppHandle, target: &Value) -> Result<PathBuf, String> {
    if !target.get("id").map(|v| v.is_string()).unwrap_or(false)
        || target["id"].as_str().unwrap_or("").is_empty()
    {
        return Err("请先选择游戏实例".into());
    }
    let dir = center_target_dir(app, target)?;
    assert_instance_idle(app, &dir)?;
    let out = dir.join("screenshots");
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    Ok(out)
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

/// 流式复制到 destination，同名自动加序号；复制前后校验源文件未被游戏改写
fn copy_screenshot(
    source: &Path,
    destination: &Path,
    rec: &TaskRecord,
    on_bytes: &mut dyn FnMut(u64),
) -> Result<PathBuf, String> {
    let before = stat_of(source)?;
    if !before.is_file() || before.is_symlink() {
        return Err("截图不是普通文件".into());
    }
    let root = real_path(destination)?;
    if stat_of(destination)?.is_symlink() {
        return Err("目标目录不能是链接".into());
    }
    let stage = root.join(format!(".wuhuicl-shot-{}.part", crate::accounts::uuid_v4()));
    let mut reader = std::fs::File::open(source).map_err(|e| e.to_string())?;
    let mut writer = std::fs::File::create_new(&stage).map_err(|e| e.to_string())?;
    let mut transferred = 0u64;
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        tasks::wait_while_paused(rec);
        if rec.cancelled.load(Ordering::SeqCst) {
            let _ = std::fs::remove_file(&stage);
            return Err("已取消".into());
        }
        let read = reader.read(&mut buffer).map_err(|e| e.to_string())?;
        if read == 0 {
            break;
        }
        writer
            .write_all(&buffer[..read])
            .map_err(|e| e.to_string())?;
        transferred += read as u64;
        on_bytes(transferred);
    }
    writer.flush().map_err(|e| e.to_string())?;
    drop(writer);
    let after = stat_of(source)?;
    let changed = after.is_symlink()
        || file_id(&after) != file_id(&before)
        || after.len() != before.len()
        || mtime_ms(&after) != mtime_ms(&before);
    if changed {
        let _ = std::fs::remove_file(&stage);
        return Err("截图在复制期间发生变化，请停止游戏后重试".into());
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
        let dest = root.join(&name);
        if resolve_abs(&dest) == source_abs {
            let _ = std::fs::remove_file(&stage);
            return Ok(source.to_path_buf());
        }
        match std::fs::rename(&stage, &dest) {
            Ok(()) => return Ok(dest),
            Err(_) if dest.exists() => continue,
            Err(e) => {
                let _ = std::fs::remove_file(&stage);
                return Err(e.to_string());
            }
        }
    }
    let _ = std::fs::remove_file(&stage);
    Err("同名截图过多，请整理目标目录".into())
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
    emit_run_progress(
        app, task_id, task_title, total, base, 0, processed, count, &name,
    );
    let file = current_file(item)?;
    if action == "trash" {
        let inner = item.clone();
        // 实例目录正在被游戏使用时不删（游戏可能正在写截图）；共享目录不做进程判定，
        // 靠 recycle 前的稳定性校验兜底
        let instance_root = inner.source.instance_id.as_ref().map(|_| {
            inner
                .source
                .root
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| inner.source.root.clone())
        });
        crate::download::with_file_job(&inner.source.root, Some(rec), || -> Result<(), String> {
            if let Some(dir) = &instance_root {
                assert_instance_idle(app, dir)?;
            }
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
    let dir = destination
        .ok_or("未指定截图目标目录".to_string())?
        .to_path_buf();
    let mut output: Option<PathBuf> = None;
    let mut on_bytes = {
        let (name, app_ref, tid, title) = (name.clone(), app, task_id, task_title);
        move |bytes: u64| {
            emit_run_progress(
                app_ref, tid, title, total, base, bytes, processed, count, &name,
            )
        }
    };
    {
        let file_for_copy = file.clone();
        let dir_for_copy = dir.clone();
        let rec_ref: Option<&TaskRecord> = Some(rec);
        crate::download::with_file_job(&dir_for_copy, rec_ref, || -> Result<(), String> {
            let copied = copy_screenshot(&file_for_copy, &dir_for_copy, rec, &mut on_bytes)?;
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
    let rec = tasks::register_task(&format!("截图 · {}", action_title(action)), "world");
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
                    "任务已取消，原截图保留".to_string()
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
        Some(format!("{failed} 张截图处理失败，请查看截图页结果"))
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
    let valid_action = matches!(action.as_str(), "copy" | "export" | "trash");
    let id_len = ids.as_array().map(|a| a.len()).unwrap_or(0);
    if !request.is_object()
        || !valid_action
        || !ids.is_array()
        || id_len < 1
        || id_len > SCAN_ENTRY_LIMIT
    {
        return Err("截图操作参数无效".into());
    }
    let items = selected_items(app, &ids)?;
    let mut destination: Option<PathBuf> = None;
    match action.as_str() {
        "copy" => {
            let target = request
                .get("target")
                .filter(|v| v.is_object())
                .ok_or("请选择目标实例".to_string())?;
            destination = Some(destination_dir(app, target)?);
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
                    .set_title("选择截图提取文件夹")
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

// ---------------- 导入（选择文件 / 拖拽，同一落位逻辑） ----------------

fn import_paths(
    app: &tauri::AppHandle,
    paths: Vec<PathBuf>,
    target: &Value,
) -> Result<Value, String> {
    let destination = destination_dir(app, target)?;
    let mut items: Vec<Stored> = Vec::new();
    for file in paths {
        let meta = stat_of(&file)?;
        is_image_file(&file)?;
        let rel = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let parent = file.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        let mut entry = Map::new();
        entry.insert("id".into(), json!(file.to_string_lossy().into_owned()));
        entry.insert("name".into(), json!(rel.clone()));
        entry.insert("size".into(), json!(meta.len()));
        entry.insert("modified".into(), json!(mtime_ms(&meta)));
        entry.insert("source".into(), json!("导入"));
        entry.insert(
            "directory".into(),
            json!(parent.to_string_lossy().into_owned()),
        );
        entry.insert("shared".into(), json!(false));
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
    if items.is_empty() {
        return Err("请选择 PNG / JPEG 图片".into());
    }
    run(app, items, "copy", Some(&destination))
}

fn import(app: &tauri::AppHandle, args: &[Value]) -> Result<Value, String> {
    let target = args.first().cloned().unwrap_or(Value::Null);
    use tauri_plugin_dialog::DialogExt;
    let picked = app
        .dialog()
        .file()
        .set_title("导入截图")
        .add_filter("PNG / JPEG 图片", &["png", "jpg", "jpeg"])
        .blocking_pick_files();
    let Some(picked) = picked else {
        return Ok(Value::Null);
    };
    let paths = picked
        .into_iter()
        .map(|raw| raw.into_path())
        .collect::<Result<Vec<PathBuf>, _>>()
        .map_err(|e| e.to_string())?;
    import_paths(app, paths, &target)
}

fn import_at(app: &tauri::AppHandle, args: &[Value]) -> Result<Value, String> {
    let files = args
        .first()
        .and_then(|v| v.as_array())
        .ok_or("请拖入 PNG / JPEG 图片".to_string())?
        .iter()
        .filter_map(|v| v.as_str())
        .map(PathBuf::from)
        .collect::<Vec<PathBuf>>();
    if files.is_empty() || files.len() > SCAN_ENTRY_LIMIT {
        return Err("请拖入 PNG / JPEG 图片".into());
    }
    let target = args.get(1).cloned().unwrap_or(Value::Null);
    import_paths(app, files, &target)
}

// ---------------- 打开 / 定位 / 拖出 ----------------

/// args: [target, id?]——带 id = 在资源管理器中定位该截图；否则打开目标实例的 screenshots 目录
fn open(app: &tauri::AppHandle, args: &[Value]) -> Result<Value, String> {
    let target = args.first().cloned().unwrap_or(Value::Null);
    let id = args
        .get(1)
        .and_then(|v| v.as_str())
        .filter(|v| !v.is_empty())
        .map(str::to_string);
    match id {
        Some(id) => {
            let item = selected_items(app, &json!([id]))?
                .into_iter()
                .next()
                .ok_or("请选择截图".to_string())?;
            let file = current_file(&item)?;
            crate::platform::show_in_folder(&file.to_string_lossy())?;
        }
        None => {
            let dir = destination_dir(app, &target)?;
            crate::platform::open_path(&dir.to_string_lossy())?;
        }
    }
    Ok(Value::Null)
}

fn handle_drag(app: &tauri::AppHandle, args: &[Value]) {
    let outcome: Result<(), String> = (|| {
        let items = selected_items(app, args.first().unwrap_or(&Value::Null))?;
        if items.len() > DRAG_LIMIT {
            return Err(format!("每次最多拖出 {DRAG_LIMIT} 张截图，请分批选择"));
        }
        let mut paths: Vec<String> = Vec::with_capacity(items.len());
        for item in &items {
            let file = drag_path(&item.source.root, &item.rel)?;
            let meta = stat_of(&file)?;
            let size = item.entry["size"].as_u64().unwrap_or(0);
            let modified = item.entry["modified"].as_f64().unwrap_or(0.0);
            if !meta.is_file() || meta.len() != size || mtime_ms(&meta) != modified {
                return Err("截图已变化，请刷新后重试".into());
            }
            paths.push(file.to_string_lossy().to_string());
        }
        crate::dragout::validate_paths(&paths)?;
        // DoDragDrop 模态循环：阻塞到玩家松手
        crate::dragout::start_drag_out(&paths).map(|_| ())
    })();
    if let Err(e) = outcome {
        crate::installer::emit_event(app, "files:dragError", &Value::String(e));
    }
}

pub fn invoke(
    app: &tauri::AppHandle,
    channel: &str,
    args: &[Value],
) -> Option<Result<Value, String>> {
    match channel {
        "screenshots:list" => {
            let target = args.get(1).cloned().unwrap_or(json!({}));
            Some(list(
                app,
                args.first().and_then(|v| v.as_str()).unwrap_or("instance"),
                &target,
            ))
        }
        "screenshots:thumbs" => Some(thumbs(
            app,
            args.first().unwrap_or(&Value::Null),
            args.get(1)
                .and_then(|v| v.as_u64())
                .map(|w| w.clamp(64, 4096) as u32)
                .unwrap_or(480),
        )),
        "screenshots:operate" => Some(operate(app, args)),
        "screenshots:import" => Some(import(app, args)),
        "screenshots:importAt" => Some(import_at(app, args)),
        "screenshots:open" => Some(open(app, args)),
        _ => None,
    }
}

pub fn send(app: &tauri::AppHandle, channel: &str, args: &[Value]) -> Option<Result<(), String>> {
    match channel {
        "screenshots:drag" => {
            handle_drag(app, args);
            Some(Ok(()))
        }
        _ => None,
    }
}
