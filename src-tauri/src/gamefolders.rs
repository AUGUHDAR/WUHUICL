//! 游戏文件夹管理（对应原版 core/gameFolders.ts + core/folderPaths.ts）。
//!
//! 通道：folders:list / add / remove / rename / setDefault / setDownload / setActive /
//!       scan / open / contextMenu。
//! 铁律：remove/rename 只解除登记或改显示名，**绝不删除/移动磁盘文件**；
//!       所有列表操作持久化 folders + activeFolder + gameDir 三元组（persistFolders）。

use crate::installer::{
    basename_of, canonical_path, path_identity, resolve_minecraft_root, same_path, DATA_DIR_NAME,
};
use crate::settings;
use serde_json::{json, Value};
use std::path::Path;
use tauri::AppHandle;

// ---------------- 列表规范化（对应原版 normalizedFolders） ----------------

/// 去重（pathIdentity）+ 保证恰好一个 isDefault
fn normalized_folders(app: &AppHandle) -> Vec<Value> {
    let current = settings::get(app)["folders"].as_array().cloned().unwrap_or_default();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut result: Vec<Value> = Vec::new();
    for folder in current {
        let raw_path = folder["path"].as_str().unwrap_or("").to_string();
        let resolved = canonical_path(&raw_path).to_string_lossy().to_string();
        let identity = path_identity(&resolved);
        if seen.contains(&identity) {
            continue;
        }
        seen.insert(identity);
        let mut f = folder.clone();
        f["path"] = json!(resolved);
        result.push(f);
    }
    if !result.is_empty() && !result.iter().any(|f| f["isDefault"] == true) {
        result[0]["isDefault"] = json!(true);
    }
    if result.iter().filter(|f| f["isDefault"] == true).count() > 1 {
        let mut found = false;
        for f in result.iter_mut() {
            if f["isDefault"] == true && !found {
                found = true;
            } else {
                f["isDefault"] = json!(false);
            }
        }
    }
    result
}

/// 对应原版 persistFolders：folders + activeFolder + gameDir 一次提交
fn persist_folders(app: &AppHandle, folders: Vec<Value>, active: Option<&str>) -> Vec<Value> {
    let current = settings::get(app);
    let active_folder = active
        .map(str::to_string)
        .unwrap_or_else(|| current["activeFolder"].as_str().unwrap_or("").to_string());
    let selected = folders
        .iter()
        .find(|f| f["path"].as_str().map(|p| path_identity(p) == path_identity(&active_folder)).unwrap_or(false));
    let fallback = folders
        .iter()
        .find(|f| f["isDefault"] == true)
        .or_else(|| folders.first())
        .cloned()
        .unwrap_or(Value::Null);
    let chosen = selected
        .and_then(|f| f["path"].as_str())
        .map(str::to_string)
        .or_else(|| fallback["path"].as_str().map(str::to_string))
        .unwrap_or_default();
    let mut patch = current.clone();
    patch["folders"] = Value::Array(folders.clone());
    patch["activeFolder"] = json!(chosen);
    patch["gameDir"] = json!(chosen);
    let _ = settings::save(app, patch);
    folders
}

/// 对应原版 listGameFolders：读取 + 规范化 + 变更检测持久化
pub fn list_game_folders(app: &AppHandle) -> Value {
    let current = settings::get(app);
    let folders = normalized_folders(app);
    let selected = folders
        .iter()
        .find(|f| {
            f["path"]
                .as_str()
                .map(|p| path_identity(p) == path_identity(current["activeFolder"].as_str().unwrap_or("")))
                .unwrap_or(false)
        })
        .cloned();
    let active = selected
        .as_ref()
        .and_then(|f| f["path"].as_str())
        .map(str::to_string)
        .or_else(|| {
            folders
                .iter()
                .find(|f| f["isDefault"] == true)
                .and_then(|f| f["path"].as_str())
                .map(str::to_string)
        })
        .or_else(|| folders.first().and_then(|f| f["path"].as_str()).map(str::to_string))
        .unwrap_or_default();
    let changed = folders.len() != current["folders"].as_array().map(|a| a.len()).unwrap_or(0)
        || active != current["activeFolder"].as_str().unwrap_or("")
        || folders.iter().enumerate().any(|(i, f)| {
            let orig = current["folders"].as_array().and_then(|a| a.get(i));
            orig.map(|o| {
                f["path"].as_str() != o["path"].as_str()
                    || f["name"].as_str() != o["name"].as_str()
                    || (f["isDefault"] == true) != (o["isDefault"] == true)
            })
            .unwrap_or(true)
        });
    if changed {
        persist_folders(app, folders.clone(), Some(&active));
    }
    json!({ "folders": folders, "active": active })
}

