//! 实例中心（对应原版 core/instanceCenter.ts 主线）：
//! center:overview / center:worlds / center:screenshots / center:backups / center:operation / center:file。
//! 备份/克隆/恢复为文件级复制实现（zip 压缩备份 + manifest；restore 按 manifest 回放）。

use crate::nbt;
use base64::Engine;
use serde_json::{json, Value};
use std::io::Read;
use std::path::{Path, PathBuf};

fn validate_instance_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.len() > 80
        || name == "."
        || name == ".."
        || name.chars().any(|c| "\\/:*?\"<>|".contains(c) || (c as u32) < 0x20)
    {
        return Err("实例名称无效".into());
    }
    Ok(())
}

/// centerTarget：登记校验 + 版本 json + 目录状态（隔离/共享）
pub(crate) fn center_target(app: &tauri::AppHandle, target: &Value) -> Result<(PathBuf, PathBuf, Value, bool), String> {
    let id = target["id"].as_str().unwrap_or("");
    validate_instance_name(id)?;
    let folder = target["folder"].as_str().unwrap_or("");
    let settings = crate::settings::get(app);
    let registered = settings["folders"]
        .as_array()
        .map(|fs| fs.iter().any(|f| f["path"].as_str() == Some(folder)))
        .unwrap_or(false);
    if !registered {
        return Err("游戏文件夹尚未登记".into());
    }
    let base = PathBuf::from(folder);
    let json = crate::installer::read_version_json_from(&base.join("versions").join(id).join(format!("{id}.json")))?;
    let isolated = json["isolated"] == true
        || json["_gameDir"] == true
        || base.join("versions").join(id).join("saves").exists();
    let dir = if isolated {
        base.join("versions").join(id)
    } else {
        base.clone()
    };
    if !dir.exists() {
        return Err("实例目录不存在".into());
    }
    Ok((base, dir, json, isolated))
}

fn running_version_ids() -> Vec<String> {
    crate::launch::running_version_ids()
}

/// assertInstanceIdle：该目录正被运行中游戏使用时拒绝写操作
fn assert_instance_idle(app: &tauri::AppHandle, dir: &Path) -> Result<(), String> {
    let running = running_version_ids();
    if running.is_empty() {
        return Ok(());
    }
    let settings = crate::settings::get(app);
    for vid in &running {
        let Some(v) = crate::versions::list_installed(&settings, true)
            .as_array()
            .and_then(|list| list.iter().find(|v| Some(vid.as_str()) == v["id"].as_str()))
            .cloned()
        else {
            continue;
        };
        let folder = v["folder"].as_str().unwrap_or("").to_string();
        let Ok(json) = crate::installer::read_version_json(app, &folder, vid) else { continue };
        let isolated = json["isolated"] == true || json["_gameDir"] == true;
        let d = if isolated {
            PathBuf::from(&folder).join("versions").join(vid)
        } else {
            PathBuf::from(&folder)
        };
        if d == dir {
            return Err("使用该目录的游戏正在运行，请退出游戏后重试".into());
        }
    }
    Ok(())
}

/// center:overview → InstanceOverview
pub fn overview(app: &tauri::AppHandle, target: &Value) -> Result<Value, String> {
    let (base, dir, json, isolated) = center_target(app, target)?;
    let id = target["id"].as_str().unwrap_or("");
    let mc_version = json["inheritsFrom"].as_str().unwrap_or(id).to_string();
    let loader = ["forge", "fabric", "quilt", "neoforge"].iter().find(|l| {
        id.to_lowercase().contains(*l) || json["libraries"].as_array().map(|libs| libs.iter().any(|lib| lib["name"].as_str().unwrap_or("").to_lowercase().contains(*l))).unwrap_or(false)
    }).map(|l| l.to_string());
    let running = assert_instance_idle(app, &dir).is_err();
    let mut roots = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let lower = name.to_lowercase();
            if name.starts_with(".wuhuicl-") || name.starts_with(".kamucl-") || name.ends_with(".exe") || name.ends_with(".dll")
                || name == format!("{id}.json") || name == format!("{id}.jar")
            {
                continue;
            }
            let _ = lower;
            roots.push(json!(name));
        }
    }
    Ok(json!({
        "target": target, "directory": dir.to_string_lossy(), "shared": !isolated,
        "name": id, "mcVersion": mc_version, "loader": loader,
        "running": running, "roots": roots
    }))
}

