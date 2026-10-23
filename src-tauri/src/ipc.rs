//! 通用 IPC 路由：通道名与 Electron 原版 1:1（71+ 通道合同面）。
//!
//! ipc_invoke = ipcMain.handle（有返回值，错误 reject）
//! ipc_send   = ipcMain.on（fire-and-forget）
//!
//! 启动关键通道（settings/accounts/versions/folders/systemInfo/exitHistory/…）
//! 为真实现；未移植通道显式返回「未实现的通道」错误，便于移植期发现缺口。

use crate::{accounts, appearance, bridge, catalog, direct, frp, selfupdate, centercmd, community, feedback, misc, modpacks, mods, projections, recordings, servers, skins, tasks, terracotta, voxlink, vlcli, worlds, exportlogs, gate, gamedir, gamefolders, installer, javaman, launch, mslogin, settings, sysinfo, versions, windowctl};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager, WebviewWindow};

#[tauri::command]
pub async fn ipc_invoke(
    app: AppHandle,
    _window: WebviewWindow,
    channel: String,
    args: Vec<Value>,
) -> Result<Value, String> {
    // 同步 command 在 WebView2 的 UI 线程上执行，扫描/下载/对话框会冻住界面（原版 main 是独立进程）。
    // OOM 探针归因：blocking 线程记下当前处理的通道，崩溃日志能直接指认肇事 IPC。
    tauri::async_runtime::spawn_blocking(move || {
        crate::set_current_ipc(&channel);
        invoke(&app, &channel, &args)
    })
    .await
    .map_err(|e| format!("后台任务崩溃：{e}"))?
}

#[tauri::command]
pub fn ipc_send(
    app: AppHandle,
    window: WebviewWindow,
    channel: String,
    args: Vec<Value>,
) -> Result<(), String> {
    // 原版逐条 ipcMain.on 都比对 event.sender === 主窗口 webContents
    send(&app, window.label() == "main", &channel, &args)
}