// ---------------- 断言工具 ----------------

/// 对应原版 assertWritableDownloadFolder：存在性 + 写探测
fn assert_writable_download_folder(folder: &str) -> Result<(), String> {
    let meta = std::fs::metadata(folder).map_err(|_| "下载文件夹已不存在，请重新选择".to_string())?;
    if !meta.is_dir() {
        return Err("下载文件夹已不存在，请重新选择".into());
    }
    let probe = Path::new(folder).join(format!(".{}-write-test-{}", DATA_DIR_NAME.trim_start_matches('.'), crate::accounts::uuid_v4()));
    let write = std::fs::OpenOptions::new().write(true).create_new(true).open(&probe);
    match write {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::remove_file(&probe);
            Err(format!("下载文件夹不可写：{}", e))
        }
    }
}

fn clean_name(value: &str) -> Result<String, String> {
    let name = value.trim();
    if name.is_empty() {
        return Err("显示名称不能为空".into());
    }
    if name.chars().count() > 64 {
        return Err("显示名称最多 64 个字符".into());
    }
    Ok(name.to_string())
}

fn find_by_identity<'a>(folders: &'a [Value], input: &str) -> Option<&'a Value> {
    folders
        .iter()
        .find(|f| f["path"].as_str().map(|p| path_identity(p) == path_identity(input)).unwrap_or(false))
}

// ---------------- 通道实现（对应原版 gameFolders.ts 各导出函数） ----------------

/// folders:add：解析真实根目录 + 去重复用；返回 {folders, folder, structure}
pub fn add_game_folder(app: &AppHandle, input: &str) -> Result<Value, String> {
    let (resolved, structure) = resolve_minecraft_root(input)?;
    let current = list_game_folders(app);
    let folders = current["folders"].as_array().cloned().unwrap_or_default();
    if let Some(duplicate) = find_by_identity(&folders, &resolved) {
        return Ok(json!({
            "folders": folders,
            "folder": duplicate,
            "structure": structure
        }));
    }
    let folder = json!({
        "path": resolved,
        "name": basename_of(&resolved),
        "isDefault": false
    });
    let mut next = folders.clone();
    next.push(folder.clone());
    let persisted = persist_folders(app, next, current["active"].as_str());
    Ok(json!({ "folders": persisted, "folder": folder, "structure": structure }))
}

/// folders:rename：只改显示名（cleanName 校验）
pub fn rename_game_folder(app: &AppHandle, input: &str, display_name: &str) -> Result<Value, String> {
    let identity = path_identity(input);
    let name = clean_name(display_name)?;
    let current = list_game_folders(app);
    let folders = current["folders"].as_array().cloned().unwrap_or_default();
    if find_by_identity(&folders, input).is_none() && !folders.iter().any(|f| f["path"].as_str().map(|p| path_identity(p) == identity).unwrap_or(false)) {
        return Err("文件夹未登记".into());
    }
    let next: Vec<Value> = folders
        .iter()
        .map(|f| {
            let mut f = f.clone();
            if f["path"].as_str().map(|p| path_identity(p) == identity).unwrap_or(false) {
                f["name"] = json!(name);
            }
            f
        })
        .collect();
    let persisted = persist_folders(app, next, current["active"].as_str());
    Ok(Value::Array(persisted))
}