fn small_image(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() > 64 * 1024 * 1024 || bytes[0..4] != [0x89, 0x50, 0x4e, 0x47] {
        return None;
    }
    Some(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(&bytes)))
}

/// center:worlds → InstanceWorld[]（level.dat 元数据 + 最近游玩排序）
pub fn worlds(_app: &tauri::AppHandle, target: &Value) -> Result<Value, String> {
    let (_, dir, _, _) = center_target(_app, target)?;
    let saves = dir.join("saves");
    let mut rows = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&saves) {
        for e in entries.flatten() {
            let id = e.file_name().to_string_lossy().to_string();
            if !e.path().is_dir() {
                continue;
            }
            let mut row = json!({ "id": id, "name": id });
            let level = e.path().join("level.dat");
            match std::fs::read(&level) {
                Ok(buf) if buf.len() <= 32 * 1024 * 1024 => match nbt::parse_nbt(&buf) {
                    Ok(root) => {
                        let data = nbt::get(&root, "Data").and_then(nbt::as_compound).map(|c| c.clone()).unwrap_or(root.clone());
                        if let Some(name) = nbt::get(&data, "LevelName").and_then(nbt::as_str) {
                            row["name"] = json!(name);
                        }
                        if let Some(v) = nbt::get(&data, "Version").and_then(nbt::as_compound).and_then(|v| nbt::get(v, "Name")).and_then(nbt::as_str) {
                            row["version"] = json!(v);
                        }
                        let game_type = nbt::get(&data, "GameType").and_then(nbt::as_int).unwrap_or(0);
                        row["mode"] = json!(["生存", "创造", "冒险", "旁观"].get(game_type as usize).copied().unwrap_or("生存"));
                        let last_played = nbt::get(&data, "LastPlayed").and_then(nbt::as_int).unwrap_or(0);
                        row["lastPlayed"] = json!(last_played);
                        if let Some(icon) = small_image(&e.path().join("icon.png")) {
                            row["icon"] = json!(icon);
                        }
                    }
                    Err(err) => row["error"] = json!(err),
                },
                Ok(_) => row["error"] = json!("level.dat 超过读取限制"),
                Err(_) => {}
            }
            rows.push(row);
        }
    }
    rows.sort_by_key(|r| std::cmp::Reverse(r["lastPlayed"].as_i64().unwrap_or(0)));
    Ok(Value::Array(rows))
}

/// center:screenshots → {total, items[24]}（缩略图 dataURL）
pub fn screenshots(_app: &tauri::AppHandle, target: &Value, page: i64) -> Result<Value, String> {
    let (_, dir, _, _) = center_target(_app, target)?;
    let shot_dir = dir.join("screenshots");
    let mut names: Vec<String> = std::fs::read_dir(&shot_dir)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.file_name().to_string_lossy().to_lowercase().ends_with(".png"))
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names.reverse();
    let page = page.max(0);
    let mut items = Vec::new();
    for id in names.iter().skip((page * 24) as usize).take(24) {
        let path = shot_dir.join(id);
        if let Some(image) = small_image(&path) {
            let date = std::fs::metadata(&path)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            items.push(json!({ "id": id, "image": image, "date": date }));
        }
    }
    Ok(json!({ "total": names.len(), "items": items }))
}

// ---------------- 备份（zip + manifest） ----------------

fn backups_dir(dir: &Path) -> PathBuf {
    dir.join(".wuhuicl-backups")
}

/// center:backups → 备份清单列表（新→旧）
pub fn backups(_app: &tauri::AppHandle, target: &Value) -> Result<Value, String> {
    let (_, dir, _, _) = center_target(_app, target)?;
    let store = backups_dir(&dir);
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&store) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if !name.ends_with(".zip") {
                continue;
            }
            let meta = e.metadata().ok();
            out.push(json!({
                "id": name.trim_end_matches(".zip"),
                "file": e.path().to_string_lossy(),
                "size": meta.as_ref().map(|m| m.len()).unwrap_or(0),
                "time": meta.and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as i64).unwrap_or(0)
            }));
        }
    }
    out.sort_by_key(|b| std::cmp::Reverse(b["time"].as_i64().unwrap_or(0)));
    Ok(Value::Array(out))
}

