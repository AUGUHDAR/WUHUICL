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
    // 截图/图标原图可达数十 MB；24 张 base64 直传会把 IPC 响应顶到 2-4GB（实测 OOM 崩溃源）。
    // 统一解码 → 等比缩到宽 320 → 重编码 PNG，单张输出通常 <200KB。
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() > 64 * 1024 * 1024 || bytes.len() < 8 || bytes[0..4] != [0x89, 0x50, 0x4e, 0x47] {
        return None;
    }
    let decoded = image::load_from_memory(&bytes).ok()?;
    let thumb = decoded.thumbnail(320, 320);
    let mut out = std::io::Cursor::new(Vec::new());
    thumb.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(out.into_inner())
    ))
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
        "repair" => repair_instance(app, &target, op),
        "worldExport" => world_export(app, &target, op),
        other => Err(format!("未知操作：{other}")),
    }
}

/// center:operation kind=worldExport：存档目录打包 zip 导出（保存对话框选目标；
/// 已存在的目标文件按原版直接拒绝，不覆盖）。对应原版 instanceCenterIpc.ts worldExport 分支。
fn world_export(app: &tauri::AppHandle, target: &Value, op: &Value) -> Result<Value, String> {
    let world = op["world"].as_str().ok_or("缺少存档名")?;
    validate_instance_name(world)?;
    let (_, dir, _, _) = center_target(app, target)?;
    crate::mods::assert_instance_idle(app, &dir)?;
    let source = dir.join("saves").join(world);
    if !source.is_dir() {
        return Err("存档不存在".into());
    }
    use tauri_plugin_dialog::DialogExt;
    let picked = app
        .dialog()
        .file()
        .set_title("导出存档")
        .set_file_name(format!("{world}.zip"))
        .add_filter("ZIP 压缩包", &["zip"])
        .blocking_save_file();
    let Some(picked) = picked else {
        return Ok(Value::Null); // 用户取消：与原版返回 null 一致
    };
    let dest = std::path::PathBuf::from(picked.to_string());
    if dest.exists() {
        return Err("导出文件已存在，请选择新文件名".into());
    }
    // 与原版 task('导出存档') 一致：注册任务、打包、收尾后返回导出路径
    let rec = crate::tasks::register_task("导出存档", "world");
    let result = (|| -> Result<Value, String> {
        crate::tasks::emit_progress(
            app,
            json!({ "taskId": rec.id, "taskTitle": "导出存档", "stage": "world", "progress": 0.0, "text": "准备任务…" }),
        );
        let file = std::fs::File::create(&dest).map_err(|e| e.to_string())?;
        let mut zip = zip::ZipWriter::new(file);
        let options: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default();
        // 存档导出为目录全量打包（exportTreeZip 语义：不过滤）
        let mut stack = vec![source.clone()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).map_err(|e| e.to_string())?.flatten() {
                let path = e.path();
                let rel = path.strip_prefix(&source).map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default();
                if rel.is_empty() {
                    continue;
                }
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.is_symlink() {
                    return Err("不支持导出包含符号链接的存档".into());
                }
                zip.start_file(rel, options).map_err(|e| e.to_string())?;
                let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
                std::io::Write::write_all(&mut zip, &bytes).map_err(|e| e.to_string())?;
            }
        }
        zip.finish().map_err(|e| e.to_string())?;
        Ok(json!(dest.to_string_lossy().to_string()))
    })();
    let ok = result.is_ok();
    crate::tasks::task_done(app, &rec.id, ok, result.as_ref().err().cloned(), false, None);
    crate::tasks::finish_task(&rec.id);
    result
}

// ---------------- 诊断与修复（instanceDiagnostics.ts + diagnosticRules.ts） ----------------

/// diagnosticRules.ts 的 11 条规则（正则逐字移植）
struct DiagRule {
    rule: &'static str,
    title: &'static str,
    pattern: &'static str,
    advice: &'static str,
    action: Option<&'static str>,
    confidence: &'static str,
}

