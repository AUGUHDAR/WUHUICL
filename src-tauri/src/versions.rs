//! 已安装版本扫描（对应原版 core/versions.ts 的 scanInstalledFolder / listInstalled / sortInstalled）。

mod instance_metadata;
mod version_evidence;

use crate::installer::{self, DATA_DIR_NAME};
use instance_metadata::resolve_instance_metadata;
use serde_json::{json, Value};
use std::cmp::Ordering;
use std::path::{Path, PathBuf};

/// versions:installed → InstalledVersion[]（all=true 时等同原版 listAllInstalled，扫全部登记文件夹）
pub fn list_installed(settings: &Value, all: bool) -> Value {
    let folders: Vec<String> = settings["folders"]
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|f| f["path"].as_str())
                .filter(|path| !path.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let game_dir = settings["activeFolder"]
        .as_str()
        .or_else(|| settings["gameDir"].as_str())
        .unwrap_or("")
        .to_string();
    let default_folder = settings["folders"]
        .as_array()
        .and_then(|list| list.iter().find(|f| f["isDefault"] == json!(true)))
        .and_then(|f| f["path"].as_str())
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| game_dir.clone());
    let active_root = game_dir.clone();
    let roots: Vec<String> = if all && !folders.is_empty() {
        folders
    } else if game_dir.is_empty() {
        Vec::new()
    } else {
        vec![game_dir]
    };
    let mut out: Vec<Value> = Vec::new();
    for root in &roots {
        let (versions, _) = scan_folder(
            root,
            Some(default_folder.as_str()),
            Some(active_root.as_str()),
        );
        out.extend(versions);
    }
    sort_installed(&mut out);
    Value::Array(out)
}

/// 对外暴露的文件夹扫描（folders:scan / folders:add 用）
pub fn scan_folder_public(root: &str, all: bool) -> Vec<Value> {
    let (mut versions, _) = scan_folder(root, None, None);
    if !all {
        versions.retain(|v| v["incomplete"] != json!(true) && v["failed"] != json!(true));
    }
    versions
}

fn text<'a>(j: &'a Value, key: &str) -> Option<&'a str> {
    j.get(key).and_then(Value::as_str).filter(|s| !s.is_empty())
}