/// folders:remove：只解除登记，不删除磁盘文件；移除最后一个时自动补回内置默认文件夹
pub fn remove_game_folder(app: &AppHandle, input: &str) -> Result<Value, String> {
    let identity = path_identity(input);
    let current = list_game_folders(app);
    let folders = current["folders"].as_array().cloned().unwrap_or_default();
    let target = folders
        .iter()
        .find(|f| f["path"].as_str().map(|p| path_identity(p) == identity).unwrap_or(false))
        .cloned()
        .ok_or("文件夹未登记")?;
    let mut next: Vec<Value> = folders
        .iter()
        .filter(|f| f["path"].as_str().map(|p| path_identity(p) != identity).unwrap_or(true))
        .cloned()
        .collect();
    if next.is_empty() {
        // 已移除最后一个文件夹：自动重建内置默认文件夹，不留死锁
        let fallback = settings::default_game_dir().to_string_lossy().to_string();
        let _ = std::fs::create_dir_all(&fallback);
        next = vec![json!({ "path": fallback, "name": "默认文件夹", "isDefault": true })];
    }
    let was_default = target["isDefault"] == true;
    if was_default {
        next[0]["isDefault"] = json!(true);
    }
    let next_active = if path_identity(current["active"].as_str().unwrap_or("")) == identity {
        next.iter()
            .find(|f| f["isDefault"] == true)
            .or_else(|| next.first())
            .and_then(|f| f["path"].as_str())
            .map(str::to_string)
            .unwrap_or_default()
    } else {
        current["active"].as_str().unwrap_or("").to_string()
    };
    Ok(Value::Array(persist_folders(app, next, Some(&next_active))))
}

/// folders:setDefault：写探测 + 切默认（承接新安装版本与 libraries/assets 共享位置）
pub fn set_default_game_folder(app: &AppHandle, input: &str) -> Result<Value, String> {
    let identity = path_identity(input);
    let current = list_game_folders(app);
    let folders = current["folders"].as_array().cloned().unwrap_or_default();
    let selected = find_by_identity(&folders, input).cloned().ok_or("文件夹未登记")?;
    let selected_path = selected["path"].as_str().unwrap_or("").to_string();
    assert_writable_download_folder(&selected_path)?;
    let next: Vec<Value> = folders
        .iter()
        .map(|f| {
            let mut f = f.clone();
            f["isDefault"] = json!(f["path"].as_str().map(|p| path_identity(p) == identity).unwrap_or(false));
            f
        })
        .collect();
    let persisted = persist_folders(app, next, Some(&selected_path));
    Ok(Value::Array(persisted))
}

/// folders:setDownload：一次提交完成注册 + 选定（已登记的根保持登记）
pub fn set_download_game_folder(app: &AppHandle, input: &str) -> Result<Value, String> {
    let (resolved, _structure) = resolve_minecraft_root(input)?;
    assert_writable_download_folder(&resolved)?;
    let current = list_game_folders(app);
    let folders = current["folders"].as_array().cloned().unwrap_or_default();
    let identity = path_identity(&resolved);
    let registered = find_by_identity(&folders, &resolved).is_some();
    let mut next = folders;
    if !registered {
        next.push(json!({
            "path": resolved,
            "name": basename_of(&resolved),
            "isDefault": false
        }));
    }
    let next: Vec<Value> = next
        .iter()
        .map(|f| {
            let mut f = f.clone();
            f["isDefault"] = json!(f["path"].as_str().map(|p| path_identity(p) == identity).unwrap_or(false));
            f
        })
        .collect();
    Ok(Value::Array(persist_folders(app, next, Some(&resolved))))
}

/// folders:setActive：切换活动文件夹（gameDir 跟随）；返回文件夹路径（renderer 期待 string）
pub fn set_active_game_folder(app: &AppHandle, input: &str) -> Result<Value, String> {
    let identity = path_identity(input);
    let current = list_game_folders(app);
    let folders = current["folders"].as_array().cloned().unwrap_or_default();
    let folder = find_by_identity(&folders, input)
        .cloned()
        .ok_or("文件夹未登记")?;
    let path = folder["path"].as_str().unwrap_or("").to_string();
    if !Path::new(&path).exists() {
        return Err("文件夹已不存在，请重新连接或解除绑定".into());
    }
    persist_folders(app, folders, Some(&path));
    Ok(json!(path))
}