const DIAG_RULES: &[DiagRule] = &[
    DiagRule { rule: "java-version", title: "Java 版本不兼容", pattern: r"(?i)UnsupportedClassVersionError|class file version .*only recognizes|requires Java (?:version )?\d+", advice: "选择该实例要求的 Java 版本后重新启动。", action: Some("java"), confidence: "certain" },
    DiagRule { rule: "java-argument", title: "Java 启动参数无效", pattern: r"(?i)Unrecognized VM option|Unrecognized option:|Invalid maximum heap size|Improperly specified VM option", advice: "检查版本设置中的 JVM 参数，只修改报错指出的参数。", action: Some("java"), confidence: "certain" },
    DiagRule { rule: "memory", title: "内存分配或使用失败", pattern: r"(?i)OutOfMemoryError|Could not reserve enough space|Native memory allocation .*failed|There is insufficient memory", advice: "检查可用内存与实例分配；无法预留时适当降低分配，堆内存耗尽时结合系统余量调整。", action: None, confidence: "certain" },
    DiagRule { rule: "dependency", title: "加载器报告模组依赖不满足", pattern: r"(?i)requires .*which is missing|requires .*which is not installed|Missing mandatory dependencies|ModResolutionException|Incompatible mod set", advice: "打开模组管理核对加载器指出的前置与版本范围，再从社区安装适配版本。", action: Some("mods"), confidence: "certain" },
    DiagRule { rule: "duplicate", title: "加载器报告重复模组", pattern: r"(?i)DuplicateModsFoundException|Duplicate mod(?:s| ID)?(?: found|:)|Found duplicate mods", advice: "打开模组管理中的清理重复，核对文件后确认处理。", action: Some("mods"), confidence: "certain" },
    DiagRule { rule: "mod-version", title: "加载器报告模组版本不兼容", pattern: r"(?i)requires.*minecraft.*but|requires.*version.*but only|incompatible with.*version", advice: "按日志给出的游戏版本与加载器要求切换模组版本。", action: Some("mods"), confidence: "certain" },
    DiagRule { rule: "archive", title: "运行文件可能损坏", pattern: r"(?i)ZipException:.*(?:zip END header|error in opening zip|invalid)|Invalid or corrupt jarfile", advice: "检查游戏本体和运行库；模组文件请通过模组管理重新下载。", action: Some("files"), confidence: "possible" },
    DiagRule { rule: "permission", title: "文件无法访问或被占用", pattern: r"(?i)AccessDeniedException|Permission denied|used by another process|being used by another process", advice: "关闭占用该文件的程序并检查目录权限，再重试；不要删除存档锁文件。", action: None, confidence: "certain" },
    DiagRule { rule: "disk", title: "磁盘空间不足", pattern: r"(?i)No space left on device|There is not enough space on the disk|ENOSPC", advice: "释放游戏目录和临时目录所在磁盘空间，再重新执行。", action: None, confidence: "certain" },
    DiagRule { rule: "graphics", title: "图形环境初始化失败", pattern: r"(?i)GLFW error \d+|Failed to create OpenGL context|Pixel format not accelerated|does not support OpenGL", advice: "检查显卡驱动及游戏版本要求，使用显卡厂商官方驱动下载入口。", action: None, confidence: "certain" },
    DiagRule { rule: "native-arch", title: "本地库或 Java 架构可能不匹配", pattern: r"(?i)wrong architecture|incompatible architecture|Can't load AMD 64-bit|mach-o.*(?:arm64|x86_64)|Bad CPU type in executable", advice: "确认 Java 与当前系统架构匹配，并重新校验运行库。", action: Some("java"), confidence: "possible" },
];