fn scan_folder(
    root: &str,
    default_folder: Option<&str>,
    active_root: Option<&str>,
) -> (Vec<Value>, Vec<String>) {
    let mut out: Vec<Value> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    let root_path = installer::canonical_path(root);
    let root_s = root_path.to_string_lossy().to_string();
    let vdir = root_path.join("versions");
    if !vdir.exists() {
        return (out, errors);
    }
    let entries: Vec<std::fs::DirEntry> = match std::fs::read_dir(&vdir) {
        Ok(rd) => rd
            .flatten()
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .collect(),
        Err(error) => {
            errors.push(format!("无法读取 versions：{}", error));
            return (out, errors);
        }
    };
    let base_default = default_folder.unwrap_or(root_s.as_str()).to_string();
    if let Some(active) = active_root {
        if installer::same_path(active, &root_s) {
            for entry in &entries {
                installer::register_version_folder(&entry.file_name().to_string_lossy(), &root_s);
            }
        }
    }
    for entry in entries {
        let name = entry.file_name().to_string_lossy().to_string();
        let json_path = vdir.join(&name).join(format!("{name}.json"));
        if !json_path.exists() {
            out.push(
                json!({ "id": name, "mcVersion": "未知", "folder": root_s, "incomplete": true }),
            );
            errors.push(format!("{name}：缺少版本描述 {name}.json"));
            continue;
        }
        let j = match installer::read_version_json_from(&json_path) {
            Ok(j) => j,
            Err(error) => {
                out.push(json!({ "id": name, "mcVersion": "未知", "folder": root_s, "incomplete": true }));
                errors.push(format!("{name}：版本描述损坏（{error}）"));
                continue;
            }
        };
        let local_parent = |id: &str| -> Option<Value> {
            let local = vdir.join(id).join(format!("{id}.json"));
            let local_base = root_path
                .join(DATA_DIR_NAME)
                .join("base")
                .join(id)
                .join(format!("{id}.json"));
            let file = if local.exists() {
                local
            } else if local_base.exists() {
                local_base
            } else {
                installer::base_version_json_path_in(&base_default, id)
            };
            installer::read_version_json_from(&file).ok()
        };
        let read_evidence = |chain: &[Value]| -> Option<String> {
            let jar = if chain.len() == 1 {
                vdir.join(&name).join(format!("{name}.jar"))
            } else {
                let parent_id = chain[chain.len() - 2]
                    .get("inheritsFrom")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let local_json = vdir.join(parent_id).join(format!("{parent_id}.json"));
                let local_base_json = root_path
                    .join(DATA_DIR_NAME)
                    .join("base")
                    .join(parent_id)
                    .join(format!("{parent_id}.json"));
                if local_json.exists() {
                    vdir.join(parent_id).join(format!("{parent_id}.jar"))
                } else if local_base_json.exists() {
                    root_path
                        .join(DATA_DIR_NAME)
                        .join("base")
                        .join(parent_id)
                        .join(format!("{parent_id}.jar"))
                } else {
                    installer::base_version_jar_path_in(&base_default, parent_id)
                }
            };
            version_evidence::read_client_version_evidence(&jar).or_else(|| {
                version_evidence::cached_client_version_evidence(
                    chain,
                    &[
                        vdir.clone(),
                        root_path.join(DATA_DIR_NAME).join("base"),
                        PathBuf::from(&base_default).join(DATA_DIR_NAME),
                    ],
                )
            })
        };
        let parent_fn: &dyn Fn(&str) -> Option<Value> = &local_parent;
        let evidence_fn: &dyn Fn(&[Value]) -> Option<String> = &read_evidence;
        let resolved = resolve_instance_metadata(&j, parent_fn, Some(evidence_fn));
        let mut item = json!({ "id": name, "mcVersion": resolved.mc_version, "folder": root_s });
        if let Some(loader) = resolved.loader {
            item["loader"] = json!(loader);
        }
        if let Some(loader_version) = &resolved.loader_version {
            item["loaderVersion"] = json!(loader_version);
        }
        for (from, to) in [
            ("_modpackName", "modpackName"),
            ("_modpackVersion", "modpackVersion"),
            ("_javaPath", "javaPath"),
            ("_icon", "icon"),
        ] {
            if let Some(value) = text(&j, from) {
                item[to] = json!(value);
            }
        }
        if j.get("_javaAuto") == Some(&Value::Bool(true)) {
            item["javaAuto"] = json!(true);
        }
        if let Some(resolution) = j
            .get("_resolution")
            .filter(|v| !v.is_null() && v.as_bool() != Some(false))
        {
            item["resolution"] = normalize_stored_resolution(resolution);
        }
        if let Some(candidate) = text(&j, "_thumbnail") {
            if let Some(thumbnail) = ensure_instance_thumbnail(candidate, &root_path) {
                item["thumbnail"] = json!(thumbnail);
            }
        }
        if let Some(fit) = j.get("_thumbnailFit").and_then(Value::as_str) {
            if matches!(fit, "fill" | "fit" | "crop") {
                item["thumbnailFit"] = json!(fit);
            }
        }
        let (game_directory, isolated, reason) = directory_state(&root_s, &name, &j);
        item["isolated"] = json!(isolated);
        item["gameDirectory"] = json!(game_directory);
        item["isolationReason"] = json!(reason);
        if resolved.broken {
            item["incomplete"] = json!(true);
            errors.push(format!(
                "{name}：继承的版本 {} 缺失或损坏",
                text(&j, "inheritsFrom").unwrap_or("未知")
            ));
        }
        if text(&j, "inheritsFrom").is_none() {
            let jar = vdir.join(&name).join(format!("{name}.jar"));
            let part = format!("{}.part", jar.to_string_lossy());
            if !jar.exists() || Path::new(&part).exists() {
                item["incomplete"] = json!(true);
            }
        }
        if vdir.join(&name).join(".installing").exists() {
            item["failed"] = json!(true);
        }
        out.push(item);
    }
    sort_installed(&mut out);
    (out, errors)
}

/// 对应原版 instances.ts instanceDirectoryState（installer 版需要 AppHandle，这里用同一套公共原语按根目录判定）
fn directory_state(root: &str, id: &str, j: &Value) -> (String, bool, &'static str) {
    let shared = installer::canonical_path(root)
        .to_string_lossy()
        .to_string();
    let instance = installer::canonical_path(
        &Path::new(&shared)
            .join("versions")
            .join(id)
            .to_string_lossy(),
    )
    .to_string_lossy()
    .to_string();
    match j.get("_gameDir") {
        Some(Value::Bool(true)) => return (instance, true, "explicit"),
        Some(Value::Bool(false)) => return (shared, false, "shared"),
        _ => {}
    }
    if let Some(configured) = j
        .get("_gameDirectory")
        .or_else(|| j.get("gameDirectory"))
        .and_then(Value::as_str)
    {
        if !configured.trim().is_empty() {
            let raw = if Path::new(configured).is_absolute() {
                PathBuf::from(configured)
            } else {
                Path::new(&instance).join(configured)
            };
            let resolved = installer::canonical_path(&raw.to_string_lossy())
                .to_string_lossy()
                .to_string();
            let isolated = !installer::same_path(&resolved, &shared);
            return (resolved, isolated, "configured-path");
        }
    }
    if text(j, "_modpackName").is_some() {
        return (instance, true, "modpack");
    }
    if installer::has_isolation_content(Path::new(&instance)) {
        return (instance, true, "detected-content");
    }
    (shared, false, "shared")
}