/// folders:scan：结构检测 + 已安装扫描 + 警告收集（FolderScanResult）
pub fn scan_game_folder(app: &AppHandle, input: &str) -> Result<Value, String> {
    let started = std::time::Instant::now();
    let current = list_game_folders(app);
    let folders = current["folders"].as_array().cloned().unwrap_or_default();
    let folder = find_by_identity(&folders, input)
        .or_else(|| {
            // 允许精确路径匹配（renderer 传回的是登记时的原始路径）
            folders
                .iter()
                .find(|f| f["path"].as_str() == Some(input))
        })
        .cloned()
        .ok_or("文件夹未登记")?;
    let folder_path = folder["path"].as_str().unwrap_or("").to_string();
    if !Path::new(&folder_path).exists() {
        return Ok(json!({
            "folder": folder,
            "structure": "missing",
            "status": "error",
            "versions": [],
            "errors": ["文件夹不存在或磁盘当前不可用"],
            "scannedAt": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            "durationMs": started.elapsed().as_millis() as u64
        }));
    }
    let structure = if Path::new(&folder_path).join(DATA_DIR_NAME).exists() {
        "kamucl"
    } else if Path::new(&folder_path).join("versions").exists() {
        "minecraft"
    } else {
        "empty"
    };
    let (versions, errors) = scan_installed_folder_with_errors(app, &folder_path);
    Ok(json!({
        "folder": folder,
        "structure": structure,
        "status": if errors.is_empty() { "ready" } else { "warning" },
        "versions": versions,
        "errors": errors,
        "scannedAt": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "durationMs": started.elapsed().as_millis() as u64
    }))
}

/// 扫描指定根目录并收集损坏条目警告
/// （对应原版 versions.ts scanInstalledFolder 的 errors 语义；版本列表复用 crate::versions::scan_folder_public）
fn scan_installed_folder_with_errors(app: &AppHandle, root: &str) -> (Vec<Value>, Vec<String>) {
    let mut versions = crate::versions::scan_folder_public(root, true);
    let mut errors: Vec<String> = Vec::new();
    let scanned_ids: std::collections::HashSet<String> = versions
        .iter()
        .map(|v| v["id"].as_str().unwrap_or("").to_string())
        .collect();
    let versions_dir = Path::new(root).join("versions");
    let Ok(entries) = std::fs::read_dir(&versions_dir) else {
        return (versions, errors);
    };
    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let json_path = entry.path().join(format!("{}.json", name));
        if !scanned_ids.contains(&name) {
            // 损坏条目不会静默消失：以 incomplete 条目 + errors 返回
            if !json_path.exists() {
                versions.push(json!({ "id": name, "mcVersion": "未知", "folder": root, "incomplete": true }));
                errors.push(format!("{}：缺少版本描述 {}.json", name, name));
            } else {
                match std::fs::read(&json_path)
                    .map_err(|e| e.to_string())
                    .and_then(|raw| serde_json::from_slice::<Value>(&raw).map_err(|e| e.to_string()))
                {
                    Err(e) => {
                        versions.push(json!({ "id": name, "mcVersion": "未知", "folder": root, "incomplete": true }));
                        errors.push(format!("{}：版本描述损坏（{}）", name, e));
                    }
                    Ok(_j) => {
                        // json 可解析但扫描器未收录（继承缺失等）：不重复报错
                    }
                }
            }
            continue;
        }
        // 继承链检查：inheritsFrom 指向的父版本在本地（versions/ 或 .wuhuicl/base）都找不到 → 损坏
        if let Ok(j) = std::fs::read(&json_path)
            .map_err(|e| e.to_string())
            .and_then(|raw| serde_json::from_slice::<Value>(&raw).map_err(|e| e.to_string()))
        {
            if let Some(parent) = j.get("inheritsFrom").and_then(|v| v.as_str()) {
                let parent_local = Path::new(root).join("versions").join(parent).join(format!("{}.json", parent));
                let parent_base = crate::installer::base_version_json_path_in(
                    &crate::installer::default_folder_path(app),
                    parent,
                );
                if !parent_local.exists() && !parent_base.exists() {
                    if let Some(item) = versions.iter_mut().find(|v| v["id"].as_str() == Some(name.as_str())) {
                        item["incomplete"] = json!(true);
                    }
                    errors.push(format!("{}：继承的版本 {} 缺失或损坏", name, parent));
                }
            }
        }
    }
    (versions, errors)
}

