//! Ordered subsystem dispatch followed by the legacy channel fallback.

use super::resources::{
    folders_add, folders_mutate, fs_list, fs_resolve, import_resource_files, isolated_version_of,
    pick, two_strings, two_strings_first_optional,
};
use crate::{
    appearance, bridge, catalog, centercmd, community, direct, exportlogs, frp, gamedir,
    gamefolders, installer, javaman, launch, misc, modpacks, mods, mslogin, projections,
    recordings, selfupdate, servers, settings, skins, sysinfo, terracotta, versions, vlcli,
    voxlink, windowctl, worlds,
};
use serde_json::{json, Value};
use tauri::AppHandle;

type Handler = fn(&AppHandle, &str, &[Value]) -> Option<Result<Value, String>>;

type DispatchStage = (&'static str, Handler);

const DOCUMENTED_SUBSYSTEM_ORDER: &[&str] = &[
    "gamefolders",
    "installer",
    "catalog",
    "community",
    "worlds",
    "modpacks",
    "servers",
    "skins",
    "centercmd",
    "misc",
    "appearance",
    "selfupdate",
    "mods",
    "bridge",
    "direct",
    "terracotta",
    "frp",
    "voxlink",
    "projections",
    "recordings",
    "launch",
    "javaman",
    "mslogin",
    "vlcli",
    "gamedir",
    "exportlogs",
];

const DOCUMENTED_SUBSYSTEM_HANDLERS: &[DispatchStage] = &[
    ("gamefolders", gamefolders::invoke),
    ("installer", installer::invoke),
    ("catalog", catalog::invoke),
    ("community", community::invoke),
    ("worlds", worlds::invoke),
    ("modpacks", modpacks::invoke),
    ("servers", servers::invoke),
    ("skins", skins::invoke),
    ("centercmd", centercmd::invoke),
    ("misc", misc::invoke),
    ("appearance", appearance::invoke),
    ("selfupdate", selfupdate::invoke),
    ("mods", mods::invoke),
    ("bridge", bridge::invoke),
    ("direct", direct::invoke),
    ("terracotta", terracotta::invoke),
    ("frp", frp::invoke),
    ("voxlink", voxlink::invoke),
    ("projections", projections::invoke),
    ("recordings", recordings::invoke),
    ("launch", launch::invoke),
    ("javaman", javaman::invoke),
    ("mslogin", mslogin::invoke),
    ("vlcli", vlcli::invoke),
    ("gamedir", gamedir::invoke),
    ("exportlogs", exportlogs::invoke),
];

fn composed_handlers() -> impl Iterator<Item = DispatchStage> {
    DOCUMENTED_SUBSYSTEM_HANDLERS.iter().copied().chain([
        ("accounts", crate::features::accounts::invoke),
        ("settings", crate::features::settings::invoke),
    ])
}

pub(crate) fn invoke(app: &AppHandle, channel: &str, args: &[Value]) -> Result<Value, String> {
    for (_, handler) in composed_handlers() {
        if let Some(result) = handler(app, channel, args) {
            return result;
        }
    }
    if channel == "settings:set" {
        if let Some(result) = crate::features::settings::invoke(app, channel, args) {
            let saved = result?;
            if let Some(window) = tauri::Manager::get_webview_window(app, "main") {
                windowctl::apply_native_appearance(&window, &saved);
            }
            windowctl::configure_auto_fit(
                app,
                saved
                    .get("uiWindowAutoFit")
                    .and_then(|value| value.as_bool())
                    .unwrap_or(false),
            );
            return Ok(saved);
        }
    } else if let Some(result) = crate::features::settings::invoke(app, channel, args) {
        return result;
    }
    legacy_invoke(app, channel, args)
}

fn legacy_invoke(app: &AppHandle, channel: &str, args: &[Value]) -> Result<Value, String> {
    match channel {
        "app:systemInfo" => Ok(sysinfo::system_info()),
        "app:selectDir" => Ok(pick(app, true, &["全部文件|*"])),
        "app:selectFile" => Ok(pick(app, false, &["整合包|*.mrpack;*.zip"])),
        "app:selectImage" => Ok(pick(app, false, &["图片|*.png;*.jpg;*.jpeg;*.webp"])),
        "folders:list" => Ok(json!({
            "folders": settings::get(app)["folders"],
            "active": settings::get(app)["activeFolder"]
        })),
        "folders:add" => {
            let path = args
                .first()
                .and_then(|value| value.as_str())
                .ok_or("缺少 path")?;
            folders_add(app, path)
        }
        "folders:remove" => {
            let path = args
                .first()
                .and_then(|value| value.as_str())
                .ok_or("缺少 path")?;
            folders_mutate(app, |folders| {
                folders.retain(|folder| folder["path"].as_str() != Some(path));
            })
        }
        "folders:rename" => {
            let (path, name) = two_strings(args)?;
            folders_mutate(app, |folders| {
                for folder in folders.iter_mut() {
                    if folder["path"].as_str() == Some(path.as_str()) {
                        folder["name"] = json!(name);
                    }
                }
            })
        }
        "folders:setDefault" => {
            let path = args
                .first()
                .and_then(|value| value.as_str())
                .ok_or("缺少 path")?;
            folders_mutate(app, |folders| {
                for folder in folders.iter_mut() {
                    folder["isDefault"] = json!(folder["path"].as_str() == Some(path));
                }
            })
        }
        "folders:setActive" => {
            let path = args
                .first()
                .and_then(|value| value.as_str())
                .ok_or("缺少 path")?;
            settings::save(app, json!({ "activeFolder": path }))?;
            Ok(json!(path))
        }
        "folders:scan" => {
            let path = args
                .first()
                .and_then(|value| value.as_str())
                .ok_or("缺少 path")?;
            let start = std::time::Instant::now();
            let list = versions::scan_folder_public(path, true);
            let status = if list.is_empty() { "warning" } else { "ready" };
            Ok(json!({
                "folder": { "path": path, "name": std::path::Path::new(path)
                    .file_name().map(|name| name.to_string_lossy()).unwrap_or_default(), "isDefault": false },
                "structure": if list.is_empty() { "empty" } else { "minecraft" },
                "status": status,
                "versions": list,
                "errors": [],
                "scannedAt": chrono::Utc::now().to_rfc3339(),
                "durationMs": start.elapsed().as_millis() as u64
            }))
        }
        "folders:open" => {
            let path = args
                .first()
                .and_then(|value| value.as_str())
                .ok_or("缺少 path")?;
            std::process::Command::new("explorer.exe")
                .arg(path)
                .spawn()
                .map_err(|error| error.to_string())?;
            Ok(Value::Null)
        }
        "folders:contextMenu" => Ok(Value::Null),

        "versions:installed" => {
            let all = args
                .first()
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
            Ok(versions::list_installed(&settings::get(app), all))
        }
        "versions:catalog" => Ok(json!({ "versions": [], "checkedAt": 0, "stale": true })),
        "versions:manifest" => Ok(json!([])),
        "versions:install" => Err("版本安装器未移植".into()),
        "versions:remove"
        | "versions:rename"
        | "versions:cleanup"
        | "versions:setJava"
        | "versions:setResolution"
        | "versions:setIsolation"
        | "versions:isolationPlan"
        | "versions:setIcon"
        | "versions:uploadIcon"
        | "versions:uploadThumbnail"
        | "versions:setThumbnailFit"
        | "versions:resetThumbnail"
        | "versions:categoriesUpdate" => Err("版本管理未移植".into()),
        "loaders:list" | "loaders:fabricApi" => Err("加载器清单未移植".into()),

        "java:list" => Ok(json!([])),
        "java:refresh" => Ok(json!([])),
        "java:cancelScan" => Ok(json!(true)),
        "java:addCustom" | "java:pickAdd" | "java:hide" => Err("Java 管理未移植".into()),

        "exitHistory:list" | "exitHistory:ack" | "exitHistory:clear" => Ok(json!([])),

        "update:check" => Ok(json!({ "ok": true, "hasUpdate": false })),
        "update:getState" | "update:getPending" => Ok(Value::Null),
        "update:getConfigStatus" => {
            let settings = settings::get(app);
            Ok(json!({
                "configVersion": settings["configVersion"].as_i64().unwrap_or(1),
                "current": 1,
                "mismatch": (if settings["configVersion"].as_i64().unwrap_or(1) > 1 { Value::from("newer") } else { Value::Null })
            }))
        }
        "update:listReleases" => Ok(json!([])),
        "update:skip"
        | "update:apply"
        | "update:applyPending"
        | "update:restoreBackup"
        | "update:resetSettings"
        | "update:applyLocal"
        | "update:start" => Err("自更新未移植".into()),
        "update:pickLocalFile" => Ok(Value::Null),

        "skin:avatar" => Ok(Value::Null),
        "skin:profile" => Ok(json!({ "username": "", "skins": [], "capes": [] })),
        "skin:history" => Ok(json!([])),
        "skin:upload" | "skin:cape" | "skin:historyDelete" | "skin:historyRename"
        | "skin:uploadHistory" | "skin:offlineApply" | "skin:offlineReset" | "skin:editorSave"
        | "skin:editorUpload" => Err("皮肤服务未移植".into()),

        "window:visibility" => Ok(windowctl::visibility(app)),
        "tasks:cancel" | "tasks:pause" | "tasks:resume" => Ok(json!(false)),

        "fs:list" => {
            let (rel, folder) = two_strings_first_optional(args)?;
            fs_list(app, &rel, &folder)
        }
        "fs:path" => {
            let (rel, folder) = two_strings_first_optional(args)?;
            Ok(json!(fs_resolve(app, &rel, &folder)?
                .to_string_lossy()
                .to_string()))
        }
        "app:openDir" => {
            let rel = args.first().and_then(|value| value.as_str()).unwrap_or("");
            let dir = fs_resolve(app, rel, "")?;
            std::fs::create_dir_all(&dir).ok();
            std::process::Command::new("explorer.exe")
                .arg(dir.as_os_str())
                .spawn()
                .map_err(|error| error.to_string())?;
            Ok(Value::Null)
        }
        "fs:remove" => {
            let (rel, name) = two_strings(args)?;
            let folder = args.get(2).and_then(|value| value.as_str()).unwrap_or("");
            let parts: Vec<&str> = rel
                .split(['/', '\\'])
                .filter(|part| !part.is_empty())
                .collect();
            let last_is_res = parts.last().map_or(false, |part| {
                matches!(*part, "mods" | "resourcepacks" | "shaderpacks")
            });
            if !(((parts.len() == 1) || (parts.len() == 3 && parts[0] == "versions"))
                && last_is_res)
            {
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
            let (rel, name) = two_strings(args)?;
            let folder = args.get(2).and_then(|value| value.as_str()).unwrap_or("");
            let running = mods::running_version_ids();
            let affected: Vec<String> = match isolated_version_of(&rel) {
                Some(id) => vec![id],
                None => versions::list_installed(&settings::get(app), false)
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .filter(|version| version["isolated"] != true)
                    .filter_map(|version| version["id"].as_str().map(str::to_string))
                    .collect(),
            };
            if affected.iter().any(|id| running.contains(id)) {
                return Err("该实例正在运行中，请先退出游戏再禁用/启用模组".into());
            }
            let dir = fs_resolve(app, &rel, folder)?;
            let base = std::path::Path::new(&name)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let lower = base.to_lowercase();
            let destination = if lower.ends_with(".jar.disabled") {
                dir.join(&base[..base.len() - ".disabled".len()])
            } else if lower.ends_with(".jar") {
                dir.join(format!("{}.disabled", base))
            } else {
                return Err("仅支持禁用 .jar 模组文件".into());
            };
            let source = dir.join(&base);
            if !source.exists() {
                return Err("文件不存在，请刷新后重试".into());
            }
            if destination.exists() {
                return Err("目标文件名已存在，请手动处理后重试".into());
            }
            std::fs::rename(&source, &destination).map_err(|error| error.to_string())?;
            fs_list(app, &rel, folder)
        }
        "fs:importResources" => {
            let files: Vec<String> = args
                .first()
                .and_then(|value| value.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let id = args.get(1).and_then(|value| value.as_str()).unwrap_or("");
            let folder = args.get(2).and_then(|value| value.as_str()).unwrap_or("");
            let kind = args.get(3).and_then(|value| value.as_str()).unwrap_or("");
            let targets = mods::scan_mod_targets(app);
            let list = targets["versions"].as_array().cloned().unwrap_or_default();
            let target = mods::select_mod_target(&list, id, folder)?;
            import_resource_files(&files, &target, kind)
        }

        "game:launch" | "game:restart" | "game:kill" | "game:restartCancel" => {
            Err("游戏启动引擎未移植".into())
        }
        "launch:exportLogs" => Ok(Value::Null),

        "servers:list" => Ok(json!([])),
        "servers:ping" => Err("服务器查询未移植".into()),
        "servers:add"
        | "servers:remove"
        | "servers:edit"
        | "servers:favorite"
        | "servers:bind"
        | "servers:syncFromDat"
        | "servers:prepareLaunch" => Err("服务器管理未移植".into()),
        "modpack:probe"
        | "modpack:install"
        | "modpack:supplyFiles"
        | "modpack:openFile"
        | "import:probe" => Err("整合包未移植".into()),
        "world:probe" | "world:import" => Err("世界导入未移植".into()),
        "community:search" | "community:project" | "community:files" | "community:download" => {
            Err("社区资源未移植".into())
        }
        "mods:parse"
        | "mods:install"
        | "mods:duplicates"
        | "mods:crossDuplicates"
        | "mods:icons"
        | "mods:targets"
        | "mods:prepare"
        | "mods:commit"
        | "mods:discard"
        | "mods:checkUpdates"
        | "mods:applyUpdates"
        | "mods:favorites"
        | "mods:favorite"
        | "mods:favoriteRemove"
        | "mods:favoriteLink"
        | "mods:favoriteVersions"
        | "mods:favoriteVersionsCancel"
        | "mods:favoriteLocal"
        | "mods:catalog"
        | "mods:setEnabled"
        | "mods:setLocked"
        | "mods:versionChoices"
        | "mods:versionPlan"
        | "mods:versionApply"
        | "mods:versionDiscard"
        | "mods:migrationPlan"
        | "mods:migrationApply"
        | "mods:supplementalList"
        | "mods:supplementalKeep"
        | "mods:supplementalRetry" => Err("模组管理未移植".into()),

        "plugins:list" => Ok(json!([])),
        "plugins:install" | "plugins:setEnabled" | "plugins:remove" | "plugins:readCode"
        | "plugins:openDir" => Err("插件系统未移植".into()),
        "keys:getDefault" => Ok(json!({})),
        "keys:setDefault" | "keys:reset" => Err("默认按键未移植".into()),
        "gameOptions:get" => Ok(json!({ "enabled": false, "id": null, "value": null })),
        "gameOptions:set" => Err("默认游戏选项未移植".into()),
        "defaultPacks:get" => Ok(json!([])),
        "defaultPacks:import"
        | "defaultPacks:pick"
        | "defaultPacks:remove"
        | "defaultPacks:move"
        | "defaultPacks:setEnabled"
        | "defaultPacks:apply" => Err("默认资源包未移植".into()),
        "mascots:state" => {
            Ok(json!({ "counts": {}, "order": [], "sound": { "muted": false, "volume": 0.5 } }))
        }
        "mascots:batch" | "mascots:slap" | "mascots:sound" | "mascots:reset" => {
            Err("玩偶状态未移植".into())
        }
        "center:overview"
        | "center:worlds"
        | "center:screenshots"
        | "center:backups"
        | "center:diagnose"
        | "center:operation"
        | "center:file"
        | "center:dependencyPlan"
        | "center:dependencyApply" => Err("实例中心未移植".into()),
        "appearance:resetTheme"
        | "appearance:exportTheme"
        | "appearance:importTheme"
        | "appearance:draftRead"
        | "appearance:draftSave"
        | "appearance:draftDiscard"
        | "appearance:draftApply"
        | "appearance:importBackground"
        | "appearance:importBackgroundMulti"
        | "appearance:resetBackground"
        | "appearance:importLaunchThumbnail"
        | "appearance:resetLaunchThumbnail" => Err("外观系统未移植".into()),
        "memory:organize" => Ok(json!({ "freedMB": 0, "processed": 0 })),

        "voxlink:tickets:pick"
        | "voxlink:tickets:release"
        | "voxlink:tickets:list"
        | "voxlink:tickets:cancel"
        | "voxlink:tickets:poll"
        | "voxlink:tickets:detail"
        | "voxlink:tickets:submit"
        | "voxlink:tickets:reply"
        | "voxlink:tickets:retract"
        | "voxlink:tickets:delete"
        | "voxlink:mods:bypass"
        | "voxlink:mods:cancel"
        | "voxlink:mods:check"
        | "voxlink:mods:download" => Err("工单系统未移植".into()),

        _ => Err(format!("未实现的通道：{}", channel)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subsystem_stages_are_ordered_and_before_typed_fallbacks() {
        let names: Vec<&str> = DOCUMENTED_SUBSYSTEM_HANDLERS
            .iter()
            .map(|(name, _)| *name)
            .collect();
        assert_eq!(names, DOCUMENTED_SUBSYSTEM_ORDER);
        let composed: Vec<&str> = composed_handlers().map(|(name, _)| name).collect();
        let expected: Vec<&str> = DOCUMENTED_SUBSYSTEM_ORDER
            .iter()
            .copied()
            .chain(["accounts", "settings"])
            .collect();
        assert_eq!(composed, expected);
        assert!(composed.contains(&"accounts"));
        assert!(composed.contains(&"settings"));
    }
}