fn normalize_stored_resolution(value: &Value) -> Value {
    let mode = match value.get("mode").and_then(Value::as_str) {
        Some(mode @ ("windowed" | "maximized" | "fullscreen")) => mode.to_string(),
        _ => {
            if value.get("fullscreen").and_then(Value::as_bool) == Some(true) {
                "fullscreen".to_string()
            } else {
                "windowed".to_string()
            }
        }
    };
    let side = |key: &str, fallback: i64| {
        value
            .get(key)
            .and_then(Value::as_f64)
            .filter(|v| v.is_finite())
            .map(|v| v.round() as i64)
            .unwrap_or(fallback)
    };
    json!({ "width": side("width", 854), "height": side("height", 480), "mode": mode, "fullscreen": mode == "fullscreen" })
}

fn ensure_instance_thumbnail(candidate: &str, root: &Path) -> Option<String> {
    let directory = root.join(DATA_DIR_NAME).join("thumbnails");
    let resolved = installer::canonical_path(candidate);
    if !resolved.starts_with(&directory) || !resolved.is_file() {
        return None;
    }
    Some(resolved.to_string_lossy().to_string())
}

// 对应原版 sortInstalled：MC 版本分组（新→旧）+ 同版本内纯净版在前 + id 字母序
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Weight(u8, u32, u8);

fn weights(s: &str) -> Vec<Weight> {
    s.chars()
        .map(|c| {
            if c.is_ascii_digit() {
                Weight(2, c as u32, 0)
            } else if c.is_ascii_alphabetic() {
                Weight(
                    4,
                    c.to_ascii_lowercase() as u32,
                    if c.is_ascii_uppercase() { 1 } else { 0 },
                )
            } else if c as u32 > 127 {
                Weight(3, c as u32, 0)
            } else {
                Weight(1, c as u32, 0)
            }
        })
        .collect()
}

fn digit_run(weights: &[Weight], from: usize) -> (usize, String) {
    let mut end = from;
    let mut digits = String::new();
    while end < weights.len() && weights[end].0 == 2 {
        digits.push(char::from_u32(weights[end].1).unwrap_or('0'));
        end += 1;
    }
    let trimmed = digits.trim_start_matches('0');
    (
        end,
        if trimmed.is_empty() {
            "0".to_string()
        } else {
            trimmed.to_string()
        },
    )
}

fn locale_cmp(a: &str, b: &str, numeric: bool) -> Ordering {
    let (ka, kb) = (weights(a), weights(b));
    let (mut i, mut j) = (0usize, 0usize);
    while i < ka.len() && j < kb.len() {
        if numeric && ka[i].0 == 2 && kb[j].0 == 2 {
            let (ie, na) = digit_run(&ka, i);
            let (je, nb) = digit_run(&kb, j);
            let ord = if na.len() != nb.len() {
                na.len().cmp(&nb.len())
            } else {
                na.cmp(&nb)
            };
            if ord != Ordering::Equal {
                return ord;
            }
            i = ie;
            j = je;
            continue;
        }
        if ka[i] != kb[j] {
            return ka[i].cmp(&kb[j]);
        }
        i += 1;
        j += 1;
    }
    (ka.len() - i).cmp(&(kb.len() - j))
}

fn sort_installed(out: &mut Vec<Value>) {
    out.sort_by(|a, b| {
        let left = a["mcVersion"].as_str().unwrap_or("");
        let right = b["mcVersion"].as_str().unwrap_or("");
        if left != right {
            return locale_cmp(right, left, true);
        }
        let has = |v: &Value| {
            v.get("loader")
                .and_then(Value::as_str)
                .map(|s| !s.is_empty())
                .unwrap_or(false)
        };
        let (la, lb) = (has(a), has(b));
        if la != lb {
            return if la {
                Ordering::Greater
            } else {
                Ordering::Less
            };
        }
        locale_cmp(
            a["id"].as_str().unwrap_or(""),
            b["id"].as_str().unwrap_or(""),
            false,
        )
    });
}