/// analyzeDiagnosticText()：脱敏后逐规则首命中行，evidence 取命中行前 1 行至后 4 行（≤2000 字符）
fn analyze_diagnostic_text(text: &str) -> Vec<Value> {
    let redacted = crate::exportlogs::redact_text(text, &[]);
    let lines: Vec<&str> = redacted.split(['\r', '\n']).filter(|l| !l.is_empty()).collect();
    let mut out = Vec::new();
    for rule in DIAG_RULES {
        let Ok(re) = regex::Regex::new(rule.pattern) else { continue };
        let Some(index) = lines.iter().position(|l| re.is_match(l)) else { continue };
        let start = index.saturating_sub(1);
        let end = (index + 4).min(lines.len());
        let evidence: String = lines[start..end].join("\n").chars().take(2000).collect();
        out.push(json!({
            "rule": rule.rule, "title": rule.title, "confidence": rule.confidence,
            "evidence": evidence, "advice": rule.advice, "action": rule.action,
        }));
    }
    if out.is_empty() && !text.trim().is_empty() {
        out.push(json!({
            "rule": "unknown", "title": "现有日志不足以确定原因", "confidence": "unknown",
            "evidence": "未发现首批诊断规则可确认的错误。", "advice": "导出诊断日志进一步检查；不会据此禁用或删除模组。",
        }));
    }
    out
}

struct DiagnosePlan {
    target: Value,
    /// 坏文件（带下载地址的）: {dest, url, sha1, size}
    files: Vec<Value>,
    metadata: String,
    time: u64,
}

fn diagnose_plans() -> &'static std::sync::Mutex<std::collections::HashMap<String, DiagnosePlan>> {
    static P: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, DiagnosePlan>>> = std::sync::OnceLock::new();
    P.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// 诊断中单个待校验文件：坏则返回描述条目，完好返回 None
fn verify_artifact(
    dest: &Path,
    url: Option<&str>,
    sha1: Option<&str>,
    size: Option<u64>,
) -> Result<Option<Value>, String> {
    let expected = crate::net::Integrity {
        sha1: sha1.map(str::to_string),
        sha512: None,
        sha256: None,
        size,
    };
    match crate::net::verify_file(dest, &expected, None) {
        Ok(None) => Ok(None),
        Ok(Some(reason)) => Ok(Some(json!({
            "dest": dest.to_string_lossy(), "url": url, "sha1": sha1, "size": size, "reason": reason,
        }))),
        Err(e) => Err(e),
    }
}

/// center:diagnose：检查实例运行环境（Java 需求/指定 Java、运行文件完整性、最近一次启动日志规则分析）。
/// 返回 {id, findings, java, session, requiredJava}；id 供 repair 使用（30 分钟有效）。
fn diagnose(app: &tauri::AppHandle, target: &Value) -> Result<Value, String> {
    let rec = crate::tasks::register_task("检查实例运行环境", "world");
    let result = diagnose_inner(app, target, &rec);
    let ok = result.is_ok();
    crate::tasks::task_done(app, &rec.id, ok, result.as_ref().err().cloned(), false, None);
    crate::tasks::finish_task(&rec.id);
    result
}

