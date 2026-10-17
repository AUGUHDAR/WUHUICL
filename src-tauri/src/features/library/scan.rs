//! Mod directory scans and archive identity extraction.

use super::archive::parse_mod_archive;
use super::fingerprint::curse_fingerprint;
use crate::net::Sha1;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::io::Read;
use std::path::Path;
use std::sync::OnceLock;

// ---------------- modScan.ts / modScanWorker.ts ----------------

/// scanModDirectory()：analysis/icons 含图标，catalog 不含；hash=true 时附 sha1；
/// 传入 names 时同时计算 CurseForge 指纹并接受 .jar.disabled/.zip
pub fn scan_mod_directory(
    dir: &str,
    hash: bool,
    names: Option<&[String]>,
    purpose: &str,
) -> Result<Vec<Value>, String> {
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
                    JAR_ZIP
                        .get_or_init(|| {
                            regex::Regex::new(r"(?i)\.(?:jar(?:\.disabled)?|zip)$").unwrap()
                        })
                        .is_match(&name)
                }
            }
            None => JAR
                .get_or_init(|| regex::Regex::new(r"(?i)\.jar$").unwrap())
                .is_match(&name),
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
                        info["iconDataUrl"] = json!(format!(
                            "data:image/png;base64,{}",
                            base64::engine::general_purpose::STANDARD.encode(bytes)
                        ));
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
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(data)).map_err(|e| e.to_string())?;
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
            .filter(|n| {
                regex::Regex::new(r"(?i)\.jar(?:\.disabled)?$")
                    .unwrap()
                    .is_match(n)
            })
            .collect(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.to_string()),
    };
    scan_mod_directory(dir, true, Some(&names), purpose)
}

/// parseModFile(): single archive parse with the legacy fixed failure shape.
pub fn parse_mod_file(file_path: &str) -> Value {
    parse_file_path(file_path)
}

fn parse_file_path(file_path: &str) -> Value {
    let file_name = crate::installer::basename_of(file_path);
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

/// Expand a one-level set of jar files and folders for bulk analysis.
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
                    skipped.push(format!(
                        "{}（文件夹内无 .jar）",
                        crate::installer::basename_of(p)
                    ));
                }
            }
            Ok(_) => {
                if p.to_lowercase().ends_with(".jar") {
                    files.push(p.clone());
                } else {
                    skipped.push(crate::installer::basename_of(p));
                }
            }
            Err(_) => skipped.push(crate::installer::basename_of(p)),
        }
    }
    (files, skipped)
}
