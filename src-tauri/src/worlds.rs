//! 世界存档探测与导入（对应原版 core/worlds.ts 1:1）。
//! world:probe / world:import；NBT 解析走 crate::nbt。

use crate::nbt::{self, NbtValue};
use crate::tasks::TaskRecord;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const MAX_LEVEL_DAT: u64 = 32 * 1024 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 250_000;
const MAX_ARCHIVE_BYTES: u64 = 32 * 1024 * 1024 * 1024;
const MAX_COMPRESSION_RATIO: f64 = 500.0;
const MAX_FOLDER_DEPTH: u32 = 10;

/// Windows 设备名 / 非法字符（normalizeWorldArchivePath）
fn is_windows_device(part: &str) -> bool {
    let lower = part.to_lowercase();
    let stem = lower.split('.').next().unwrap_or("");
    ["con", "prn", "aux", "nul"].contains(&stem)
        || (stem.len() == 4
            && stem.starts_with("com")
            && stem[3..].chars().all(|c| c.is_ascii_digit())
            && stem[3..].parse::<u8>().map(|d| (1..=9).contains(&d)).unwrap_or(false))
        || (stem.len() == 4
            && stem.starts_with("lpt")
            && stem[3..].chars().all(|c| c.is_ascii_digit())
            && stem[3..].parse::<u8>().map(|d| (1..=9).contains(&d)).unwrap_or(false))
}

fn normalize_archive_path(input: &str) -> Result<String, String> {
    if input.is_empty() || input.contains('\0') {
        return Err("压缩包包含空文件名或 NUL 字符".into());
    }
    let slash = input.replace('\\', "/");
    if slash.starts_with('/') || slash.starts_with("//") || (slash.as_bytes().get(1) == Some(&b':')) {
        return Err(format!("压缩包包含绝对路径：{input}"));
    }
    let parts: Vec<&str> = slash.split('/').filter(|p| !p.is_empty() && *p != ".").collect();
    if parts.is_empty() {
        return Ok(String::new());
    }
    if parts.iter().any(|p| *p == "..") {
        return Err(format!("压缩包包含路径穿越：{input}"));
    }
    if parts.iter().any(|p| is_windows_device(p) || p.chars().any(|c| ":*?\"<>|".contains(c))) {
        return Err(format!("压缩包包含 Windows 非法路径：{input}"));
    }
    Ok(parts.join("/"))
}

// ---------------- level.dat 元数据 ----------------

fn approximate_version(data_version: i64) -> Option<&'static str> {
    const RANGES: [(i64, &str); 20] = [
        (4189, "约 1.21.4 或更新"), (3953, "约 1.21–1.21.3"), (3837, "约 1.20.5–1.20.6"),
        (3700, "约 1.20.3–1.20.4"), (3578, "约 1.20.2"), (3463, "约 1.20–1.20.1"),
        (3337, "约 1.19.4"), (3218, "约 1.19.3"), (3105, "约 1.19–1.19.2"),
        (2975, "约 1.18.2"), (2860, "约 1.18–1.18.1"), (2724, "约 1.17–1.17.1"),
        (2566, "约 1.16–1.16.5"), (2225, "约 1.15–1.15.2"), (1901, "约 1.14–1.14.4"),
        (1519, "约 1.13–1.13.2"), (1343, "约 1.12–1.12.2"), (819, "约 1.11–1.11.2"),
        (510, "约 1.10–1.10.2"), (169, "约 1.9–1.9.4"),
    ];
    RANGES.iter().find(|(m, _)| data_version >= *m).map(|(_, s)| *s)
}