fn diagnose_inner(app: &tauri::AppHandle, target: &Value, rec: &std::sync::Arc<crate::tasks::TaskRecord>) -> Result<Value, String> {
    let (base, dir, instance_json, _) = center_target(app, target)?;
    let id = target["id"].as_str().unwrap_or("");
    let folder = base.to_string_lossy().to_string();
    let merged = crate::installer::read_version_json(app, &folder, id)?;
    let mc_version = merged.get("id").and_then(|v| v.as_str()).unwrap_or(id).to_string();
    let mut findings: Vec<Value> = Vec::new();
    let emit = |done: usize, total: usize, text: &str| {
        crate::tasks::emit_progress(
            app,
            json!({ "taskId": rec.id, "taskTitle": "检查实例运行环境", "stage": "world",
                    "progress": if total > 0 { done as f64 / total as f64 } else { 0.0 }, "text": text }),
        );
    };
    emit(0, 1, "准备任务…");

    // 1) Java 需求与候选（resolveJavaRequirement + listJavaSummary 过滤兼容）
    let requirement = crate::launch::resolve_java_requirement(&merged, &mc_version, &instance_json);
    let need = match &requirement {
        Ok((major, _)) => Some(*major),
        Err(_) => None,
    };
    if let Err(e) = &requirement {
        findings.push(json!({
            "rule": "java-requirement", "title": "无法确认 Java 需求", "confidence": "unknown",
            "evidence": crate::exportlogs::redact_text(e, &[]),
            "advice": "补全官方游戏版本元数据，或检查模组声明的 Java 需求。", "action": "java",
        }));
    }
    let mut java: Vec<Value> = Vec::new();
    if let Some(need_major) = need {
        for candidate in crate::javaman::scan_quick(app, false) {
            let major = candidate.get("major").and_then(|v| v.as_i64()).unwrap_or(0);
            if major >= need_major {
                java.push(candidate);
            }
        }
    }
    // 指定 Java 校验（实例 _javaAuto/_javaPath，否则全局 javaAuto/javaPath）
    let s = crate::settings::get(app);
    let configured = if instance_json["_javaAuto"] == json!(true) {
        None
    } else {
        instance_json["_javaPath"].as_str().map(str::to_string).or_else(|| {
            if s["javaAuto"] == json!(false) {
                s["javaPath"].as_str().map(str::to_string)
            } else {
                None
            }
        })
    };
    match (&configured, need) {
        (Some(path), _) if !Path::new(path).exists() => {
            findings.push(json!({
                "rule": "java-missing", "title": "指定的 Java 已不存在", "confidence": "certain",
                "evidence": crate::exportlogs::redact_text(path, &[]),
                "advice": need.map(|n| format!("为本实例选择 Java {n}。")).unwrap_or_else(|| "重新选择有效的 Java 路径。".into()),
                "action": "java",
            }));
        }
        (Some(path), Some(_)) => match crate::javaman::probe(app, path) {
            Some(info) => {
                let major = info.get("major").and_then(|v| v.as_i64()).unwrap_or(0);
                if let Some(need_major) = need {
                    if major < need_major {
                        findings.push(json!({
                            "rule": "java-selection", "title": "指定 Java 与实例需求不一致", "confidence": "certain",
                            "evidence": format!("指定 Java {}（Java {}），实例推荐 Java {need_major}", crate::exportlogs::redact_text(path, &[]), major),
                            "advice": "选择满足游戏、加载器与模组要求的完整 Java，或开启自动管理。", "action": "java",
                        }));
                    }
                }
            }
            None => {
                findings.push(json!({
                    "rule": "java-selection", "title": "指定 Java 与实例需求不一致", "confidence": "certain",
                    "evidence": "指定 Java 无法正常运行", "advice": "选择满足游戏、加载器与模组要求的完整 Java，或开启自动管理。", "action": "java",
                }));
            }
        },
        (None, Some(_)) if java.is_empty() => {
            findings.push(json!({
                "rule": "java-unavailable", "title": "未发现已登记的兼容 Java", "confidence": "possible",
                "evidence": format!("实例推荐：Java {}（64 位）", need.unwrap_or(0)),
                "advice": "在 Java 管理中扫描或安装适配运行时；自动管理将准备推荐版本。", "action": "java",
            }));
        }
        _ => {}
    }

    // 2) 运行文件完整性：client jar + libraries + asset index
    let mut files: Vec<(PathBuf, Option<String>, Option<String>, Option<u64>)> = Vec::new(); // dest/url/sha1/size
    let client = merged.get("downloads").and_then(|d| d.get("client")).cloned().unwrap_or(Value::Null);
    if !client.is_null() {
        let dest = crate::installer::client_jar_path(app, &folder, id);
        files.push((
            dest,
            client.get("url").and_then(|v| v.as_str()).map(str::to_string),
            client.get("sha1").and_then(|v| v.as_str()).map(str::to_string),
            client.get("size").and_then(|v| v.as_u64()),
        ));
    }
    for lib in crate::installer::collect_libraries(&merged) {
        files.push((
            crate::installer::libraries_dir(app).join(&lib.rel),
            lib.url,
            lib.sha1,
            lib.size,
        ));
    }
    let assets = crate::installer::assets_dir(app);
    let asset_index = merged.get("assetIndex").cloned().unwrap_or(Value::Null);
    let index_id = asset_index.get("id").and_then(|v| v.as_str()).unwrap_or("");
    if !index_id.is_empty()
        && regex::Regex::new(r"^[\w.-]+$").unwrap().is_match(index_id)
        && index_id != "."
        && index_id != ".."
    {
        files.push((
            assets.join("indexes").join(format!("{index_id}.json")),
            asset_index.get("url").and_then(|v| v.as_str()).map(str::to_string),
            asset_index.get("sha1").and_then(|v| v.as_str()).map(str::to_string),
            asset_index.get("size").and_then(|v| v.as_u64()),
        ));
    }
    let total = files.len();
    let mut bad: Vec<Value> = Vec::new();
    let mut files_count = findings.iter().filter(|f| f["rule"] == json!("files")).count();
    for (i, (dest, url, sha1, size)) in files.iter().enumerate() {
        if rec.cancelled.load(std::sync::atomic::Ordering::SeqCst) {
            return Err("已取消".into());
        }
        if let Some(bad_entry) = verify_artifact(dest, url.as_deref(), sha1.as_deref(), *size)? {
            bad.push(bad_entry.clone());
            if files_count < 10 {
                let name = dest.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
                findings.push(json!({
                    "rule": "files", "title": "运行文件缺失或损坏", "confidence": "certain",
                    "evidence": format!("{}：{}", name, bad_entry["reason"].as_str().unwrap_or("")),
                    "advice": if url.is_some() { "校验并修复运行文件。" } else { "元数据缺少下载地址，请重新安装对应加载器。" },
                    "action": if url.is_some() { json!("files") } else { Value::Null },
                }));
                files_count += 1;
            }
        }
        emit(i + 1, total, "检查运行文件");
    }
    // asset objects（index 完好才查；每 64 个报一次进度）
    if !index_id.is_empty()
        && !bad
            .iter()
            .any(|b| b["dest"].as_str().map(|d| d.ends_with(&format!("{index_id}.json"))).unwrap_or(false))
    {
        let index_path = assets.join("indexes").join(format!("{index_id}.json"));
        if let Ok(raw) = std::fs::read_to_string(&index_path) {
            if let Ok(index) = serde_json::from_str::<Value>(&raw) {
                if let Some(objects) = index.get("objects").and_then(|v| v.as_object()) {
                    let list: Vec<(&String, &Value)> = objects.iter().collect();
                    let hash_re = regex::Regex::new(r"^[a-f0-9]{40}$").unwrap();
                    for (i, (_, o)) in list.iter().enumerate() {
                        if rec.cancelled.load(std::sync::atomic::Ordering::SeqCst) {
                            return Err("已取消".into());
                        }
                        let hash = o.get("hash").and_then(|v| v.as_str()).unwrap_or("");
                        if !hash_re.is_match(hash) {
                            continue;
                        }
                        let dest = assets.join("objects").join(&hash[..2]).join(hash);
                        let url = format!("https://resources.download.minecraft.net/{}/{}", &hash[..2], hash);
                        if let Some(bad_entry) = verify_artifact(&dest, Some(&url), Some(hash), o.get("size").and_then(|v| v.as_u64()))? {
                            bad.push(bad_entry);
                            if files_count < 10 {
                                findings.push(json!({
                                    "rule": "files", "title": "运行文件缺失或损坏", "confidence": "certain",
                                    "evidence": format!("{hash}：资源对象缺失",),
                                    "advice": "校验并修复运行文件。", "action": "files",
                                }));
                                files_count += 1;
                            }
                        }
                        if i % 64 == 0 {
                            emit(i, list.len(), "检查语言与声音资源");
                        }
                    }
                }
            }
        }
    }

    // 3) 最近一次启动会话日志 → 规则分析（只看本实例本目录的会话）
    let history = crate::misc::load_exit_history(app);
    let dir_identity = crate::installer::path_identity(&dir.to_string_lossy());
    let session_ctx = history
        .iter()
        .filter(|e| e.get("kind").and_then(|v| v.as_str()) == Some("game"))
        .filter_map(|e| e.get("context"))
        .filter(|c| {
            c.get("versionId").and_then(|v| v.as_str()) == Some(id)
                && c
                    .get("effectiveGameDir")
                    .and_then(|v| v.as_str())
                    .map(|d| crate::installer::path_identity(d) == dir_identity)
                    .unwrap_or(false)
                && c
                    .get("logDir")
                    .and_then(|v| v.as_str())
                    .map(|d| {
                        let base = Path::new(d).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                        regex::Regex::new(r"^[a-f0-9-]{36}$").unwrap().is_match(&base)
                    })
                    .unwrap_or(false)
        })
        .cloned()
        .max_by_key(|c| c.get("startedAt").and_then(|v| v.as_str()).and_then(|s| s.parse::<i64>().ok()).unwrap_or(0));
    let mut session: Option<String> = None;
    match session_ctx {
        Some(ctx) => {
            let log_dir = Path::new(ctx.get("logDir").and_then(|v| v.as_str()).unwrap_or(""));
            let mut text = String::new();
            for name in ["stdout.log", "stderr.log"] {
                if let Ok(meta) = std::fs::metadata(log_dir.join(name)) {
                    let take = meta.len().min(2 * 1024 * 1024) as usize;
                    if let Ok(mut f) = std::fs::File::open(log_dir.join(name)) {
                        use std::io::{Read, Seek, SeekFrom};
                        let skip = meta.len() as usize - take;
                        if f.seek(SeekFrom::Start(skip as u64)).is_ok() {
                            let mut buf = vec![0u8; take];
                            if f.read(&mut buf).is_ok() {
                                text.push('\n');
                                text.push_str(&String::from_utf8_lossy(&buf));
                            }
                        }
                    }
                }
            }
            findings.extend(analyze_diagnostic_text(&text));
            session = ctx.get("startedAt").and_then(|v| v.as_str()).map(str::to_string);
        }
        None => {
            findings.push(json!({
                "rule": "no-session", "title": "暂无该实例的启动日志", "confidence": "unknown",
                "evidence": "只展示本次环境检查结果，未读取其他实例或旧目录的日志。",
                "advice": "下次通过本启动器启动失败后可查看对应会话原因。",
            }));
        }
    }

    // 4) 缓存修复计划（坏文件中带 url 的）并返回
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    {
        let mut plans = diagnose_plans().lock().unwrap();
        plans.retain(|_, p| now.saturating_sub(p.time) < 30 * 60 * 1000);
    }
    let plan_id = crate::accounts::uuid_v4();
    diagnose_plans().lock().unwrap().insert(
        plan_id.clone(),
        DiagnosePlan {
            target: target.clone(),
            files: bad
                .into_iter()
                .filter(|f| f.get("url").and_then(|v| v.as_str()).map(|u| !u.is_empty()).unwrap_or(false))
                .collect(),
            metadata: serde_json::to_string(&merged).unwrap_or_default(),
            time: now,
        },
    );
    emit(1, 1, "检查完成");
    Ok(json!({ "id": plan_id, "findings": findings, "java": java, "session": session, "requiredJava": need }))
}

