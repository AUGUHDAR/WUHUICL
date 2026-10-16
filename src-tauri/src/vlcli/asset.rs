//! VoxLink CLI 取件层：解析版本、双源下载、SHA-256 校验、本地缓存。

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use tauri::Emitter;

pub const OWNER_REPO: &str = "AUGUHDAR/VoxLink-app";
pub const ASSET: &str = "VoxLink-CLI-windows-amd64.exe";
pub const SUMS: &str = "SHA256SUMS.txt";
const LATEST_API: &str = "https://api.github.com/repos/AUGUHDAR/VoxLink-app/releases/latest";
const GH_ASSET: &str = "https://github.com/AUGUHDAR/VoxLink-app/releases/download/";
const GITEE_ASSET: &str = "https://gitee.com/AUGUHDAR/VoxLink-app/releases/download/";
const MAX_BYTES: u64 = 32 * 1024 * 1024;

pub fn dir_for(app: &tauri::AppHandle) -> PathBuf {
    crate::settings::user_data_dir(app).join("voxlink")
}

pub fn binary_path(app: &tauri::AppHandle) -> PathBuf {
    dir_for(app).join(ASSET)
}

fn meta_path(app: &tauri::AppHandle) -> PathBuf {
    dir_for(app).join("cli-meta.json")
}

pub fn read_meta(app: &tauri::AppHandle) -> Value {
    fs::read_to_string(meta_path(app))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({}))
}

fn write_meta(app: &tauri::AppHandle, meta: &Value) {
    if let Some(p) = meta_path(app).parent() {
        let _ = fs::create_dir_all(p);
    }
    if let Ok(s) = serde_json::to_string_pretty(meta) {
        let _ = fs::write(meta_path(app), s);
    }
}

/// 解析最新发布号与校验值；失败返回 Err，调用方可退回缓存。
pub fn resolve_latest() -> Result<(String, String), String> {
    let page = crate::net::get_text(LATEST_API, 12_000).map_err(|e| format!("版本查询失败：{}", e))?;
    let v: Value = serde_json::from_str(&page).map_err(|_| "版本接口返回异常".to_string())?;
    let tag = v["tag_name"].as_str().unwrap_or("").trim().to_string();
    if tag.is_empty() {
        return Err("版本接口缺少 tag_name".into());
    }
    let sums = crate::net::get_text(&format!("{}{}/{}", GH_ASSET, tag, SUMS), 20_000)
        .or_else(|_| crate::net::get_text(&format!("{}{}/{}", GITEE_ASSET, tag, SUMS), 20_000))
        .map_err(|e| format!("校验文件获取失败：{}", e))?;
    let sha = expected_sha(&sums, ASSET).ok_or("校验文件里没有这个资产".to_string())?;
    Ok((tag, sha))
}

pub fn expected_sha(sums: &str, asset: &str) -> Option<String> {
    for line in sums.lines() {
        let mut it = line.split_whitespace();
        let (Some(hash), Some(name)) = (it.next(), it.next()) else {
            continue;
        };
        let clean = name.trim_start_matches('*').replace('\\', "/");
        if clean.rsplit('/').next() == Some(asset) && hash.len() == 64 {
            return Some(hash.to_ascii_lowercase());
        }
    }
    None
}