/// metadataFromLevelDat：level.dat → WorldCandidateInfo 核心字段
#[allow(clippy::type_complexity)]
fn metadata_from_level_dat(
    buffer: &[u8],
    fallback_name: &str,
) -> Result<(Value, usize), String> {
    let root = nbt::parse_nbt(buffer)?;
    let data = nbt::get(&root, "Data").and_then(nbt::as_compound).unwrap_or(&root);
    let version = nbt::get(data, "Version").and_then(nbt::as_compound);
    let data_version = nbt::get(data, "DataVersion")
        .and_then(nbt::as_int)
        .or_else(|| version.and_then(|v| nbt::get(v, "Id")).and_then(nbt::as_int));
    let exact_version = version
        .and_then(|v| nbt::get(v, "Name"))
        .and_then(nbt::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let declared_datapacks = nbt::get(data, "DataPacks")
        .and_then(nbt::as_compound)
        .and_then(|dp| nbt::get(dp, "Enabled"))
        .and_then(|e| match e {
            NbtValue::List(l) => Some(l.values.iter().filter_map(nbt::as_str).filter(|n| *n != "vanilla" && *n != "minecraft").count()),
            _ => None,
        })
        .unwrap_or(0);
    let game_modes = ["生存", "创造", "冒险", "旁观"];
    let game_type = nbt::get(data, "GameType").and_then(nbt::as_int);
    let keys: Vec<String> = data.iter().map(|(k, _)| k.to_lowercase()).collect();
    let mut mod_evidence: Vec<String> = Vec::new();
    let mut loader = None;
    if keys.iter().any(|k| k == "fml" || k.contains("forge")) {
        mod_evidence.push("level.dat 含 Forge/FML 元数据".into());
        loader = Some("forge");
    }
    if keys.iter().any(|k| k.contains("fabric")) {
        mod_evidence.push("level.dat 含 Fabric 元数据".into());
        loader = Some("fabric");
    }
    if keys.iter().any(|k| k.contains("bukkit")) {
        mod_evidence.push("level.dat 含 Bukkit 元数据".into());
    }
    let world_name = nbt::get(data, "LevelName")
        .and_then(nbt::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| fallback_name.to_string());
    let version_confidence = if exact_version.is_some() {
        "exact"
    } else if data_version.is_none() {
        "unknown"
    } else {
        "approximate"
    };
    let minecraft_version = exact_version.or_else(|| data_version.and_then(approximate_version).map(str::to_string));
    let candidate = json!({
        "worldName": world_name,
        "dataVersion": data_version,
        "minecraftVersion": minecraft_version,
        "versionConfidence": version_confidence,
        "gameMode": game_type.filter(|g| (*g as usize) < game_modes.len()).map(|g| game_modes[g as usize]),
        "hardcore": nbt::get(data, "hardcore").and_then(nbt::as_int) == Some(1),
        "modEvidence": mod_evidence,
        "loader": loader,
        "loaderConfidence": loader.map(|_| "inferred"),
        "declaredDatapacks": declared_datapacks
    });
    Ok((candidate, declared_datapacks))
}

// ---------------- ZIP 巡检 ----------------

struct ZipEntryInfo {
    name: String,
    directory: bool,
    uncompressed_size: u64,
    compressed_size: u64,
    symlink: bool,
}

fn zip_entry_is_symlink(file: &zip::read::ZipFile) -> bool {
    #[cfg(unix)]
    {
        file.unix_mode().map(|m| (m >> 16) & 0o170_000 == 0o120_000).unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        let _ = file;
        false
    }
}

fn inspect_zip(path: &Path) -> Result<(Vec<ZipEntryInfo>, Vec<String>), String> {
    let file = std::fs::File::open(path).map_err(|e| format!("ZIP 无法打开：{e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("ZIP 无法打开：{e}"))?;
    let mut entries = Vec::new();
    let mut warnings = Vec::new();
    let mut total_uncompressed: u64 = 0;
    let mut total_compressed: u64 = 0;
    for i in 0..archive.len() {
        let entry = archive.by_index_raw(i).map_err(|e| e.to_string())?;
        if entries.len() >= MAX_ARCHIVE_ENTRIES {
            return Err("压缩包文件数量超过安全上限".into());
        }
        let name = normalize_archive_path(entry.name())?;
        let symlink = {
            let mode = (entry.unix_mode().unwrap_or(0) >> 16) & 0o170_000;
            mode == 0o120_000
        };
        if symlink {
            warnings.push(format!("已忽略符号链接：{name}"));
        }
        total_uncompressed += entry.size();
        total_compressed += entry.compressed_size();
        if total_uncompressed > MAX_ARCHIVE_BYTES {
            return Err("压缩包解压后超过 32 GB 安全上限".into());
        }
        if entry.size() > 64 * 1024 * 1024
            && entry.size() as f64 / std::cmp::max(1, entry.compressed_size()) as f64 > MAX_COMPRESSION_RATIO
        {
            return Err(format!("压缩比异常，疑似解压炸弹：{name}"));
        }
        entries.push(ZipEntryInfo {
            name,
            directory: entry.is_dir(),
            uncompressed_size: entry.size(),
            compressed_size: entry.compressed_size(),
            symlink,
        });
    }
    if total_uncompressed > 64 * 1024 * 1024
        && total_uncompressed as f64 / std::cmp::max(1, total_compressed) as f64 > MAX_COMPRESSION_RATIO
    {
        return Err("压缩包整体压缩比异常，疑似解压炸弹".into());
    }
    Ok((entries, warnings))
}

fn read_zip_entry(path: &Path, wanted: &str, limit: u64) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = normalize_archive_path(entry.name())?;
        if name != wanted {
            continue;
        }
        if entry.size() > limit {
            return Err(format!(
                "{} 超过安全大小限制",
                Path::new(wanted).file_name().map(|n| n.to_string_lossy()).unwrap_or_default()
            ));
        }
        let mut out = Vec::with_capacity(entry.size() as usize);
        std::io::Read::read_to_end(&mut entry, &mut out).map_err(|e| format!("ZIP 条目解压失败：{e}"))?;
        return Ok(out);
    }
    Err(format!("ZIP 中找不到 {wanted}"))
}

fn relative_in_root(name: &str, root: &str) -> Option<String> {
    if root.is_empty() {
        return Some(name.to_string());
    }
    if name == root {
        return Some(String::new());
    }
    name.strip_prefix(&format!("{root}/")).map(str::to_string)
}

// ---------------- probe ----------------

fn posix_dir(name: &str) -> String {
    match name.rfind('/') {
        Some(i) => name[..i].to_string(),
        None => String::new(),
    }
}

fn archive_resource_packs(entries: &[ZipEntryInfo], world_root: &str) -> Value {
    if world_root.is_empty() {
        return json!([]);
    }
    let files: std::collections::HashSet<String> = entries
        .iter()
        .filter(|e| !e.directory && !e.symlink)
        .map(|e| e.name.clone())
        .collect();
    let mut roots_with_assets = std::collections::HashSet::new();
    for f in &files {
        let lower = f.to_lowercase();
        if let Some(marker) = lower.find("/assets/") {
            if marker > 0 {
                roots_with_assets.insert(f[..marker].to_string());
            }
        }
    }
    let mut result = Vec::new();
    for f in &files {
        if Path::new(f).file_name().map(|n| n.to_string_lossy().to_lowercase()) != Some("pack.mcmeta".into()) {
            continue;
        }
        let root = posix_dir(f);
        if root.is_empty() {
            continue;
        }
        let in_world = relative_in_root(&root, world_root);
        if in_world.is_some() && root.to_lowercase().contains("datapacks") {
            continue;
        }
        if in_world.is_none() && !root.is_empty() {
            // 原版：root 不在世界根内时（datapacks 内置包）跳过
        }
        if !roots_with_assets.contains(&root) {
            continue;
        }
        let name = Path::new(&root)
            .file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_default()
            .to_string();
        result.push(json!({ "id": root, "name": if name.is_empty() { "资源包".to_string() } else { name } }));
        if result.len() >= 128 {
            break;
        }
    }
    Value::Array(result)
}

fn zip_world_info(path: &Path) -> Result<Value, String> {
    let (entries, warnings) = inspect_zip(path)?;
    let mut level_entries: Vec<&ZipEntryInfo> = entries
        .iter()
        .filter(|e| {
            !e.directory
                && !e.symlink
                && Path::new(&e.name)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_lowercase() == "level.dat")
                    .unwrap_or(false)
        })
        .collect();
    level_entries.sort_by_key(|e| e.name.split('/').count());
    if level_entries.is_empty() {
        return Ok(Value::Null);
    }
    let mut candidates = Vec::new();
    let mut parse_errors: Vec<String> = Vec::new();
    let fallback = path
        .file_stem()
        .map(|s| s.to_string_lossy())
        .unwrap_or_default();
    for level in level_entries.iter().take(32) {
        let root = posix_dir(&level.name);
        match (|| -> Result<Value, String> {
            let buf = read_zip_entry(path, &level.name, MAX_LEVEL_DAT)?;
            let (mut metadata, declared) = metadata_from_level_dat(&buf, &Path::new(&root).file_name().map(|n| n.to_string_lossy()).unwrap_or_default().trim(),)?;
            let members: Vec<&ZipEntryInfo> = entries
                .iter()
                .filter(|e| !e.directory && relative_in_root(&e.name, &root).is_some())
                .collect();
            let mut datapacks = std::collections::HashSet::new();
            let mut has_world_resource_pack = false;
            let mut has_server_config = false;
            for member in &members {
                let rel = relative_in_root(&member.name, &root).unwrap_or_default();
                if rel.eq_ignore_ascii_case("resources.zip") {
                    has_world_resource_pack = true;
                }
                if let Some(rest) = rel.strip_prefix("datapacks/").or_else(|| rel.strip_prefix("DATAPACKS/")) {
                    if let Some(dp) = rest.split('/').next() {
                        if !dp.is_empty() {
                            datapacks.insert(dp.to_string());
                        }
                    }
                }
                let lower = rel.to_lowercase();
                if lower.starts_with("serverconfig/") || lower.starts_with("config/") {
                    has_server_config = true;
                }
            }
            if has_server_config && !metadata["modEvidence"].as_array().map(|a| a.iter().any(|v| v == "目录中存在模组配置痕迹")).unwrap_or(false) {
                if let Some(arr) = metadata["modEvidence"].as_array_mut() {
                    let mut a = arr.clone();
                    a.push(json!("目录中存在模组配置痕迹"));
                    metadata["modEvidence"] = Value::Array(a);
                }
            }
            let total_bytes: u64 = members.iter().map(|e| e.uncompressed_size).sum();
            metadata["id"] = json!(if root.is_empty() { ".".to_string() } else { root.clone() });
            metadata["datapackCount"] = json!(declared.max(datapacks.len()));
            metadata["resourcePacks"] = archive_resource_packs(&entries, &root);
            metadata["hasWorldResourcePack"] = json!(has_world_resource_pack);
            metadata["fileCount"] = json!(members.len());
            metadata["totalBytes"] = json!(total_bytes);
            Ok(metadata)
        })() {
            Ok(c) => candidates.push(c),
            Err(e) => parse_errors.push(format!("{}：{}", level.name, e)),
        }
    }
    if candidates.is_empty() {
        return Err(format!("检测到 level.dat，但均无法解析：{}", parse_errors.join("；")));
    }
    Ok(json!({
        "sourcePath": path.to_string_lossy(),
        "sourceType": "zip",
        "candidates": candidates,
        "warnings": warnings
    }))
}