/// center:operation kind=repair：按诊断计划重下并校验坏文件（repairInstanceFiles）。
fn repair_instance(app: &tauri::AppHandle, _target: &Value, op: &Value) -> Result<Value, String> {
    let plan_id = op["planId"].as_str().unwrap_or("");
    if plan_id.is_empty() {
        return Err("缺少诊断计划，请先运行检查".into());
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let plan = {
        let mut plans = diagnose_plans().lock().unwrap();
        match plans.get(plan_id) {
            Some(p) if now.saturating_sub(p.time) < 30 * 60 * 1000 => {
                let taken = DiagnosePlan {
                    target: p.target.clone(),
                    files: p.files.clone(),
                    metadata: p.metadata.clone(),
                    time: p.time,
                };
                plans.remove(plan_id);
                taken
            }
            _ => return Err("检查结果已过期，请重新检查".into()),
        }
    };
    let rec = crate::tasks::register_task("修复运行文件", "world");
    let result = (|| -> Result<Value, String> {
        let (base, dir, _, _) = center_target(app, &plan.target)?;
        let id = plan.target["id"].as_str().unwrap_or("");
        let folder = base.to_string_lossy().to_string();
        let merged = crate::installer::read_version_json(app, &folder, id)?;
        if serde_json::to_string(&merged).unwrap_or_default() != plan.metadata {
            return Err("实例版本配置已变化，请重新检查".into());
        }
        let total = plan.files.len();
        let mut done = 0usize;
        for f in &plan.files {
            if rec.cancelled.load(std::sync::atomic::Ordering::SeqCst) {
                return Err("已取消".into());
            }
            crate::mods::assert_instance_idle(app, &dir)?;
            let dest = PathBuf::from(f.get("dest").and_then(|v| v.as_str()).unwrap_or(""));
            let url = f.get("url").and_then(|v| v.as_str()).unwrap_or("");
            if dest.as_os_str().is_empty() || url.is_empty() {
                continue;
            }
            let mut opts = crate::download::DownloadFileOpts::default();
            opts.sha1 = f.get("sha1").and_then(|v| v.as_str());
            opts.size = f.get("size").and_then(|v| v.as_u64());
            crate::download::with_file_job(&dest, Some(rec.as_ref()), || {
                crate::download::download_file(app, &rec, url, &dest, &opts)
            })?;
            done += 1;
            crate::tasks::emit_progress(
                app,
                json!({ "taskId": rec.id, "taskTitle": "修复运行文件", "stage": "world",
                        "progress": done as f64 / total.max(1) as f64, "text": format!("修复文件 {done}/{total}") }),
            );
        }
        Ok(json!(done))
    })();
    let cancelled = result.as_ref().err().map(|e| e == "已取消").unwrap_or(false);
    crate::tasks::task_done(app, &rec.id, result.is_ok(), result.as_ref().err().cloned(), cancelled, None);
    crate::tasks::finish_task(&rec.id);
    result
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
    // screenshot/preview 与列表接口同口径：缩图后再传，原图直传会打爆 IPC 缓冲
    small_image(&path)
        .map(|data| json!(data))
        .ok_or_else(|| "图片读取或缩放失败".to_string())
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
        "center:diagnose" => Some(diagnose(app, &target())),
        "center:dependencyPlan" => Some((|| {
            let target = target();
            let file_name = args.get(1).and_then(|v| v.as_str()).ok_or("缺少文件名")?;
            let (base, _, _, _) = center_target(app, &target)?;
            let version_id = target["id"].as_str().unwrap_or("");
            crate::mods::plan_missing_dependencies(app, version_id, &base.to_string_lossy(), file_name)
        })()),
        "center:dependencyApply" => Some((|| {
            let id = args.first().and_then(|v| v.as_str()).ok_or("缺少参数")?.to_string();
            let confirmed = args.get(1).and_then(|v| v.as_bool()).unwrap_or(false);
            // 对应原版 task('补充必要前置')：注册任务 → apply → taskDone
            let rec = crate::tasks::register_task("补充必要前置", "world");
            let task_id = rec.id.clone();
            let sink = move |fraction: f64| {
                crate::tasks::emit_progress(
                    app,
                    json!({ "taskId": task_id, "taskTitle": "补充必要前置", "stage": "world", "progress": fraction, "text": "下载并校验必要前置" }),
                );
            };
            let result = crate::mods::apply_missing_dependencies(app, &id, confirmed, &rec, Some(&sink));
            let cancelled = result.as_ref().err().map(|e| e == "已取消").unwrap_or(false);
            crate::tasks::task_done(app, &rec.id, result.is_ok(), result.as_ref().err().cloned(), cancelled, None);
            crate::tasks::finish_task(&rec.id);
            result
        })()),
        _ => None,
    }
}