// ---------------- invoke 入口 ----------------

/// 统一入口（PORT_CONVENTIONS 铁律 2）：返回 None 表示本模块不认识该通道
pub fn invoke(app: &tauri::AppHandle, channel: &str, args: &[Value]) -> Option<Result<Value, String>> {
    match channel {
        "folders:list" => Some(Ok(list_game_folders(app))),
        "folders:add" => Some(
            args.first()
                .and_then(|v| v.as_str())
                .ok_or_else(|| "缺少参数".to_string())
                .and_then(|p| add_game_folder(app, p)),
        ),
        "folders:remove" => Some(
            args.first()
                .and_then(|v| v.as_str())
                .ok_or_else(|| "缺少参数".to_string())
                .and_then(|p| remove_game_folder(app, p)),
        ),
        "folders:rename" => Some(
            args.first()
                .and_then(|v| v.as_str())
                .zip(args.get(1).and_then(|v| v.as_str()))
                .ok_or_else(|| "缺少参数".to_string())
                .and_then(|(p, name)| rename_game_folder(app, p, name)),
        ),
        "folders:setDefault" => Some(
            args.first()
                .and_then(|v| v.as_str())
                .ok_or_else(|| "缺少参数".to_string())
                .and_then(|p| set_default_game_folder(app, p)),
        ),
        "folders:setDownload" => Some(
            args.first()
                .and_then(|v| v.as_str())
                .ok_or_else(|| "缺少参数".to_string())
                .and_then(|p| set_download_game_folder(app, p)),
        ),
        "folders:setActive" => Some(
            args.first()
                .and_then(|v| v.as_str())
                .ok_or_else(|| "缺少参数".to_string())
                .and_then(|p| set_active_game_folder(app, p)),
        ),
        "folders:scan" => Some(
            args.first()
                .and_then(|v| v.as_str())
                .ok_or_else(|| "缺少参数".to_string())
                .and_then(|p| scan_game_folder(app, p)),
        ),
        "folders:open" => Some(folders_open(app, args)),
        "folders:contextMenu" => Some(folders_context_menu(app, args)),
        _ => None,
    }
}

/// folders:open（ipc.ts 注册）：登记校验 + shell.openPath（错误字符串 → Err）
fn folders_open(app: &AppHandle, args: &[Value]) -> Result<Value, String> {
    let p = args.first().and_then(|v| v.as_str()).unwrap_or("");
    let state = list_game_folders(app);
    let folders = state["folders"].as_array().cloned().unwrap_or_default();
    let target = folders
        .iter()
        .find(|f| f["path"].as_str().map(|fp| same_path(fp, p)).unwrap_or(false))
        .cloned()
        .ok_or("文件夹未登记")?;
    let path = target["path"].as_str().unwrap_or("").to_string();
    if cfg!(windows) {
        std::process::Command::new("explorer.exe")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    } else {
        std::process::Command::new("xdg-open")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(Value::Null)
}

/// folders:contextMenu：原生右键菜单未移植；仅保留「未登记报错」语义（偏差见报告）
fn folders_context_menu(app: &AppHandle, args: &[Value]) -> Result<Value, String> {
    let p = args.first().and_then(|v| v.as_str()).unwrap_or("");
    let state = list_game_folders(app);
    let folders = state["folders"].as_array().cloned().unwrap_or_default();
    if find_by_identity(&folders, p).is_none() {
        return Err("文件夹未登记".into());
    }
    Ok(Value::Null)
}