fn find_folder_candidates(root: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let mut queue = vec![(root.to_path_buf(), 0u32)];
    let mut candidates = Vec::new();
    let mut visited = 0usize;
    while let Some((dir, depth)) = queue.pop() {
        let entries = std::fs::read_dir(&dir).map_err(|e| e.to_string())?;
        for entry in entries.flatten() {
            visited += 1;
            if visited > MAX_ARCHIVE_ENTRIES {
                return Err("目录文件数量超过安全扫描上限".into());
            }
        }
        let mut has_level = false;
        for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())?.flatten() {
            let is_file = entry.file_type().map(|t| t.is_file()).unwrap_or(false);
            if is_file && entry.file_name().to_string_lossy().to_lowercase() == "level.dat" {
                has_level = true;
                break;
            }
        }
        if has_level {
            let rel = dir
                .strip_prefix(root)
                .map(|r| r.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            candidates.push((if rel.is_empty() { ".".to_string() } else { rel }, dir));
            continue;
        }
        if depth >= MAX_FOLDER_DEPTH {
            continue;
        }
        for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())?.flatten() {
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            let is_symlink = entry.file_type().map(|t| t.is_symlink()).unwrap_or(false);
            if is_dir && !is_symlink {
                queue.push((entry.path(), depth + 1));
            }
        }
    }
    Ok(candidates)
}