fn invoke(app: &AppHandle, channel: &str, args: &[Value]) -> Result<Value, String> {
    // 子系统模块优先：认领的通道直接返回（含 Err）
    if let Some(r) = gamefolders::invoke(app, channel, args) {
        return r;
    }
    if let Some(r) = installer::invoke(app, channel, args) {
        return r;
    }
    if let Some(r) = catalog::invoke(app, channel, args) {
        return r;
    }
    if let Some(r) = community::invoke(app, channel, args) {
        return r;
    }
    if let Some(r) = worlds::invoke(app, channel, args) {
        return r;
    }
    if let Some(r) = modpacks::invoke(app, channel, args) {
        return r;
    }
    if let Some(r) = servers::invoke(app, channel, args) {
        return r;
    }
    if let Some(r) = skins::invoke(app, channel, args) {
        return r;
    }
    if let Some(r) = centercmd::invoke(app, channel, args) {
        return r;
    }
    if let Some(r) = misc::invoke(app, channel, args) {
        return r;
    }
    if let Some(r) = appearance::invoke(app, channel, args) {
        return r;
    }
    if let Some(r) = selfupdate::invoke(app, channel, args) {
        return r;
    }
    if let Some(r) = mods::invoke(app, channel, args) {
        return r;
    }
    if let Some(r) = bridge::invoke(app, channel, args) {
        return r;
    }
    if let Some(r) = direct::invoke(app, channel, args) {
        return r;
    }
    if let Some(r) = terracotta::invoke(app, channel, args) {
        return r;
    }
    if let Some(r) = frp::invoke(app, channel, args) {
        return r;
    }
    if let Some(r) = voxlink::invoke(app, channel, args) {
        return r;
    }
    if let Some(r) = feedback::invoke(app, channel, args) {
        return r;
    }
    if let Some(r) = projections::invoke(app, channel, args) {
        return r;
    }
    if let Some(r) = recordings::invoke(app, channel, args) {
        return r;
    }
    if let Some(r) = launch::invoke(app, channel, args) {
        return r;
    }
    if let Some(r) = javaman::invoke(app, channel, args) {
        return r;
    }
    if let Some(r) = mslogin::invoke(app, channel, args) {
        return r;
    }
    if let Some(r) = vlcli::invoke(app, channel, args) {
        return r;
    }
    if let Some(r) = gamedir::invoke(app, channel, args) {
        return r;
    }
    if let Some(r) = exportlogs::invoke(app, channel, args) {
        return r;
    }
    match channel {
        // ---------------- 设置 ----------------
        "settings:get" => Ok(settings::get(app)),
        "settings:set" => {
            let patch = args.first().cloned().unwrap_or(json!({}));
            let saved = settings::save(app, patch)?;
            // 对应原版 settingsSet：保存后重设外观与窗口自适应
            if let Some(win) = app.get_webview_window("main") {
                windowctl::apply_native_appearance(&win, &saved);
            }
            windowctl::configure_auto_fit(app, saved.get("uiWindowAutoFit").and_then(|v| v.as_bool()).unwrap_or(false));
            Ok(saved)
        }
        "app:systemInfo" => Ok(sysinfo::system_info()),
        // 剪贴板读文本（Win32 直读，不走 navigator.clipboard——WebView2 读剪贴板会弹权限框）
        "app:readClipboard" => Ok(json!(sysinfo::clipboard_text())),
        "app:selectDir" => Ok(pick(app, true, &["全部文件|*"])),
        "app:selectFile" => Ok(pick(app, false, &["整合包|*.mrpack;*.zip"])),
        "app:selectImage" => Ok(pick(app, false, &["图片|*.png;*.jpg;*.jpeg;*.webp"])),

        // ---------------- 游戏文件夹 ----------------
        "folders:list" => Ok(json!({
            "folders": settings::get(app)["folders"],
            "active": settings::get(app)["activeFolder"]
        })),
        "folders:add" => {
            let path = args.first().and_then(|v| v.as_str()).ok_or("缺少 path")?;
            folders_add(app, path)
        }
        "folders:remove" => {
            let path = args.first().and_then(|v| v.as_str()).ok_or("缺少 path")?;
            folders_mutate(app, |folders| {
                folders.retain(|f| f["path"].as_str() != Some(path));
            })
        }
        "folders:rename" => {
            let (path, name) = two_strings(args)?;
            folders_mutate(app, |folders| {
                for f in folders.iter_mut() {
                    if f["path"].as_str() == Some(path.as_str()) {
                        f["name"] = json!(name);
                    }
                }
            })
        }
        "folders:setDefault" => {
            let path = args.first().and_then(|v| v.as_str()).ok_or("缺少 path")?;
            folders_mutate(app, |folders| {
                for f in folders.iter_mut() {
                    f["isDefault"] = json!(f["path"].as_str() == Some(path));
                }
            })
        }
        "folders:setActive" => {
            let path = args.first().and_then(|v| v.as_str()).ok_or("缺少 path")?;
            settings::save(app, json!({ "activeFolder": path }))?;
            Ok(json!(path))
        }
        "folders:scan" => {
            let path = args.first().and_then(|v| v.as_str()).ok_or("缺少 path")?;
            let start = std::time::Instant::now();
            let list = versions::scan_folder_public(path, true);
            let status = if list.is_empty() { "warning" } else { "ready" };
            Ok(json!({
                "folder": { "path": path, "name": std::path::Path::new(path)
                    .file_name().map(|n| n.to_string_lossy()).unwrap_or_default(), "isDefault": false },
                "structure": if list.is_empty() { "empty" } else { "minecraft" },
                "status": status,
                "versions": list,
                "errors": [],
                "scannedAt": chrono::Utc::now().to_rfc3339(),
                "durationMs": start.elapsed().as_millis() as u64
            }))
        }
        "folders:open" => {
            let path = args.first().and_then(|v| v.as_str()).ok_or("缺少 path")?;
            std::process::Command::new("explorer.exe")
                .arg(path)
                .spawn()
                .map_err(|e| e.to_string())?;
            Ok(Value::Null)
        }
        "folders:contextMenu" => Ok(Value::Null), // 原生右键菜单：子系统阶段接

        // ---------------- 账号 ----------------
        "accounts:list" => Ok(accounts::list(app)),
        "accounts:addOffline" => {
            let name = args.first().and_then(|v| v.as_str()).ok_or("缺少用户名")?;
            accounts::add_offline(app, name).map_err(|e| e)
        }
        "accounts:remove" => {
            let id = args.first().and_then(|v| v.as_str()).ok_or("缺少 id")?;
            Ok(accounts::remove(app, id))
        }
        "accounts:select" => {
            let id = args.first().and_then(|v| v.as_str()).ok_or("缺少 id")?;
            Ok(accounts::select(app, id))
        }
        "accounts:selected" => Ok(accounts::selected(app)),
        "accounts:msBegin" => Err("微软登录链未移植".into()),
        "accounts:msCancel" => Ok(Value::Null),
        "accounts:yggProviders" => Ok(json!([])),
        "accounts:refresh" => Err("令牌刷新未移植".into()),
        "accounts:yggRuntime" => Err("authlib-injector 运行时未移植".into()),
        "accounts:yggProbe" | "accounts:yggLogin" | "accounts:yggSaveProvider"
        | "accounts:yggRemoveProvider" | "accounts:yggSelectProfile" => {
            Err("外置登录未移植".into())
        }

        // ---------------- 版本 ----------------
        "versions:installed" => {
            let all = args.first().and_then(|v| v.as_bool()).unwrap_or(false);
            Ok(versions::list_installed(&settings::get(app), all))
        }
        "versions:catalog" => Ok(json!({ "versions": [], "checkedAt": 0, "stale": true })),
        "versions:manifest" => Ok(json!([])),
        "versions:install" => Err("版本安装器未移植".into()),
        "versions:remove" | "versions:rename" | "versions:cleanup" | "versions:setJava"
        | "versions:setResolution" | "versions:setIsolation" | "versions:isolationPlan"
        | "versions:setIcon" | "versions:uploadIcon" | "versions:uploadThumbnail"
        | "versions:setThumbnailFit" | "versions:resetThumbnail" | "versions:categoriesUpdate" => {
            Err("版本管理未移植".into())
        }
        "loaders:list" | "loaders:fabricApi" => Err("加载器清单未移植".into()),

        // ---------------- Java ----------------
        "java:list" => Ok(json!([])),
        "java:refresh" => Ok(json!([])),
        "java:cancelScan" => Ok(json!(true)),
        "java:addCustom" | "java:pickAdd" | "java:hide" => Err("Java 管理未移植".into()),

        // ---------------- 退出历史 ----------------
        "exitHistory:list" | "exitHistory:ack" | "exitHistory:clear" => Ok(json!([])),

        // ---------------- 自更新 ----------------
        "update:check" => Ok(json!({ "ok": true, "hasUpdate": false })),
        "update:getState" | "update:getPending" => Ok(Value::Null),
        "update:getConfigStatus" => {
            let s = settings::get(app);
            Ok(json!({
                "configVersion": s["configVersion"].as_i64().unwrap_or(1),
                "current": 1,
                "mismatch": (if s["configVersion"].as_i64().unwrap_or(1) > 1 { Value::from("newer") } else { Value::Null })
            }))
        }
        "update:listReleases" => Ok(json!([])),
        "update:skip" | "update:apply" | "update:applyPending" | "update:restoreBackup"
        | "update:resetSettings" | "update:applyLocal" | "update:start" => {
            Err("自更新未移植".into())
        }
        "update:pickLocalFile" => Ok(Value::Null),

        // ---------------- 皮肤 ----------------
        "skin:avatar" => Ok(Value::Null),
        "skin:profile" => Ok(json!({ "username": "", "skins": [], "capes": [] })),
        "skin:history" => Ok(json!([])),
        "skin:upload" | "skin:cape" | "skin:historyDelete" | "skin:historyRename"
        | "skin:uploadHistory" | "skin:offlineApply" | "skin:offlineReset" | "skin:editorSave"
        | "skin:editorUpload" => Err("皮肤服务未移植".into()),

        // ---------------- 窗口 ----------------
        "window:visibility" => Ok(windowctl::visibility(app)),

        // ---------------- 任务 ----------------
        // 对应原版 cancelTaskAndWait：发出取消后等任务主流程退出（finish_task 移除登记），
        // 30s 未退出抛「取消超时」，避免 UI 把仍在写盘的任务当成已取消
        "tasks:cancel" => {
            let id = args.first().and_then(|v| v.as_str()).ok_or("缺少 taskId")?;
            if !tasks::cancel_task(id) {
                return Ok(json!(false));
            }
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            while tasks::get_task(id).is_some() {
                if std::time::Instant::now() >= deadline {
                    return Err("取消超时：后台任务尚未停止，请稍后重试".into());
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Ok(json!(true))
        }
        "tasks:pause" => {
            let id = args.first().and_then(|v| v.as_str()).ok_or("缺少 taskId")?;
            Ok(json!(tasks::pause_task(id)))
        }
        "tasks:resume" => {
            let id = args.first().and_then(|v| v.as_str()).ok_or("缺少 taskId")?;
            Ok(json!(tasks::resume_task(id)))
        }

        // ---------------- 文件 ----------------
        "fs:list" => {
            let (rel, folder) = two_strings_first_optional(args)?;
            fs_list(app, &rel, &folder)
        }
        // 「版本文件夹内的所有」：聚合共享根 + 各实例，返回 {entries, instances, warnings}
        "fs:listAggregate" => {
            let (rel, folder) = two_strings_first_optional(args)?;
            fs_list_aggregate(app, &rel, &folder)
        }
        "fs:path" => {
            let (rel, folder) = two_strings_first_optional(args)?;
            Ok(json!(fs_resolve(app, &rel, &folder)?.to_string_lossy().to_string()))
        }
        "app:openDir" => {
            let rel = args.first().and_then(|v| v.as_str()).unwrap_or("");
            let dir = fs_resolve(app, rel, "")?;
            std::fs::create_dir_all(&dir).ok();
            std::process::Command::new("explorer.exe")
                .arg(dir.as_os_str())
                .spawn()
                .map_err(|e| e.to_string())?;
            Ok(Value::Null)
        }
        "fs:remove" => {
            // 对应原版 ipc.ts IPC.fsRemove
            let (rel, name) = two_strings(args)?;
            let folder = args.get(2).and_then(|v| v.as_str()).unwrap_or("");
            let parts: Vec<&str> = rel.split(['/', '\\']).filter(|s| !s.is_empty()).collect();
            let last_is_res = parts
                .last()
                .map_or(false, |k| matches!(*k, "mods" | "resourcepacks" | "shaderpacks"));
            if !(((parts.len() == 1) || (parts.len() == 3 && parts[0] == "versions")) && last_is_res) {
                return Err("不支持删除该目录中的文件".into());
            }
            let dir = fs_resolve(app, &rel, folder)?;
            let instance_dir = dir.parent().ok_or("路径异常")?.to_path_buf();
            let dir_for_job = dir.clone();
            crate::download::with_file_job(&dir, None, move || {
                mods::assert_instance_idle(app, &instance_dir)?;
                crate::projections::recycle_file(&dir_for_job, &name)
            })?;
            fs_list(app, &rel, folder)
        }
        "fs:toggleDisable" => {
            // 对应原版 ipc.ts IPC.fsToggleDisable：.jar ↔ .jar.disabled
            let (rel, name) = two_strings(args)?;
            let folder = args.get(2).and_then(|v| v.as_str()).unwrap_or("");
            let running = mods::running_version_ids();
            let affected: Vec<String> = match isolated_version_of(&rel) {
                Some(id) => vec![id],
                None => versions::list_installed(&settings::get(app), false)
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .filter(|v| v["isolated"] != true)
                    .filter_map(|v| v["id"].as_str().map(str::to_string))
                    .collect(),
            };
            if affected.iter().any(|id| running.contains(id)) {
                return Err("该实例正在运行中，请先退出游戏再禁用/启用模组".into());
            }
            let dir = fs_resolve(app, &rel, folder)?;
            toggle_disable_in_dir(&dir, &name)?;
            fs_list(app, &rel, folder)
        }
        // 以下三条按绝对目录作用域（聚合视图的每一行自带 directory，直接定位回该实例目录）
        "fs:removeAt" => {
            let (dir_arg, name) = two_strings(args)?;
            let dir = mods::checked_resource_dir(app, &dir_arg)?;
            let instance_dir = dir.parent().ok_or("路径异常")?.to_path_buf();
            let dir_for_job = dir.clone();
            crate::download::with_file_job(&dir, None, move || {
                mods::assert_instance_idle(app, &instance_dir)?;
                crate::projections::recycle_file(&dir_for_job, &name)
            })?;
            Ok(Value::Null)
        }
        "fs:toggleDisableAt" => {
            let (dir_arg, name) = two_strings(args)?;
            let dir = mods::checked_resource_dir(app, &dir_arg)?;
            toggle_disable_in_dir(&dir, &name)?;
            Ok(Value::Null)
        }
        "fs:dragAt" => {
            fs_drag_at(app, args);
            Ok(Value::Null)
        }
        "fs:importResources" => {
            // 对应原版 ipc.ts IPC.fsImportResources
            let files: Vec<String> = args
                .first()
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            let id = args.get(1).and_then(|v| v.as_str()).unwrap_or("");
            let folder = args.get(2).and_then(|v| v.as_str()).unwrap_or("");
            let kind = args.get(3).and_then(|v| v.as_str()).unwrap_or("");
            let targets = mods::scan_mod_targets(app);
            let list = targets["versions"].as_array().cloned().unwrap_or_default();
            let target = mods::select_mod_target(&list, id, folder)?;
            import_resource_files(&files, &target, kind)
        }

        // ---------------- 游戏 ----------------
        "game:launch" | "game:restart" | "game:kill" | "game:restartCancel" => {
            Err("游戏启动引擎未移植".into())
        }
        "launch:exportLogs" => Ok(Value::Null),

        // ---------------- 服务器 / 整合包 / 世界 / 社区 / 模组 ----------------
        "servers:list" => Ok(json!([])),
        "servers:ping" => Err("服务器查询未移植".into()),
        "servers:add" | "servers:remove" | "servers:edit" | "servers:favorite"
        | "servers:bind" | "servers:syncFromDat" | "servers:prepareLaunch" => {
            Err("服务器管理未移植".into())
        }
        "modpack:probe" | "modpack:install" | "modpack:supplyFiles" | "modpack:openFile"
        | "import:probe" => Err("整合包未移植".into()),
        "world:probe" | "world:import" => Err("世界导入未移植".into()),
        "community:search" | "community:project" | "community:files" | "community:download" => {
            Err("社区资源未移植".into())
        }
        "mods:parse" | "mods:install" | "mods:duplicates" | "mods:crossDuplicates"
        | "mods:icons" | "mods:targets" | "mods:prepare" | "mods:commit" | "mods:discard"
        | "mods:checkUpdates" | "mods:applyUpdates" | "mods:favorites" | "mods:favorite"
        | "mods:favoriteRemove" | "mods:favoriteLink" | "mods:favoriteVersions"
        | "mods:favoriteVersionsCancel" | "mods:favoriteLocal" | "mods:catalog"
        | "mods:setEnabled" | "mods:setLocked" | "mods:versionChoices" | "mods:versionPlan"
        | "mods:versionApply" | "mods:versionDiscard" | "mods:migrationPlan"
        | "mods:migrationApply" | "mods:supplementalList" | "mods:supplementalKeep"
        | "mods:supplementalRetry" => Err("模组管理未移植".into()),

        // ---------------- 插件 / 默认配置 / 玩偶 / 实例中心 / 外观 ----------------
        "plugins:list" => Ok(json!([])),
        "plugins:install" | "plugins:setEnabled" | "plugins:remove" | "plugins:readCode"
        | "plugins:openDir" => Err("插件系统未移植".into()),
        "keys:getDefault" => Ok(json!({})),
        "keys:setDefault" | "keys:reset" => Err("默认按键未移植".into()),
        "gameOptions:get" => Ok(json!({ "enabled": false, "id": null, "value": null })),
        "gameOptions:set" => Err("默认游戏选项未移植".into()),
        "defaultPacks:get" => Ok(json!([])),
        "defaultPacks:import" | "defaultPacks:pick" | "defaultPacks:remove"
        | "defaultPacks:move" | "defaultPacks:setEnabled" | "defaultPacks:apply" => {
            Err("默认资源包未移植".into())
        }
        "mascots:state" => Ok(json!({ "counts": {}, "order": [], "sound": { "muted": false, "volume": 0.5 } })),
        "mascots:batch" | "mascots:slap" | "mascots:sound" | "mascots:reset" => {
            Err("玩偶状态未移植".into())
        }
        "center:overview" | "center:worlds" | "center:screenshots" | "center:backups"
        | "center:diagnose" | "center:operation" | "center:file"
        | "center:dependencyPlan" | "center:dependencyApply" => Err("实例中心未移植".into()),
        "appearance:resetTheme" | "appearance:exportTheme" | "appearance:importTheme"
        | "appearance:draftRead" | "appearance:draftSave" | "appearance:draftDiscard"
        | "appearance:draftApply" | "appearance:importBackground"
        | "appearance:importBackgroundMulti" | "appearance:resetBackground"
        | "appearance:importLaunchThumbnail" | "appearance:resetLaunchThumbnail" => {
            Err("外观系统未移植".into())
        }
        "memory:organize" => Ok(json!({ "freedMB": 0, "processed": 0 })),

        // ---------------- 联机：direct / tc / frp / voxlink 已各自接管，无占位分支 ----------------
        "voxlink:tickets:pick" | "voxlink:tickets:release" | "voxlink:tickets:list"
        | "voxlink:tickets:cancel" | "voxlink:tickets:poll" | "voxlink:tickets:detail"
        | "voxlink:tickets:submit" | "voxlink:tickets:reply" | "voxlink:tickets:retract"
        | "voxlink:tickets:delete" | "voxlink:mods:bypass" | "voxlink:mods:cancel"
        | "voxlink:mods:check" | "voxlink:mods:download" => Err("工单系统未移植".into()),

        // 投影 / 录像 / 桥接已由各模块 invoke 接管，不再有占位分支

        _ => Err(format!("未实现的通道：{}", channel)),
    }
}

fn send(app: &AppHandle, from_main: bool, channel: &str, args: &[Value]) -> Result<(), String> {
    if let Some(r) = projections::send(app, channel, args) {
        return r;
    }
    if let Some(r) = recordings::send(app, channel, args) {
        return r;
    }
    match channel {
        "boot:renderer-ready" => {
            gate::renderer_ready(app);
            // 对应原版 index.ts:125：页面加载完成后恢复 frp 隧道
            frp::restore(app);
            // 对应原版 ipc.ts：renderer-ready 时重放运行中游戏的启动状态
            let states = launch::active_launch_states();
            if !states.is_empty() {
                if let Some(win) = app.get_webview_window("main") {
                    for st in states {
                        let _ = tauri::Emitter::emit_to(&win, "main", "event:launchState", st);
                    }
                }
            } else {
                launch::restore_running_game(app);
            }
            Ok(())
        }
        "boot:stage" => {
            if let Some(s) = args.first().and_then(|v| v.as_str()) {
                gate::stage(app, s);
            }
            Ok(())
        }
        "boot:splash-ready" => {
            gate::splash_ready(app);
            Ok(())
        }
        "boot:assembled" => {
            gate::assembled(app);
            Ok(())
        }
        "boot:finished" => {
            gate::finish(app);
            Ok(())
        }
        "boot:splash-failed" => {
            gate::splash_failed(app);
            Ok(())
        }
        "fs:drag" => {
            fs_drag(app, args);
            Ok(())
        }
        _ => windowctl::command(app, from_main, channel, args).or_else(|e| {
            // 未识别的 send 通道：记日志不炸（与原版 ipcMain.on 未注册一致只是静默）
            if e.starts_with("未实现的 send 通道") {
                Ok(())
            } else {
                Err(e)
            }
        }),
    }
}

/// 对应原版 ipc.ts ipcMain.on('fs:drag') 与 core/resourceDragPaths.ts：
/// 目录与文件名校验逐条照抄；校验通过后启动 OLE DoDragDrop 模态拖出。
fn fs_drag(app: &AppHandle, args: &[Value]) {
    let rel = args.first().and_then(|v| v.as_str()).unwrap_or("");
    let names = args.get(1).cloned().unwrap_or(Value::Null);
    let folder = args.get(2).and_then(|v| v.as_str());
    let msg: String = (|| {
        let parts: Vec<&str> = rel.split('/').collect();
        let instance_shape = parts.len() == 3
            && parts[0] == "versions"
            && !parts[1].is_empty()
            && !parts[1].contains('\\')
            && matches!(parts[2], "mods" | "resourcepacks" | "shaderpacks");
        // 「版本文件夹内的所有」视图里的共享根目录：单级 <rel>，目录按活动文件夹解析
        let shared_shape = parts.len() == 1 && matches!(parts[0], "mods" | "resourcepacks" | "shaderpacks");
        if !instance_shape && !shared_shape {
            return "不支持拖出该目录".to_string();
        }
        let folder = match folder {
            Some(f) if !f.is_empty() => f,
            _ => return "未指定游戏文件夹".to_string(),
        };
        let dir = if instance_shape {
            let target = json!({ "folder": folder, "id": parts[1] });
            let (_, dir, _, _) = match centercmd::center_target(app, &target) {
                Ok(v) => v,
                Err(e) => return e,
            };
            dir.join(parts[2])
        } else {
            let Ok(dir) = mods::checked_resource_dir(app, &std::path::Path::new(folder).join(parts[0]).to_string_lossy()) else {
                return "资源目录无效".to_string();
            };
            dir
        };
        let files = match drag_resource_files(&dir, &names) {
            Ok(v) => v,
            Err(e) => return e,
        };
        if let Err(e) = crate::dragout::validate_paths(&files) {
            return e;
        }
        // DoDragDrop 是模态循环：阻塞本调用线程直到松手/取消（用户手势全程持有）
        match crate::dragout::start_drag_out(&files) {
            Ok(_) => String::new(), // 拖出成功：不推错误
            Err(e) => e,
        }
    })();
    // 空 msg = 成功，不推 files:dragError
    if !msg.is_empty() {
        if let Some(win) = app.get_webview_window("main") {
            let _ = tauri::Emitter::emit_to(&win, "main", "files:dragError", format!("Error: {}", msg));
        }
    }
}

/// 模组禁用/启用（.jar ↔ .jar.disabled），按目录执行：实例视图与聚合视图共用
fn toggle_disable_in_dir(dir: &std::path::Path, name: &str) -> Result<(), String> {
    let base = std::path::Path::new(name)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let lower = base.to_lowercase();
    let to = if lower.ends_with(".jar.disabled") {
        dir.join(&base[..base.len() - ".disabled".len()])
    } else if lower.ends_with(".jar") {
        dir.join(format!("{}.disabled", base))
    } else {
        return Err("仅支持禁用 .jar 模组文件".into());
    };
    let from = dir.join(&base);
    if !from.exists() {
        return Err("文件不存在，请刷新后重试".into());
    }
    if to.exists() {
        return Err("目标文件名已存在，请手动处理后重试".into());
    }
    std::fs::rename(&from, &to).map_err(|e| e.to_string())
}

/// fs_drag 的目录作用域版本：前端聚合视图的每行直接给出绝对目录
fn fs_drag_at(app: &AppHandle, args: &[Value]) {
    let dir_arg = args.first().and_then(|v| v.as_str()).unwrap_or("");
    let names = args.get(1).cloned().unwrap_or(Value::Null);
    let msg: String = (|| {
        let dir = match mods::checked_resource_dir(app, dir_arg) {
            Ok(dir) => dir,
            Err(e) => return e,
        };
        let files = match drag_resource_files(&dir, &names) {
            Ok(v) => v,
            Err(e) => return e,
        };
        if let Err(e) = crate::dragout::validate_paths(&files) {
            return e;
        }
        match crate::dragout::start_drag_out(&files) {
            Ok(_) => String::new(),
            Err(e) => e,
        }
    })();
    if !msg.is_empty() {
        if let Some(win) = app.get_webview_window("main") {
            let _ = tauri::Emitter::emit_to(&win, "main", "files:dragError", format!("Error: {}", msg));
        }
    }
}

/// 对应原版 dragResourceFilesSync + dragPath
fn drag_resource_files(root: &std::path::Path, names: &Value) -> Result<Vec<String>, String> {
    let arr = match names.as_array() {
        Some(a) if !a.is_empty() && a.len() <= 1000 => a,
        _ => return Err("每次可拖出 1–1000 个文件，请分批选择".into()),
    };
    let base = std::fs::symlink_metadata(root).map_err(|_| "目录不是普通目录".to_string())?;
    if !base.is_dir() || base.file_type().is_symlink() {
        return Err("目录不是普通目录".into());
    }
    let mut out: Vec<String> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for item in arr {
        let name = match item.as_str() {
            Some(n) => n,
            None => return Err("无效的文件名".into()),
        };
        if name.contains('/') || name.contains('\\') {
            return Err("无效的文件名".into());
        }
        if seen.iter().any(|s| s == name) {
            continue;
        }
        seen.push(name.to_string());
        let mut file = root.to_path_buf();
        for part in name.split('/') {
            file = file.join(part);
            let m = std::fs::symlink_metadata(&file).map_err(|_| "不跟随符号链接".to_string())?;
            if m.file_type().is_symlink() {
                return Err("不跟随符号链接".into());
            }
        }
        let m = std::fs::symlink_metadata(&file).map_err(|_| "不支持拖出特殊文件".to_string())?;
        if !m.is_file() && !m.is_dir() {
            return Err("不支持拖出特殊文件".into());
        }
        out.push(file.to_string_lossy().to_string());
    }
    Ok(out)
}

// ---------------- 工具 ----------------

fn two_strings(args: &[Value]) -> Result<(String, String), String> {
    let a = args.first().and_then(|v| v.as_str()).ok_or("缺少参数")?;
    let b = args.get(1).and_then(|v| v.as_str()).ok_or("缺少参数")?;
    Ok((a.to_string(), b.to_string()))
}

fn two_strings_first_optional(args: &[Value]) -> Result<(String, String), String> {
    let a = args.first().and_then(|v| v.as_str()).unwrap_or("");
    let b = args.get(1).and_then(|v| v.as_str()).unwrap_or("");
    Ok((a.to_string(), b.to_string()))
}

/// 对应原版 /^versions\/([^/]+)\//：只有隔离实例路径能取出实例 id
fn isolated_version_of(rel: &str) -> Option<String> {
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

/// 对应原版 core/resourceFiles.ts importResourceFiles（判定顺序与文案逐条照抄）
fn import_resource_files(files: &[String], target: &Value, kind: &str) -> Result<Value, String> {
    let jar_re = regex::Regex::new(r"(?i)\.jar(\.disabled)?$").unwrap();
    let zip_re = regex::Regex::new(r"(?i)\.zip$").unwrap();
    let game_dir = match kind {
        "mods" | "resourcepacks" | "shaderpacks" => target["gameDirectory"].as_str().filter(|s| !s.is_empty()),
        _ => None,
    };
    let Some(game_dir) = game_dir else {
        return Err("无效资源目录".into());
    };
    if files.is_empty() {
        return Err("请拖入资源文件".into());
    }
    let dir = std::path::Path::new(game_dir).join(kind);
    let mut names: Vec<String> = Vec::new();
    let mut copies: Vec<(std::path::PathBuf, std::path::PathBuf, bool)> = Vec::new();
    for file in files {
        let p = std::path::Path::new(file);
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let meta = std::fs::metadata(p).map_err(|e| format!("此页面不支持 {name}：{e}"))?;
        let unsupported = if meta.is_dir() {
            kind == "mods"
        } else {
            !(if kind == "mods" { jar_re.is_match(&name) } else { zip_re.is_match(&name) })
        };
        if unsupported {
            return Err(format!("此页面不支持 {name}"));
        }
        if !meta.is_dir() && !meta.is_file() {
            return Err("不支持此文件类型".into());
        }
        let dest = dir.join(&name);
        let low = name.to_lowercase();
        if names.contains(&low) || dest.exists() {
            return Err(format!("同名文件已存在，未覆盖：{name}"));
        }
        names.push(low);
        copies.push((p.to_path_buf(), dest, meta.is_dir()));
    }
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    for (src, dest, is_dir) in copies {
        if is_dir {
            copy_tree(&src, &dest)?;
        } else {
            let mut opts = std::fs::OpenOptions::new();
            opts.create_new(true).write(true);
            let mut data = std::fs::read(&src).map_err(|e| e.to_string())?;
            let mut f = opts.open(&dest).map_err(|e| e.to_string())?;
            std::io::Write::write_all(&mut f, &mut data).map_err(|e| e.to_string())?;
        }
    }
    Ok(json!(files.len()))
}

/// fs.cpSync(..., { recursive: true, force: false, errorOnExist: true }) 等价
fn copy_tree(src: &std::path::Path, dest: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(src).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let to = dest.join(entry.file_name());
        if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            copy_tree(&entry.path(), &to)?;
        } else if to.exists() {
            return Err(format!("同名文件已存在，未覆盖：{}", to.display()));
        } else {
            std::fs::copy(entry.path(), &to).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn folders_mutate(
    app: &AppHandle,
    f: impl FnOnce(&mut Vec<Value>),
) -> Result<Value, String> {
    let mut s = settings::get(app);
    {
        let folders = s["folders"].as_array_mut().ok_or("设置损坏")?;
        f(folders);
    }
    let out = s["folders"].clone();
    settings::save(app, s)?;
    Ok(out)
}

fn folders_add(app: &AppHandle, path: &str) -> Result<Value, String> {
    let canonical = installer::strip_device_prefix(
        &std::fs::canonicalize(path).map_err(|_| "目录不存在".to_string())?,
    )
    .to_string_lossy()
    .to_string();
    let name = std::path::Path::new(&canonical)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| canonical.clone());
    let mut s = settings::get(app);
    {
        let folders = s["folders"].as_array_mut().ok_or("设置损坏")?;
        if folders.iter().any(|f| f["path"].as_str() == Some(canonical.as_str())) {
            return Err("该目录已登记".into());
        }
        folders.push(json!({ "path": canonical, "name": name, "isDefault": false }));
    }
    let folder = s["folders"].as_array().unwrap().last().cloned().unwrap();
    let folders_json = s["folders"].clone();
    let structure = versions::scan_folder_public(&canonical, true);
    settings::save(app, s)?;
    Ok(json!({ "folders": folders_json, "folder": folder, "structure": structure }))
}

/// 游戏目录解析：'' = 活动文件夹根；mods/resourcepacks/shaderpacks/saves/config 等相对安全；
/// versions/<id>/... 允许；拒绝越界（对应原版 safeDir）
/// 对应原版 core/resourceDirectory.ts resolveResourceDirectory：只读选中的那一个实例
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
    let raw = std::fs::read_to_string(&json_path)
        .map_err(|_| "所选游戏版本已移除或版本描述损坏，请到「游戏版本」页刷新或修复".to_string())?;
    let j: Value = serde_json::from_str(raw.trim_start_matches('\u{feff}'))
        .map_err(|_| "所选游戏版本已移除或版本描述损坏，请到「游戏版本」页刷新或修复".to_string())?;
    let state = crate::installer::instance_directory_state(app, folder, id, &j);
    let game_dir = state["path"].as_str().ok_or("所选游戏版本已移除或版本描述损坏，请到「游戏版本」页刷新或修复")?;
    Ok(std::path::Path::new(game_dir).join(kind))
}

/// 对应原版 ipc.ts 的 safeDir：允许 gameDir 下单级子目录，或 versions/<id>/<sub> 三级
pub fn fs_resolve(app: &AppHandle, rel: &str, folder: &str) -> Result<std::path::PathBuf, String> {
    let s = settings::get(app);
    let parts: Vec<&str> = rel.split(['/', '\\']).filter(|x| !x.is_empty() && *x != ".").collect();
    if parts.iter().any(|x| *x == "..") {
        return Err("非法目录".into());
    }
    let is_version_path = parts.first().copied() == Some("versions");
    if parts.len() > if is_version_path { 3 } else { 2 } {
        return Err("非法目录".into());
    }
    if !folder.is_empty() {
        let known = s["folders"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .any(|f| crate::installer::path_identity(f["path"].as_str().unwrap_or(""))
                        == crate::installer::path_identity(folder))
            })
            .unwrap_or(false);
        if !known {
            return Err("游戏文件夹未登记".into());
        }
    }
    let base = if !folder.is_empty() {
        folder.to_string()
    } else if is_version_path && parts.len() >= 2 {
        crate::installer::folder_of_version(app, parts[1])
    } else {
        let active = s["activeFolder"].as_str().unwrap_or("").to_string();
        if active.is_empty() {
            s["gameDir"].as_str().unwrap_or("").to_string()
        } else {
            active
        }
    };
    if base.is_empty() {
        return Err("未配置游戏目录".into());
    }
    if is_version_path && parts.len() == 3 && matches!(parts[2], "mods" | "resourcepacks" | "shaderpacks") {
        return resolve_resource_directory(app, &base, parts[1], parts[2]);
    }
    let mut out = std::path::PathBuf::from(&base);
    for p in &parts {
        out.push(p);
    }
    if !out.starts_with(std::path::Path::new(&base)) {
        return Err("非法目录".into());
    }
    Ok(out)
}

/// 对应原版 core/resourceDirectory.ts listResourceEntries（字段名 isDir 与原版一致）
fn fs_list(app: &AppHandle, rel: &str, folder: &str) -> Result<Value, String> {
    let dir = fs_resolve(app, rel, folder)?;
    let names = match std::fs::read_dir(&dir) {
        Ok(rd) => rd.filter_map(|e| e.ok()).collect::<Vec<_>>(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(json!([])),
        Err(_) => return Err("无法读取资源目录，请检查文件夹权限后重试".into()),
    };
    let mut out: Vec<Value> = Vec::new();
    for e in names {
        let name = e.file_name().to_string_lossy().into_owned();
        let meta = match std::fs::symlink_metadata(&e.path()) {
            Ok(m) => m,
            Err(_) => continue,
        };
        // 链接与 Finder 元数据不是可玩资源：不显示、也不删用户的文件
        if meta.file_type().is_symlink() || name == ".DS_Store" {
            continue;
        }
        let mtime = meta
            .modified()
            .ok()
            .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        out.push(json!({ "name": name, "size": meta.len(), "isDir": meta.is_dir(), "mtime": mtime }));
    }
    out.sort_by(|a, b| {
        let da = a["isDir"].as_bool().unwrap_or(false);
        let db = b["isDir"].as_bool().unwrap_or(false);
        db.cmp(&da).then_with(|| {
            a["name"]
                .as_str()
                .unwrap_or("")
                .to_lowercase()
                .cmp(&b["name"].as_str().unwrap_or("").to_lowercase())
        })
    });
    Ok(Value::Array(out))
}

/// 「版本文件夹内的所有」：聚合共享根目录 + 每个已装实例的该子目录。
/// 与投影/录像/服务器三页同口径——按实例分开列，**不合并同名文件**（不同实例的同名
/// mod 是两份独立文件），每条带 instanceId/instanceName 供前端按行定位回实例目录。
fn fs_list_aggregate(app: &AppHandle, rel: &str, folder: &str) -> Result<Value, String> {
    if !matches!(rel, "mods" | "resourcepacks" | "shaderpacks") {
        return Err("内容范围只支持模组/资源包/光影包目录".into());
    }
    // 共享根目录（<folder>/<rel>）
    let mut sources: Vec<(String, String, std::path::PathBuf)> = Vec::new();
    let shared = fs_resolve(app, rel, folder)?;
    let shared_identity = crate::installer::path_identity(&shared.to_string_lossy());
    sources.push((String::new(), "共享目录".to_string(), shared));

    // 各已装实例：优先取实例声明目录（隔离时 = versions/<id>），否则回落共享根
    let settings_value = settings::get(app);
    let installed = crate::versions::list_installed(&settings_value, false);
    let mut warnings: Vec<String> = Vec::new();
    let mut instances: Vec<Value> = Vec::new();
    for v in installed.as_array().cloned().unwrap_or_default() {
        let id = v["id"].as_str().unwrap_or("").to_string();
        if id.is_empty() {
            continue;
        }
        let vfolder = v
            .get("folder")
            .and_then(|x| x.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| folder.to_string());
        // 实例目录与共享根相同时不重复列：共享条目已覆盖
        match crate::mods::resolve_resource_directory(app, &vfolder, &id, rel) {
            Ok(dir) => {
                if crate::installer::path_identity(&dir.to_string_lossy()) == shared_identity {
                    continue;
                }
                instances.push(json!({ "id": id, "name": id, "folder": vfolder }));
                sources.push((id.clone(), id, dir));
            }
            Err(e) => warnings.push(format!("{}：{}", id, e)),
        }
    }

    let mut out: Vec<Value> = Vec::new();
    for (instance_id, instance_name, dir) in sources {
        let names = match std::fs::read_dir(&dir) {
            Ok(rd) => rd.filter_map(|e| e.ok()).collect::<Vec<_>>(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => {
                warnings.push(format!("{}：无法读取资源目录", dir.to_string_lossy()));
                continue;
            }
        };
        for e in names {
            let name = e.file_name().to_string_lossy().into_owned();
            let meta = match std::fs::symlink_metadata(&e.path()) {
                Ok(m) => m,
                Err(_) => continue,
            };
            if meta.file_type().is_symlink() || name == ".DS_Store" {
                continue;
            }
            let mtime = meta
                .modified()
                .ok()
                .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            out.push(json!({
                "name": name,
                "size": meta.len(),
                "isDir": meta.is_dir(),
                "mtime": mtime,
                "instanceId": instance_id,
                "instanceName": instance_name,
                "directory": dir.to_string_lossy()
            }));
        }
    }
    // 实例优先、目录优先、再按名称：同名文件在不同实例下相邻可见
    out.sort_by(|a, b| {
        let ia = a["instanceId"].as_str().unwrap_or("");
        let ib = b["instanceId"].as_str().unwrap_or("");
        ia.cmp(ib).then_with(|| {
            let da = a["isDir"].as_bool().unwrap_or(false);
            let db = b["isDir"].as_bool().unwrap_or(false);
            db.cmp(&da).then_with(|| {
                a["name"]
                    .as_str()
                    .unwrap_or("")
                    .to_lowercase()
                    .cmp(&b["name"].as_str().unwrap_or("").to_lowercase())
            })
        })
    });
    Ok(json!({ "entries": out, "instances": instances, "warnings": warnings }))
}

/// 文件/目录选择（tauri-plugin-dialog 阻塞式，对应原版 dialog.showOpenDialog）
fn pick(app: &AppHandle, directory: bool, filter: &[&str]) -> Value {
    use tauri_plugin_dialog::DialogExt;
    let dialog = app.dialog().file();
    let picked = if directory {
        dialog.blocking_pick_folder()
    } else {
        let mut d = app.dialog().file();
        for f in filter {
            let (title, patterns) = f.split_once('|').unwrap_or((f, "*"));
            let _ = title;
            let exts: Vec<&str> = patterns.split(';').filter_map(|p| p.strip_prefix("*.")).collect();
            if !exts.is_empty() {
                d = d.add_filter(title.to_string(), &exts);
            }
        }
        d.blocking_pick_file()
    };
    picked.map(|p| json!(p.to_string())).unwrap_or(Value::Null)
}
