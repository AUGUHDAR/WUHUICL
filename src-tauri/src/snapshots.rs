//! 实例快照（「后悔药」）：把某个实例的 mods / 资源包 / 光影整体打包成一个 zip，
//! 之后可以整体回滚到那一刻的样子。
//!
//! 设计取舍：
//! - **单文件 zip** 而不是复制成目录：玩家动辄几百个 mod，复制成目录会产生上千个
//!   小文件（慢、占 inode、备份目录本身也会变成新的混乱来源）。zip 一个文件、
//!   按 sha1 记清单，体积还能靠 deflate 压一部分。
//! - **手动 + 自动**两条来源：自动快照只在「大批量变更前」触发（安装整合包、
//!   批量更新、大版本安装），平时不做，避免每装一个 mod 就多一份快照。
//! - **回滚是可逆的**：回滚前先给当前状态再拍一张快照，玩家可以「后悔自己的后悔」。
//! - 快照只覆盖资源目录（mods/resourcepacks/shaderpacks），**不碰存档与配置**——
//!   存档体积大且不可重建，混进来会让快照从「几百 MB」变成「几个 GB」。

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// 快照覆盖的资源目录种类；与 mods.rs 的 resolve_resource_directory 支持的集合一致
pub const SNAPSHOT_KINDS: &[&str] = &["mods", "resourcepacks", "shaderpacks"];
/// 单个快照上限：超过就拒绝并提示，避免一个失误把磁盘写满
const MAX_SNAPSHOT_BYTES: u64 = 4 * 1024 * 1024 * 1024;
/// 保留的快照数量上限（超出后按时间淘汰最旧的自动快照，手动快照不淘汰）
const MAX_SNAPSHOTS: usize = 40;

fn snapshots_root(app: &tauri::AppHandle) -> PathBuf {
    crate::settings::user_data_dir(app).join("snapshots")
}

/// 实例在快照目录里的稳定 key：folder + version id 可能含路径分隔符与中文，
/// 用哈希做目录名，真实标识写在 meta.json 里。
fn instance_key(folder: &str, id: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(folder.as_bytes());
    h.update(b"\0");
    h.update(id.as_bytes());
    let d = h.finalize();
    d.iter().take(12).map(|b| format!("{b:02x}")).collect()
}