pub fn sha256_of(path: &Path) -> Result<String, String> {
    let file = fs::File::open(path).map_err(|e| format!("读取失败：{}", e))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    let mut reader = std::io::BufReader::new(file);
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("读取失败：{}", e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn emit_progress(app: &tauri::AppHandle, stage: &str, done: u64, total: Option<u64>, text: &str) {
    let _ = app.emit_to("main", "vlcli:progress", json!({
        "stage": stage, "bytes": done, "total": total, "text": text,
    }));
}

/// 确保本地有可用的 CLI；force=true 时即使版本一致也重下。
pub fn ensure(app: &tauri::AppHandle, force: bool) -> Value {
    let target = binary_path(app);
    let meta = read_meta(app);
    let cached_version = meta["version"].as_str().unwrap_or("").to_string();
    // 24 小时内不重复问版本，玩家不该每次进联机都等一趟网络
    let fresh = meta["checkedAt"].as_u64().unwrap_or(0) + 86_400_000 > crate::download::now_ms();
    if !force && fresh && target.is_file() && !cached_version.is_empty() {
        return json!({ "ok": true, "version": cached_version, "cached": true, "path": target.to_string_lossy() });
    }

    let (tag, sha) = match resolve_latest() {
        Ok(v) => v,
        Err(e) => {
            if !force && target.is_file() {
                return json!({ "ok": true, "offline": true, "version": cached_version,
                    "path": target.to_string_lossy(), "reason": e });
            }
            return json!({ "ok": false, "error": e });
        }
    };
    if !force && cached_version == tag && target.is_file() {
        match sha256_of(&target) {
            Ok(actual) if actual == sha => {
                write_meta(app, &json!({ "version": tag, "sha256": sha,
                    "updatedAt": meta["updatedAt"].clone(), "checkedAt": crate::download::now_ms(),
                    "bytes": meta["bytes"].clone() }));
                return json!({ "ok": true, "version": tag, "cached": true, "path": target.to_string_lossy() });
            }
            Ok(_) => {
                let _ = fs::remove_file(&target);
            }
            Err(_) => {}
        }
    }

    emit_progress(app, "download", 0, None, "正在准备联机组件…");
    let urls = vec![format!("{}{}/{}", GH_ASSET, tag, ASSET), format!("{}{}/{}", GITEE_ASSET, tag, ASSET)];
    match fetch(&urls, &target, &sha, app) {
        Ok(bytes) => {
            write_meta(app, &json!({ "version": tag, "sha256": sha, "updatedAt": crate::download::now_ms(),
                "checkedAt": crate::download::now_ms(),
                "bytes": bytes }));
            emit_progress(app, "ready", bytes, Some(bytes), "联机组件就绪");
            json!({ "ok": true, "version": tag, "path": target.to_string_lossy(), "downloaded": true })
        }
        Err(e) => {
            emit_progress(app, "failed", 0, None, &e);
            json!({ "ok": false, "error": e, "version": cached_version })
        }
    }
}

fn fetch(urls: &[String], target: &Path, sha: &str, app: &tauri::AppHandle) -> Result<u64, String> {
    let parent = target.parent().ok_or("组件路径异常")?;
    fs::create_dir_all(parent).map_err(|e| format!("无法创建组件目录：{}", e))?;
    let mut last = String::from("联机组件下载失败");
    for url in urls {
        match download_one(url, target, sha, app) {
            Ok(n) => return Ok(n),
            Err(e) => {
                crate::download::note_host_failure(url);
                last = e;
            }
        }
    }
    Err(last)
}

fn download_one(url: &str, target: &Path, sha: &str, app: &tauri::AppHandle) -> Result<u64, String> {
    let resp = crate::net::open_stream(url, &[], None)?;
    let total = resp
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    if let Some(t) = total {
        if t > MAX_BYTES {
            return Err("组件体积异常，已中止".into());
        }
    }
    let tmp = target.with_extension("part");
    let mut file = fs::File::create(&tmp).map_err(|e| format!("无法写入组件：{}", e))?;
    let mut stream = resp;
    let mut done = 0u64;
    let mut last_emit = 0u64;
    let mut buf = [0u8; 1 << 16];
    loop {
        let n = stream.read(&mut buf).map_err(|e| format!("下载中断：{}", e))?;
        if n == 0 {
            break;
        }
        if done + n as u64 > MAX_BYTES {
            return Err("组件体积异常，已中止".into());
        }
        std::io::Write::write_all(&mut file, &buf[..n]).map_err(|e| format!("写入失败：{}", e))?;
        done += n as u64;
        if done - last_emit >= 131_072 {
            last_emit = done;
            emit_progress(app, "download", done, total, "正在准备联机组件…");
        }
    }
    std::io::Write::flush(&mut file).map_err(|e| format!("写入失败：{}", e))?;
    drop(file);
    let actual = sha256_of(&tmp)?;
    if actual != sha {
        let _ = fs::remove_file(&tmp);
        return Err("组件校验不通过，已丢弃".into());
    }
    fs::rename(&tmp, target).map_err(|e| format!("组件落位失败：{}", e))?;
    crate::download::note_host_success(url);
    Ok(done)
}

/// 读组件自报的版本与能力，纯本地调用。
pub fn probe(app: &tauri::AppHandle) -> Value {
    let target = binary_path(app);
    if !target.is_file() {
        return json!({ "ok": false, "error": "组件尚未就绪" });
    }
    match crate::vlcli::cmd::run(&target, &["--version".into()], std::time::Duration::from_secs(8)) {
        Ok(v) => v,
        Err(e) => json!({ "ok": false, "error": e.message }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sum_lines_in_both_styles() {
        let sums = "da1dee4d081a4d62e82455e857d08393a7fa47875c35f3cde1a0541435079ad8 *VoxLink-CLI-windows-amd64.exe\n\
                    1111111111111111111111111111111111111111111111111111111111111111  other.txt\n";
        assert_eq!(
            expected_sha(sums, ASSET).unwrap(),
            "da1dee4d081a4d62e82455e857d08393a7fa47875c35f3cde1a0541435079ad8"
        );
        assert_eq!(expected_sha(sums, "missing.exe"), None);
    }

    #[test]
    fn rejects_short_hash() {
        let sums = "abc  VoxLink-CLI-windows-amd64.exe\n";
        assert!(expected_sha(sums, ASSET).is_none());
    }

    #[test]
    fn matches_windows_path_in_sums() {
        let sums = "ab1dee4d081a4d62e82455e857d08393a7fa47875c35f3cde1a0541435079ad8  sub\\VoxLink-CLI-windows-amd64.exe\n";
        assert!(expected_sha(sums, ASSET).is_some());
    }
}
