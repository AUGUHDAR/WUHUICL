//! Filesystem, folder, and dialog helpers for IPC fallback channels.

use crate::{installer, settings, versions};
use serde_json::{json, Value};
use tauri::AppHandle;

pub(crate) fn drag_resource_files(
    root: &std::path::Path,
    names: &Value,
) -> Result<Vec<String>, String> {
    let entries = match names.as_array() {
        Some(entries) if !entries.is_empty() && entries.len() <= 1000 => entries,
        _ => return Err("每次可拖出 1–1000 个文件，请分批选择".into()),
    };
    let base = std::fs::symlink_metadata(root).map_err(|_| "目录不是普通目录".to_string())?;
    if !base.is_dir() || base.file_type().is_symlink() {
        return Err("目录不是普通目录".into());
    }
    let mut output = Vec::new();
    let mut seen = Vec::new();
    for item in entries {
        let name = match item.as_str() {
            Some(name) => name,
            None => return Err("无效的文件名".into()),
        };
        if name.contains('/') || name.contains('\\') {
            return Err("无效的文件名".into());
        }
        if seen.iter().any(|seen_name| seen_name == name) {
            continue;
        }
        seen.push(name.to_string());
        let mut file = root.to_path_buf();
        for part in name.split('/') {
            file = file.join(part);
            let metadata =
                std::fs::symlink_metadata(&file).map_err(|_| "不跟随符号链接".to_string())?;
            if metadata.file_type().is_symlink() {
                return Err("不跟随符号链接".into());
            }
        }
        let metadata =
            std::fs::symlink_metadata(&file).map_err(|_| "不支持拖出特殊文件".to_string())?;
        if !metadata.is_file() && !metadata.is_dir() {
            return Err("不支持拖出特殊文件".into());
        }
        output.push(file.to_string_lossy().to_string());
    }
    Ok(output)
}

pub(crate) fn two_strings(args: &[Value]) -> Result<(String, String), String> {
    let first = args
        .first()
        .and_then(|value| value.as_str())
        .ok_or("缺少参数")?;
    let second = args
        .get(1)
        .and_then(|value| value.as_str())
        .ok_or("缺少参数")?;
    Ok((first.to_string(), second.to_string()))
}

pub(crate) fn two_strings_first_optional(args: &[Value]) -> Result<(String, String), String> {
    let first = args.first().and_then(|value| value.as_str()).unwrap_or("");
    let second = args.get(1).and_then(|value| value.as_str()).unwrap_or("");
    Ok((first.to_string(), second.to_string()))
}

pub(crate) fn isolated_version_of(rel: &str) -> Option<String> {
    let mut parts = rel.split('/');
    if parts.next()? != "versions" {
        return None;
    }
    let id = parts.next()?.trim_end_matches('\\');
    if id.is_empty() || parts.next().is_none() {
        return None;
    }
    Some(id.to_string())
}

pub(crate) fn import_resource_files(
    files: &[String],
    target: &Value,
    kind: &str,
) -> Result<Value, String> {
    let jar_re = regex::Regex::new(r"(?i)\.jar(\.disabled)?$").unwrap();
    let zip_re = regex::Regex::new(r"(?i)\.zip$").unwrap();
    let game_dir = match kind {
        "mods" | "resourcepacks" | "shaderpacks" => target["gameDirectory"]
            .as_str()
            .filter(|value| !value.is_empty()),
        _ => None,
    };
    let Some(game_dir) = game_dir else {
        return Err("无效资源目录".into());
    };
    if files.is_empty() {
        return Err("请拖入资源文件".into());
    }
    let dir = std::path::Path::new(game_dir).join(kind);
    let mut names = Vec::new();
    let mut copies: Vec<(std::path::PathBuf, std::path::PathBuf, bool)> = Vec::new();
    for file in files {
        let path = std::path::Path::new(file);
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let metadata =
            std::fs::metadata(path).map_err(|error| format!("此页面不支持 {name}：{error}"))?;
        let unsupported = if metadata.is_dir() {
            kind == "mods"
        } else {
            !(if kind == "mods" {
                jar_re.is_match(&name)
            } else {
                zip_re.is_match(&name)
            })
        };
        if unsupported {
            return Err(format!("此页面不支持 {name}"));
        }
        if !metadata.is_dir() && !metadata.is_file() {
            return Err("不支持此文件类型".into());
        }
        let destination = dir.join(&name);
        let lower = name.to_lowercase();
        if names.contains(&lower) || destination.exists() {
            return Err(format!("同名文件已存在，未覆盖：{name}"));
        }
        names.push(lower);
        copies.push((path.to_path_buf(), destination, metadata.is_dir()));
    }
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    for (source, destination, is_dir) in copies {
        if is_dir {
            copy_tree(&source, &destination)?;
        } else {
            let mut options = std::fs::OpenOptions::new();
            options.create_new(true).write(true);
            let mut data = std::fs::read(&source).map_err(|error| error.to_string())?;
            let mut file = options
                .open(&destination)
                .map_err(|error| error.to_string())?;
            std::io::Write::write_all(&mut file, &mut data).map_err(|error| error.to_string())?;
        }
    }
    Ok(json!(files.len()))
}