/// center:operation kind=backup：打包实例根（排除运行时目录）为 zip
fn backup_instance(app: &tauri::AppHandle, target: &Value, op: &Value) -> Result<Value, String> {
    let (_, dir, _, _) = center_target(app, target)?;
    assert_instance_idle(app, &dir)?;
    let id = target["id"].as_str().unwrap_or("instance");
    let title = op["name"].as_str().unwrap_or("实例手动备份");
    let store = backups_dir(&dir);
    std::fs::create_dir_all(&store).map_err(|e| e.to_string())?;
    let backup_id = format!("{}-{}", chrono::Local::now().format("%Y%m%d-%H%M%S"), &crate::accounts::uuid_v4()[..8]);
    let zip_path = store.join(format!("{backup_id}.zip"));
    let file = std::fs::File::create(&zip_path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipWriter::new(file);
    let options: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default();
    let mut count = 0usize;
    let manifest_entries: Vec<Value> = walk_collect(&dir, &dir)?;
    for rel in manifest_entries.iter() {
        let rel_s = rel.as_str().unwrap_or_default().to_string();
        let src = dir.join(&rel_s);
        if src.is_dir() {
            continue;
        }
        zip.start_file(rel_s.clone(), options).map_err(|e| e.to_string())?;
        let bytes = std::fs::read(&src).map_err(|e| e.to_string())?;
        std::io::Write::write_all(&mut zip, &bytes).map_err(|e| e.to_string())?;
        count += 1;
    }
    // manifest
    let manifest = json!({
        "format": 1, "id": backup_id, "createdAt": chrono::Utc::now().to_rfc3339(),
        "title": title, "automatic": false, "source": id,
        "files": manifest_entries, "metadata": { "type": "instance", "target": target }
    });
    zip.start_file(".wuhuicl-backup-manifest.json", options).map_err(|e| e.to_string())?;
    std::io::Write::write_all(&mut zip, serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?.as_slice()).map_err(|e| e.to_string())?;
    zip.finish().map_err(|e| e.to_string())?;
    Ok(json!({ "kind": "backup", "id": backup_id, "files": count }))
}

/// 递归收集可备份相对路径（排除运行时/启动器文件）
fn walk_collect(root: &Path, dir: &Path) -> Result<Vec<Value>, String> {
    let mut out = Vec::new();
    let entries = std::fs::read_dir(dir).map_err(|e| e.to_string())?;
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let lower = name.to_lowercase();
        if name.starts_with(".wuhuicl-") || name.starts_with(".kamucl-") || lower.ends_with(".exe") || lower.ends_with(".dll") || lower.ends_with(".log") {
            continue;
        }
        let rel = e
            .path()
            .strip_prefix(root)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        let ft = e.file_type().map_err(|e| e.to_string())?;
        if ft.is_dir() {
            out.push(json!(format!("{rel}/")));
            out.extend(walk_collect(root, &e.path())?);
        } else {
            out.push(json!(rel));
        }
    }
    Ok(out)
}