fn inspect_folder_world(root: &Path) -> Result<(u64, u64, usize, bool, Vec<String>), String> {
    let mut queue = vec![root.to_path_buf()];
    let mut file_count: u64 = 0;
    let mut total_bytes: u64 = 0;
    let mut datapacks = std::collections::HashSet::new();
    let mut has_world_resource_pack = false;
    let mut has_server_config = false;
    while let Some(dir) = queue.pop() {
        for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())?.flatten() {
            let Ok(meta) = entry.metadata() else { continue };
            if entry.file_type().map(|t| t.is_symlink()).unwrap_or(false) {
                continue;
            }
            if meta.is_dir() {
                queue.push(entry.path());
                continue;
            }
            file_count += 1;
            total_bytes += meta.len();
            if file_count > MAX_ARCHIVE_ENTRIES as u64 || total_bytes > MAX_ARCHIVE_BYTES {
                return Err("存档目录超过安全导入上限".into());
            }
            let rel = entry
                .path()
                .strip_prefix(root)
                .map(|r| r.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            if rel.eq_ignore_ascii_case("resources.zip") {
                has_world_resource_pack = true;
            }
            if let Some(rest) = rel.strip_prefix("datapacks/") {
                if let Some(dp) = rest.split('/').next() {
                    if !dp.is_empty() {
                        datapacks.insert(dp.to_string());
                    }
                }
            }
            let lower = rel.to_lowercase();
            if lower.starts_with("serverconfig/") || lower.starts_with("config/") {
                has_server_config = true;
            }
        }
    }
    let mod_evidence = if has_server_config { vec!["目录中存在模组配置痕迹".to_string()] } else { Vec::new() };
    Ok((file_count, total_bytes, datapacks.len(), has_world_resource_pack, mod_evidence))
}