fn copy_tree(src: &std::path::Path, dest: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(|error| error.to_string())?;
    for entry in std::fs::read_dir(src).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let destination = dest.join(entry.file_name());
        if entry
            .file_type()
            .map_err(|error| error.to_string())?
            .is_dir()
        {
            copy_tree(&entry.path(), &destination)?;
        } else if destination.exists() {
            return Err(format!("同名文件已存在，未覆盖：{}", destination.display()));
        } else {
            std::fs::copy(entry.path(), &destination).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

pub(crate) fn folders_mutate(
    app: &AppHandle,
    mutate: impl FnOnce(&mut Vec<Value>),
) -> Result<Value, String> {
    let mut current = settings::get(app);
    {
        let folders = current["folders"].as_array_mut().ok_or("设置损坏")?;
        mutate(folders);
    }
    let output = current["folders"].clone();
    settings::save(app, current)?;
    Ok(output)
}

pub(crate) fn folders_add(app: &AppHandle, path: &str) -> Result<Value, String> {
    let canonical = installer::strip_device_prefix(
        &std::fs::canonicalize(path).map_err(|_| "目录不存在".to_string())?,
    )
    .to_string_lossy()
    .to_string();
    let name = std::path::Path::new(&canonical)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| canonical.clone());
    let mut current = settings::get(app);
    {
        let folders = current["folders"].as_array_mut().ok_or("设置损坏")?;
        if folders
            .iter()
            .any(|folder| folder["path"].as_str() == Some(canonical.as_str()))
        {
            return Err("该目录已登记".into());
        }
        folders.push(json!({ "path": canonical, "name": name, "isDefault": false }));
    }
    let folder = current["folders"]
        .as_array()
        .unwrap()
        .last()
        .cloned()
        .unwrap();
    let folders_json = current["folders"].clone();
    let structure = versions::scan_folder_public(&canonical, true);
    settings::save(app, current)?;
    Ok(json!({ "folders": folders_json, "folder": folder, "structure": structure }))
}

fn resolve_resource_directory(
    app: &AppHandle,
    folder: &str,
    id: &str,
    kind: &str,
) -> Result<std::path::PathBuf, String> {
    if id.trim().is_empty() {
        return Err("请先安装或选择一个游戏版本".into());
    }
    if id == "." || id == ".." || id.contains(['\\', '/', ':', '\0']) {
        return Err("游戏版本名称无效，请重新选择".into());
    }
    if folder.is_empty() || !matches!(kind, "mods" | "resourcepacks" | "shaderpacks") {
        return Err("资源目录无效，请重新选择游戏版本".into());
    }
    let json_path = std::path::Path::new(folder)
        .join("versions")
        .join(id)
        .join(format!("{}.json", id));
    let raw = std::fs::read_to_string(&json_path).map_err(|_| {
        "所选游戏版本已移除或版本描述损坏，请到「游戏版本」页刷新或修复".to_string()
    })?;
    let json: Value = serde_json::from_str(raw.trim_start_matches('\u{feff}')).map_err(|_| {
        "所选游戏版本已移除或版本描述损坏，请到「游戏版本」页刷新或修复".to_string()
    })?;
    let state = installer::instance_directory_state(app, folder, id, &json);
    let game_dir = state["path"]
        .as_str()
        .ok_or("所选游戏版本已移除或版本描述损坏，请到「游戏版本」页刷新或修复")?;
    Ok(std::path::Path::new(game_dir).join(kind))
}

/// Resolve a safe resource path using the active or explicitly selected folder.
pub fn fs_resolve(app: &AppHandle, rel: &str, folder: &str) -> Result<std::path::PathBuf, String> {
    let current = settings::get(app);
    let parts: Vec<&str> = rel
        .split(['/', '\\'])
        .filter(|part| !part.is_empty() && *part != ".")
        .collect();
    if parts.iter().any(|part| *part == "..") {
        return Err("非法目录".into());
    }
    let is_version_path = parts.first().copied() == Some("versions");
    if parts.len() > if is_version_path { 3 } else { 2 } {
        return Err("非法目录".into());
    }
    if !folder.is_empty() {
        let known = current["folders"]
            .as_array()
            .map(|folders| {
                folders.iter().any(|known| {
                    installer::path_identity(known["path"].as_str().unwrap_or(""))
                        == installer::path_identity(folder)
                })
            })
            .unwrap_or(false);
        if !known {
            return Err("游戏文件夹未登记".into());
        }
    }
    let base = if !folder.is_empty() {
        folder.to_string()
    } else if is_version_path && parts.len() >= 2 {
        installer::folder_of_version(app, parts[1])
    } else {
        let active = current["activeFolder"].as_str().unwrap_or("").to_string();
        if active.is_empty() {
            current["gameDir"].as_str().unwrap_or("").to_string()
        } else {
            active
        }
    };
    if base.is_empty() {
        return Err("未配置游戏目录".into());
    }
    if is_version_path
        && parts.len() == 3
        && matches!(parts[2], "mods" | "resourcepacks" | "shaderpacks")
    {
        return resolve_resource_directory(app, &base, parts[1], parts[2]);
    }
    let mut output = std::path::PathBuf::from(&base);
    for part in &parts {
        output.push(part);
    }
    if !output.starts_with(std::path::Path::new(&base)) {
        return Err("非法目录".into());
    }
    Ok(output)
}

pub(crate) fn fs_list(app: &AppHandle, rel: &str, folder: &str) -> Result<Value, String> {
    let dir = fs_resolve(app, rel, folder)?;
    let names = match std::fs::read_dir(&dir) {
        Ok(entries) => entries.filter_map(|entry| entry.ok()).collect::<Vec<_>>(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(json!([])),
        Err(_) => return Err("无法读取资源目录，请检查文件夹权限后重试".into()),
    };
    let mut output = Vec::new();
    for entry in names {
        let name = entry.file_name().to_string_lossy().into_owned();
        let metadata = match std::fs::symlink_metadata(&entry.path()) {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        if metadata.file_type().is_symlink() || name == ".DS_Store" {
            continue;
        }
        let mtime = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or(0);
        output.push(
            json!({ "name": name, "size": metadata.len(), "isDir": metadata.is_dir(), "mtime": mtime }),
        );
    }
    output.sort_by(|left, right| {
        let left_dir = left["isDir"].as_bool().unwrap_or(false);
        let right_dir = right["isDir"].as_bool().unwrap_or(false);
        right_dir.cmp(&left_dir).then_with(|| {
            left["name"]
                .as_str()
                .unwrap_or("")
                .to_lowercase()
                .cmp(&right["name"].as_str().unwrap_or("").to_lowercase())
        })
    });
    Ok(Value::Array(output))
}

pub(crate) fn pick(app: &AppHandle, directory: bool, filters: &[&str]) -> Value {
    use tauri_plugin_dialog::DialogExt;
    let dialog = app.dialog().file();
    let picked = if directory {
        dialog.blocking_pick_folder()
    } else {
        let mut dialog = app.dialog().file();
        for filter in filters {
            let (title, patterns) = filter.split_once('|').unwrap_or((filter, "*"));
            let extensions: Vec<&str> = patterns
                .split(';')
                .filter_map(|pattern| pattern.strip_prefix("*."))
                .collect();
            if !extensions.is_empty() {
                dialog = dialog.add_filter(title.to_string(), &extensions);
            }
        }
        dialog.blocking_pick_file()
    };
    picked
        .map(|path| json!(path.to_string()))
        .unwrap_or(Value::Null)
}