fn instance_dir(app: &tauri::AppHandle, folder: &str, id: &str) -> PathBuf {
    snapshots_root(app).join(instance_key(folder, id))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 目录里参与快照的文件：跳过子目录里的临时/隐藏内容，但保留 `.jar.disabled`
/// （禁用状态本身就是玩家要回滚的东西之一）。
fn collect_files(dir: &Path) -> Vec<(String, PathBuf, u64)> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        // 跳过明确的临时残留
        if name.ends_with(".part") || name.ends_with(".tmp") || name.starts_with(".~") {
            continue;
        }
        out.push((name.to_string(), path, meta.len()));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn sha1_of(path: &Path) -> Option<String> {
    use sha1::{Digest, Sha1};
    let mut f = fs::File::open(path).ok()?;
    let mut h = Sha1::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Some(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// 拍一张快照。`reason` 只作文案展示（手动 / 安装整合包 / 批量更新…），
/// `auto` 区分自动与手动，用于后续淘汰策略。
pub fn create(
    app: &tauri::AppHandle,
    folder: &str,
    id: &str,
    name: &str,
    reason: &str,
    auto: bool,
) -> Result<Value, String> {
    if folder.is_empty() || id.is_empty() {
        return Err("请先选择游戏实例".into());
    }
    // 各资源目录的现状清单
    let mut per_kind: Vec<(String, Vec<(String, PathBuf, u64)>)> = Vec::new();
    let mut total_bytes = 0u64;
    for kind in SNAPSHOT_KINDS {
        let Ok(dir) = crate::mods::resolve_resource_directory(app, folder, id, kind) else {
            continue;
        };
        if !dir.exists() {
            continue;
        }
        let files = collect_files(&dir);
        if files.is_empty() {
            continue;
        }
        total_bytes += files.iter().map(|(_, _, n)| *n).sum::<u64>();
        per_kind.push(((*kind).to_string(), files));
    }
    if per_kind.is_empty() {
        return Err("这个实例的模组 / 资源包 / 光影目录都是空的，没有可备份的内容".into());
    }
    if total_bytes > MAX_SNAPSHOT_BYTES {
        return Err(format!(
            "内容过大（约 {:.1} GB），超过单个快照上限",
            total_bytes as f64 / 1024.0 / 1024.0 / 1024.0
        ));
    }

    let dir_out = instance_dir(app, folder, id);
    fs::create_dir_all(&dir_out).map_err(|e| e.to_string())?;
    let created = now_ms();
    let snap_id = format!("{created}");
    let zip_path = dir_out.join(format!("{snap_id}.zip"));

    // 打包：zip 里用 相对路径(mods/xxx.jar) 存，回滚时据此还原
    let file = fs::File::create(&zip_path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipWriter::new(file);
    let opts: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);

    let mut manifest: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for (kind, files) in &per_kind {
        let mut entries = Vec::new();
        for (file_name, path, size) in files {
            let arcname = format!("{kind}/{file_name}");
            if let Ok(mut src) = fs::File::open(path) {
                if zip.start_file(arcname.clone(), opts).is_ok() {
                    let mut buf = [0u8; 64 * 1024];
                    loop {
                        match src.read(&mut buf) {
                            Ok(0) => break,
                            Ok(n) => {
                                if zip.write_all(&buf[..n]).is_err() {
                                    break;
                                }
                            }
                            Err(_) => break,
                        }
                    }
                }
            }
            entries.push(json!({
                "name": file_name,
                "size": size,
                "sha1": sha1_of(path).unwrap_or_default()
            }));
        }
        manifest.insert((*kind).clone(), entries);
    }
    zip.finish().map_err(|e| e.to_string())?;

    let meta = json!({
        "id": snap_id,
        "createdAt": created,
        "reason": reason,
        "auto": auto,
        "folder": folder,
        "versionId": id,
        "instanceName": name,
        "bytes": total_bytes,
        "zipBytes": fs::metadata(&zip_path).map(|m| m.len()).unwrap_or(0),
        "counts": per_kind.iter().map(|(k, f)| (k.clone(), json!(f.len()))).collect::<BTreeMap<String, Value>>(),
        "manifest": manifest
    });
    let meta_path = dir_out.join(format!("{snap_id}.json"));
    fs::write(&meta_path, serde_json::to_vec_pretty(&meta).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;

    prune(app, folder, id);
    Ok(meta)
}

/// 淘汰：自动快照超过上限时按时间删最旧的；手动快照始终保留。
fn prune(app: &tauri::AppHandle, folder: &str, id: &str) {
    let Ok(list) = list(app, folder, id) else { return };
    let arr = list.as_array().cloned().unwrap_or_default();
    let mut autos: Vec<&Value> = arr.iter().filter(|m| m["auto"] == true).collect();
    if autos.len() <= MAX_SNAPSHOTS {
        return;
    }
    autos.sort_by_key(|m| m["createdAt"].as_u64().unwrap_or(0));
    let dir = instance_dir(app, folder, id);
    let excess = autos.len() - MAX_SNAPSHOTS;
    for meta in autos.into_iter().take(excess) {
        if let Some(sid) = meta["id"].as_str() {
            let _ = fs::remove_file(dir.join(format!("{sid}.zip")));
            let _ = fs::remove_file(dir.join(format!("{sid}.json")));
        }
    }
}

/// 某实例的全部快照（新的在前）
pub fn list(app: &tauri::AppHandle, folder: &str, id: &str) -> Result<Value, String> {
    let dir = instance_dir(app, folder, id);
    let mut out: Vec<Value> = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Ok(raw) = fs::read(&path) else { continue };
            let Ok(mut meta) = serde_json::from_slice::<Value>(&raw) else {
                continue;
            };
            // 清单体积大（几百个文件），列表只给摘要
            if let Some(obj) = meta.as_object_mut() {
                obj.remove("manifest");
                // zip 还在不在，前端据此决定「回滚」按钮是否可用
                let sid = obj.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let zip_ok = dir.join(format!("{sid}.zip")).exists();
                obj.insert("available".into(), json!(zip_ok));
            }
            out.push(meta);
        }
    }
    out.sort_by_key(|m| std::cmp::Reverse(m["createdAt"].as_u64().unwrap_or(0)));
    Ok(json!(out))
}

/// 回滚到指定快照。
/// 顺序：先给"当前状态"拍一张（这样回滚本身也可撤销）→ 清空目标目录 → 从 zip 还原。
/// `kinds` 为空表示按快照里实际包含的种类全量还原。
pub fn restore(
    app: &tauri::AppHandle,
    folder: &str,
    id: &str,
    snapshot_id: &str,
    name: &str,
) -> Result<Value, String> {
    let dir = instance_dir(app, folder, id);
    let meta_path = dir.join(format!("{snapshot_id}.json"));
    let zip_path = dir.join(format!("{snapshot_id}.zip"));
    if !meta_path.exists() || !zip_path.exists() {
        return Err("这个快照文件已丢失，无法回滚".into());
    }
    let meta: Value = serde_json::from_slice(&fs::read(&meta_path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;

    // 回滚前先备份当前状态：让「回滚」这一步本身也可撤销
    let safety = create(
        app,
        folder,
        id,
        name,
        &format!("回滚前自动备份（回滚到 {}）", &snapshot_id[..snapshot_id.len().min(13)]),
        true,
    );
    let safety_id = safety.as_ref().ok().and_then(|m| m["id"].as_str()).map(str::to_string);

    let file = fs::File::open(&zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;

    // 目标目录集合：快照里出现过的 kind
    let kinds: Vec<String> = meta["manifest"]
        .as_object()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();

    let mut restored = 0usize;
    for kind in &kinds {
        let Ok(target) = crate::mods::resolve_resource_directory(app, folder, id, kind) else {
            continue;
        };
        fs::create_dir_all(&target).map_err(|e| e.to_string())?;
        // 清掉现有文件（只清文件，不动子目录，避免误删显存缓存之类的结构目录）
        if let Ok(entries) = fs::read_dir(&target) {
            for entry in entries.flatten() {
                if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
            let Some(arc) = entry.enclosed_name() else {
                continue;
            };
            let arc_str = arc.to_string_lossy().replace('\\', "/");
            let Some(rest) = arc_str.strip_prefix(&format!("{kind}/")) else {
                continue;
            };
            if rest.is_empty() || rest.contains('/') {
                continue;
            }
            let dest = target.join(rest);
            let mut out = fs::File::create(&dest).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
            restored += 1;
        }
    }
    Ok(json!({
        "ok": true,
        "restored": restored,
        "kinds": kinds,
        "safetySnapshot": safety_id
    }))
}

/// 删除一个快照文件（手动清理磁盘用）
pub fn delete(app: &tauri::AppHandle, folder: &str, id: &str, snapshot_id: &str) -> Result<Value, String> {
    let dir = instance_dir(app, folder, id);
    let _ = fs::remove_file(dir.join(format!("{snapshot_id}.zip")));
    let _ = fs::remove_file(dir.join(format!("{snapshot_id}.json")));
    Ok(json!({ "ok": true }))
}

// ---------------- 通道分发 ----------------

pub fn invoke(app: &tauri::AppHandle, channel: &str, args: &[Value]) -> Option<Result<Value, String>> {
    let s = |i: usize| args.get(i).and_then(|v| v.as_str()).unwrap_or("").to_string();
    match channel {
        "snapshots:list" => Some(list(app, &s(0), &s(1))),
        "snapshots:create" => {
            let auto = args.get(4).and_then(|v| v.as_bool()).unwrap_or(false);
            Some(crate::installer::run_blocking(|| {
                create(app, &s(0), &s(1), &s(2), &s(3), auto)
            }))
        }
        "snapshots:restore" => Some(crate::installer::run_blocking(|| {
            restore(app, &s(0), &s(1), &s(2), &s(3))
        })),
        "snapshots:delete" => Some(delete(app, &s(0), &s(1), &s(2))),
        _ => None,
    }
}