fn folder_world_info(folder: &Path) -> Result<Value, String> {
    let found = find_folder_candidates(folder)?;
    if found.is_empty() {
        return Ok(Value::Null);
    }
    let mut candidates = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    for (id, root) in found.iter().take(32) {
        let level_path = root.join("level.dat");
        let Ok(stat) = std::fs::metadata(&level_path) else { continue };
        if stat.len() > MAX_LEVEL_DAT {
            warnings.push(format!("{id}：level.dat 超过安全大小限制"));
            continue;
        }
        let name = root.file_name().map(|n| n.to_string_lossy()).unwrap_or_default().to_string();
        let buf = match std::fs::read(&level_path) {
            Ok(b) => b,
            Err(e) => {
                warnings.push(format!("{id}：{e}"));
                continue;
            }
        };
        match metadata_from_level_dat(&buf, &name) {
            Ok((mut metadata, declared)) => {
                match inspect_folder_world(root) {
                    Ok((file_count, total_bytes, dp_count, has_rp, mut evidence)) => {
                        let existing = metadata["modEvidence"].as_array().cloned().unwrap_or_default();
                        for ev in &existing {
                            let s = ev.as_str().unwrap_or_default().to_string();
                            if !evidence.contains(&s) {
                                evidence.push(s);
                            }
                        }
                        metadata["modEvidence"] = json!(evidence);
                        metadata["id"] = json!(id);
                        metadata["datapackCount"] = json!(declared.max(dp_count));
                        metadata["resourcePacks"] = json!([]);
                        metadata["hasWorldResourcePack"] = json!(has_rp);
                        metadata["fileCount"] = json!(file_count);
                        metadata["totalBytes"] = json!(total_bytes);
                        candidates.push(metadata);
                    }
                    Err(e) => warnings.push(format!("{id}：{e}")),
                }
            }
            Err(e) => warnings.push(format!("{id}：{e}")),
        }
    }
    if candidates.is_empty() {
        return Err(format!("检测到 level.dat，但均无法解析：{}", warnings.join("；")));
    }
    Ok(json!({
        "sourcePath": folder.to_string_lossy(),
        "sourceType": "folder",
        "candidates": candidates,
        "warnings": warnings
    }))
}

/// world:probe → WorldImportInfo | null；返回 null 表示该路径不是存档
pub fn probe_world(input: &str) -> Result<Value, String> {
    if input.is_empty() {
        return Ok(Value::Null);
    }
    let path = PathBuf::from(input);
    if !path.exists() {
        return Ok(Value::Null);
    }
    let meta = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
    if meta.is_symlink() {
        return Err("不允许从符号链接导入存档".into());
    }
    if meta.is_dir() {
        return folder_world_info(&path);
    }
    if meta.is_file() && path.extension().map(|e| e.to_string_lossy().to_lowercase()) == Some("zip".into()) {
        return zip_world_info(&path);
    }
    Ok(Value::Null)
}