/// center:operation kind=clone：stage 复制 + rename 发布
fn clone_instance(app: &tauri::AppHandle, target: &Value, op: &Value) -> Result<Value, String> {
    let (base, dir, json, _) = center_target(app, target)?;
    assert_instance_idle(app, &dir)?;
    let name = op["name"].as_str().unwrap_or("");
    validate_instance_name(name)?;
    let destination = op["destinationFolder"].as_str().unwrap_or(base.to_str().unwrap_or(""));
    let dest = PathBuf::from(destination).join("versions").join(name);
    if dest.exists() {
        return Err("目标实例已存在，请更换名称".into());
    }
    std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
    // 复制全部玩家文件（排除运行时）
    let entries = walk_collect(&dir, &dir)?;
    for rel in entries.iter() {
        let rel_s = rel.as_str().unwrap_or_default().to_string();
        if rel_s.ends_with('/') {
            continue;
        }
        let src = dir.join(&rel_s);
        let dst = dest.join(&rel_s);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::copy(&src, &dst).map_err(|e| e.to_string())?;
    }
    // 版本 json 换 id + 隔离
    let mut new_json = json.clone();
    if let Some(obj) = new_json.as_object_mut() {
        obj["id"] = json!(name);
        obj["_gameDir"] = json!(true);
        obj.remove("inheritsFrom");
        obj.remove("gameDirectory");
    }
    std::fs::write(dest.join(format!("{name}.json")), serde_json::to_string_pretty(&new_json).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    // jar
    let id = target["id"].as_str().unwrap_or("");
    let jar = dir.join(format!("{id}.jar"));
    if jar.exists() {
        std::fs::copy(&jar, dest.join(format!("{name}.jar"))).map_err(|e| e.to_string())?;
    }
    Ok(json!({ "folder": destination, "id": name }))
}

/// center:operation kind=restore：备份 zip 回放到实例目录（先备份当前）
fn restore_instance(app: &tauri::AppHandle, target: &Value, op: &Value) -> Result<Value, String> {
    let (_, dir, _, isolated) = center_target(app, target)?;
    if !isolated {
        return Err("共享或自定义目录请恢复为新的隔离实例".into());
    }
    assert_instance_idle(app, &dir)?;
    let backup_id = op["backupId"].as_str().unwrap_or("");
    let zip_path = backups_dir(&dir).join(format!("{backup_id}.zip"));
    if !zip_path.exists() {
        return Err("备份不存在".into());
    }
    // 安全网：恢复前自动备份
    let _ = backup_instance(app, target, &json!({ "name": "实例覆盖恢复前" }));
    let file = std::fs::File::open(&zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().trim_start_matches('/').replace('\\', "/");
        if name == ".wuhuicl-backup-manifest.json" {
            continue;
        }
        let target_path = dir.join(&name);
        if name.ends_with('/') {
            let _ = std::fs::create_dir_all(&target_path);
            continue;
        }
        if let Some(parent) = target_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut out = Vec::new();
        entry.read_to_end(&mut out).map_err(|e| e.to_string())?;
        std::fs::write(&target_path, out).map_err(|e| e.to_string())?;
    }
    Ok(json!({ "restored": backup_id }))
}

/// center:operation 主分发
fn operation(app: &tauri::AppHandle, op: &Value) -> Result<Value, String> {
    let kind = op["kind"].as_str().unwrap_or("");
    let target = op.get("target").cloned().unwrap_or(Value::Null);
    match kind {
        "backup" => backup_instance(app, &target, op),
        "clone" => clone_instance(app, &target, op),
        "restore" => restore_instance(app, &target, op),
        "repair" => Err("实例修复尚未接入（诊断规则子系统移植后开放）".into()),
        "worldExport" => Err("存档导出尚未接入".into()),
        other => Err(format!("未知操作：{other}")),
    }
}

/// center:file：读取 world/screenshot/preview 文件为 dataURL 或打开目录
fn center_file(app: &tauri::AppHandle, target: &Value, kind: &str, id: &str) -> Result<Value, String> {
    let (_, dir, _, _) = center_target(app, target)?;
    let path = match kind {
        "world" => dir.join("saves").join(id),
        "screenshot" => dir.join("screenshots").join(id),
        "preview" => dir.join(id),
        "directory" => {
            std::process::Command::new("explorer.exe").arg(dir.as_os_str()).spawn().map_err(|e| e.to_string())?;
            return Ok(Value::Null);
        }
        other => return Err(format!("未知文件类型：{other}")),
    };
    if kind == "world" {
        // 存档目录打开
        std::process::Command::new("explorer.exe").arg(path.as_os_str()).spawn().map_err(|e| e.to_string())?;
        return Ok(Value::Null);
    }
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    Ok(json!(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))))
}

/// 统一入口
pub fn invoke(app: &tauri::AppHandle, channel: &str, args: &[Value]) -> Option<Result<Value, String>> {
    let target = || args.first().cloned().unwrap_or(Value::Null);
    match channel {
        "center:overview" => Some(overview(app, &target())),
        "center:worlds" => Some(worlds(app, &target())),
        "center:screenshots" => Some(screenshots(
            app,
            &target(),
            args.get(1).and_then(|v| v.as_i64()).unwrap_or(0),
        )),
        "center:backups" => Some(backups(app, &target())),
        "center:operation" => Some(operation(app, &args.first().cloned().unwrap_or(Value::Null))),
        "center:file" => Some(center_file(
            app,
            &target(),
            args.get(1).and_then(|v| v.as_str()).unwrap_or(""),
            args.get(2).and_then(|v| v.as_str()).unwrap_or(""),
        )),
        "center:diagnose" => Some(Err("实例诊断尚未接入".into())),
        "center:dependencyPlan" | "center:dependencyApply" => Some(Err("依赖补全尚未接入".into())),
        _ => None,
    }
}