// ---------------- import ----------------

fn clean_leaf_name(input: &str, label: &str) -> Result<String, String> {
    let value = input.trim();
    if value.is_empty() {
        return Err(format!("{label}不能为空"));
    }
    if value.chars().count() > 120 {
        return Err(format!("{label}最多 120 个字符"));
    }
    if value == "." || value == ".." || value.chars().any(|c| "\\/:*?\"<>|".contains(c)) || is_windows_device(value) {
        return Err(format!("{label}包含非法字符或设备名"));
    }
    Ok(value.trim_end_matches(['.', ' ']).to_string())
}

fn unique_name(parent: &Path, requested: &str) -> String {
    let mut candidate = requested.to_string();
    let mut suffix = 2;
    let ext = requested.rsplit_once('.').map(|(_, e)| format!(".{e}")).unwrap_or_default();
    let stem = requested.strip_suffix(&ext).unwrap_or(requested).to_string();
    while parent.join(&candidate).exists() {
        candidate = format!("{stem}-{suffix}{ext}");
        suffix += 1;
    }
    candidate
}

fn copy_folder_world(source: &Path, destination: &Path, rec: Option<&TaskRecord>, on_file: &mut impl FnMut()) -> Result<(), String> {
    let mut queue = vec![(source.to_path_buf(), destination.to_path_buf())];
    while let Some((src, dst)) = queue.pop() {
        if let Some(r) = rec {
            crate::tasks::wait_while_paused(r);
            if r.cancelled.load(std::sync::atomic::Ordering::SeqCst) {
                return Err("已取消".into());
            }
        }
        std::fs::create_dir_all(&dst).map_err(|e| e.to_string())?;
        for entry in std::fs::read_dir(&src).map_err(|e| e.to_string())?.flatten() {
            let from = entry.path();
            let to = dst.join(entry.file_name());
            let Ok(meta) = std::fs::symlink_metadata(&from) else { continue };
            if meta.is_symlink() {
                continue;
            }
            if meta.is_dir() {
                queue.push((from, to));
            } else {
                if let Some(parent) = to.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                // flags: 'wx' 等价：失败即存在错误
                std::fs::File::create_new(&to).map_err(|e| e.to_string())?.sync_all().ok();
                std::fs::copy(&from, &to).map_err(|e| e.to_string())?;
                on_file();
            }
        }
    }
    Ok(())
}

fn extract_zip_world(
    file_path: &Path,
    world_root: &str,
    destination: &Path,
    rec: Option<&TaskRecord>,
    on_file: &mut impl FnMut(),
) -> Result<(), String> {
    let (entries, _) = inspect_zip(file_path)?;
    let expected: std::collections::HashMap<String, &ZipEntryInfo> =
        entries.iter().map(|e| (e.name.clone(), e)).collect();
    let file = std::fs::File::open(file_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let root = if world_root == "." { "" } else { world_root };
    for i in 0..archive.len() {
        if let Some(r) = rec {
            crate::tasks::wait_while_paused(r);
            if r.cancelled.load(std::sync::atomic::Ordering::SeqCst) {
                return Err("已取消".into());
            }
        }
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = normalize_archive_path(entry.name())?;
        let Some(safe) = expected.get(&name) else { continue };
        let Some(relative) = relative_in_root(&name, root) else { continue };
        if relative.is_empty() || safe.directory || safe.symlink {
            continue;
        }
        let target = destination.join(relative.replace('/', "\\"));
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut out = Vec::with_capacity(entry.size() as usize);
        std::io::Read::read_to_end(&mut entry, &mut out).map_err(|e| format!("ZIP 条目解压失败：{e}"))?;
        std::fs::File::create_new(&target).map_err(|e| e.to_string())?;
        std::fs::write(&target, out).map_err(|e| e.to_string())?;
        on_file();
    }
    Ok(())
}

/// world:import（对应原版 importWorld / importWorldInFolder；newInstance 分支安装目标
/// 实例后导入，失败连同新实例目录一并回滚）
pub fn import_world(
    app: &tauri::AppHandle,
    input: &str,
    options: &Value,
    rec: &TaskRecord,
) -> Result<Value, String> {
    let target_folder = options["targetFolder"].as_str().unwrap_or("").to_string();
    if target_folder.is_empty() {
        return Err("目标游戏文件夹未在 WUHUICL 中登记".into());
    }
    let info = probe_world(input)?.as_object().cloned().ok_or("该路径不是有效的 Minecraft 存档")?;
    let candidate_id = options["candidateId"].as_str().unwrap_or("");
    let candidates = info["candidates"].as_array().cloned().unwrap_or_default();
    let candidate = candidates
        .iter()
        .find(|c| c["id"].as_str() == Some(candidate_id))
        .cloned()
        .ok_or("所选存档已变化，请重新拖入并确认")?;
    let world_name = clean_leaf_name(options["worldName"].as_str().unwrap_or(""), "存档名称")?;
    let mut version_id = options["targetVersionId"].as_str().unwrap_or("").trim().to_string();
    let mut created_instance_dir: Option<std::path::PathBuf> = None;
    if let Some(new_inst) = options["newInstance"].as_object() {
        let minecraft_version = new_inst["minecraftVersion"].as_str().unwrap_or("").trim().to_string();
        let instance_name = clean_leaf_name(new_inst["instanceName"].as_str().unwrap_or(""), "实例名称")?;
        if minecraft_version.is_empty() {
            return Err("新实例必须指定 Minecraft 版本".into());
        }
        let dir = std::path::Path::new(&target_folder).join("versions").join(&instance_name);
        if dir.exists() {
            return Err(format!("实例名称已存在：{instance_name}"));
        }
        let loader = new_inst["loader"].as_str().unwrap_or("").to_string();
        let loader_version = new_inst["loaderVersion"].as_str().unwrap_or("").to_string();
        let app2 = app.clone();
        // 原版：创建目标实例占总进度 0~0.65，事件文本带「创建目标实例 ·」前缀
        let installed_id = crate::installer::install_vanilla_public(
            app,
            Some(rec),
            &minecraft_version,
            &target_folder,
            Some(&instance_name),
            if loader.is_empty() { None } else { Some(loader.as_str()) },
            if loader_version.is_empty() { None } else { Some(loader_version.as_str()) },
            |event: &Value| {
                let mut e = event.clone();
                if let Some(obj) = e.as_object_mut() {
                    let base = obj
                        .get("overall")
                        .and_then(|v| v.as_f64())
                        .or_else(|| obj.get("progress").and_then(|v| v.as_f64()))
                        .unwrap_or(0.0);
                    obj.insert("overall".into(), json!(base * 0.65));
                    let text = obj.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    obj.insert("text".into(), json!(format!("创建目标实例 · {text}")));
                }
                crate::tasks::emit_progress(&app2, e);
            },
        )?;
        version_id = installed_id;
        crate::installer::set_new_instance_isolation(&target_folder, &version_id, true)?;
        created_instance_dir = Some(std::path::Path::new(&target_folder).join("versions").join(&version_id));
    }
    if version_id.is_empty() {
        return Err("请选择已有实例或创建新实例".into());
    }
    let outcome = (|| -> Result<Value, String> {
    let installed = crate::versions::scan_folder_public(&target_folder, true);
    let installed = installed
        .iter()
        .find(|v| v["id"].as_str() == Some(version_id.as_str()))
        .cloned()
        .ok_or("目标实例不存在或文件不完整")?;
    if installed["incomplete"] == true || installed["failed"] == true {
        return Err("目标实例不存在或文件不完整".into());
    }
    if candidate["versionConfidence"] == "exact"
        && candidate["minecraftVersion"].is_string()
        && installed["mcVersion"] != candidate["minecraftVersion"]
        && options["allowVersionMismatch"] != true
    {
        return Err(format!(
            "存档版本为 {}，目标实例为 {}，需要明确确认跨版本导入",
            candidate["minecraftVersion"].as_str().unwrap_or(""),
            installed["mcVersion"].as_str().unwrap_or("")
        ));
    }
    // 游戏目录：隔离实例 = versions/<id>，共享 = folder 根
    let json_path = std::path::Path::new(&target_folder)
        .join("versions")
        .join(&version_id)
        .join(format!("{version_id}.json"));
    let game_directory = if installed["isolated"] == true {
        std::path::Path::new(&target_folder).join("versions").join(&version_id)
    } else {
        std::path::Path::new(&target_folder).to_path_buf()
    };
    let _ = json_path;
    let saves = game_directory.join("saves");
    let destination = saves.join(&world_name);
    if destination.exists() {
        return Err(format!("目标实例已存在同名存档：{world_name}"));
    }
    let transaction = game_directory.join(format!(".world-import-{}", crate::accounts::uuid_v4()));
    let staged_world = transaction.join("world");
    let file_count = candidate["fileCount"].as_u64().unwrap_or(0);
    let source_type = info["sourceType"].as_str().unwrap_or("").to_string();
    let emit_progress = |copied: u64, done: bool, app: &tauri::AppHandle, task_id: &str| {
        let ratio = if file_count > 0 { copied as f64 / file_count as f64 } else { 1.0 };
        crate::tasks::emit_progress(
            app,
            json!({
                "taskId": task_id,
                "taskTitle": "导入存档",
                "stage": if done { "done" } else { "world" },
                "progress": if done { 1.0 } else { ratio },
                "overall": if done { 1.0 } else { ratio * 0.9 + 0.05 },
                "text": if done { format!("存档「{world_name}」导入完成") } else { format!("导入存档文件 {copied}/{file_count}") }
            }),
        );
    };

    let result = (|| -> Result<Value, String> {
        std::fs::create_dir_all(&staged_world).map_err(|e| e.to_string())?;
        let mut copied: u64 = 0;
        let source_path = PathBuf::from(info["sourcePath"].as_str().unwrap_or(""));
        if source_type == "folder" {
            let source_root = source_path.join(candidate_id.replace('/', "\\"));
            copy_folder_world(&source_root, &staged_world, Some(rec), &mut || {
                copied += 1;
                if copied % 16 == 0 || copied == file_count {
                    emit_progress(copied, false, app, &rec.id.clone());
                }
            })?;
        } else {
            extract_zip_world(&source_path, candidate_id, &staged_world, Some(rec), &mut || {
                copied += 1;
                if copied % 16 == 0 || copied == file_count {
                    emit_progress(copied, false, app, &rec.id.clone());
                }
            })?;
        }
        std::fs::create_dir_all(&saves).map_err(|e| e.to_string())?;
        if destination.exists() {
            return Err(format!("目标实例已存在同名存档：{world_name}"));
        }
        std::fs::rename(&staged_world, &destination).map_err(|e| e.to_string())?;
        emit_progress(copied, true, app, &rec.id);
        Ok(json!({
            "versionId": version_id,
            "worldName": world_name,
            "worldDirectory": destination.to_string_lossy(),
            "installedResourcePacks": [],
            "createdInstance": created_instance_dir.is_some()
        }))
    })();

    let _ = std::fs::remove_dir_all(&transaction);
    result.map_err(|e| format!("存档导入失败，已回滚：{e}"))
    })();

    // 新实例导入失败时连同刚创建的实例目录一并回滚（原版 finally 分支）
    if outcome.is_err() {
        if let Some(d) = &created_instance_dir {
            let _ = std::fs::remove_dir_all(d);
        }
    }
    outcome
}

/// 统一入口
pub fn invoke(app: &tauri::AppHandle, channel: &str, args: &[Value]) -> Option<Result<Value, String>> {
    match channel {
        "world:probe" => Some(
            args.first()
                .and_then(|v| v.as_str())
                .ok_or_else(|| "缺少参数".to_string())
                .and_then(probe_world),
        ),
        "world:import" => Some((|| {
            let Some(input) = args.first().and_then(|v| v.as_str()) else {
                return Err("缺少参数".to_string());
            };
            let options = args.get(1).cloned().unwrap_or(Value::Null);
            let rec = crate::tasks::register_task("导入存档", "world");
            let app2 = app.clone();
            let input2 = input.to_string();
            let options2 = options.clone();
            // invoke 契约：立即受理，完成经 event:taskDone 推送（与原版一致不阻塞）
            std::thread::spawn(move || {
                let result = import_world(&app2, &input2, &options2, &rec);
                let cancelled = result.as_ref().err().map(|e| e == "已取消").unwrap_or(false);
                crate::tasks::task_done(
                    &app2,
                    &rec.id,
                    result.is_ok(),
                    result.as_ref().err().cloned(),
                    cancelled,
                    None,
                );
            });
            Ok(Value::Null)
        })()),
        _ => None,
    }
}
