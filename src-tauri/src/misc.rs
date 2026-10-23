//! 杂项子系统集合（对应原版 core/ 的 keybindings / defaultGameOptions /
//! defaultResourcePacks / exitHistory / mascots / plugins 文件级实现）。
//! 这些通道全部是 userData 下的 JSON 存取，语义 1:1。

use serde_json::{json, Value};
use std::path::PathBuf;

// ---------------- 默认按键（keybindings.ts） ----------------

const BIND_RE_OK: fn(&str) -> bool = |v| v.starts_with("key.keyboard.") || v.starts_with("key.mouse.");

// 对应原版 shared/keybindings.ts 的 VANILLA_KEYBINDS（id 与 defaultBind 逐条照抄，顺序同原版）
const VANILLA_KEYBINDS: &[(&str, &str)] = &[
    ("key_key.forward", "key.keyboard.w"),
    ("key_key.back", "key.keyboard.s"),
    ("key_key.left", "key.keyboard.a"),
    ("key_key.right", "key.keyboard.d"),
    ("key_key.jump", "key.keyboard.space"),
    ("key_key.sneak", "key.keyboard.left.shift"),
    ("key_key.sprint", "key.keyboard.left.control"),
    ("key_key.attack", "key.mouse.left"),
    ("key_key.use", "key.mouse.right"),
    ("key_key.pickItem", "key.mouse.middle"),
    ("key_key.inventory", "key.keyboard.e"),
    ("key_key.drop", "key.keyboard.q"),
    ("key_key.swapOffhand", "key.keyboard.f"),
    ("key_key.hotbar.1", "key.keyboard.1"),
    ("key_key.hotbar.2", "key.keyboard.2"),
    ("key_key.hotbar.3", "key.keyboard.3"),
    ("key_key.hotbar.4", "key.keyboard.4"),
    ("key_key.hotbar.5", "key.keyboard.5"),
    ("key_key.hotbar.6", "key.keyboard.6"),
    ("key_key.hotbar.7", "key.keyboard.7"),
    ("key_key.hotbar.8", "key.keyboard.8"),
    ("key_key.hotbar.9", "key.keyboard.9"),
    ("key_key.togglePerspective", "key.keyboard.f5"),
    ("key_key.smoothCamera", "key.keyboard.unknown"),
    ("key_key.zoom", "key.keyboard.c"),
    ("key_key.chat", "key.keyboard.t"),
    ("key_key.command", "key.keyboard.slash"),
    ("key_key.socialInteractions", "key.keyboard.p"),
    ("key_key.advancements", "key.keyboard.l"),
    ("key_key.screenshot", "key.keyboard.f2"),
    ("key_key.fullscreen", "key.keyboard.f11"),
    ("key_key.narrator", "key.keyboard.b"),
    ("key_key.playerlist", "key.keyboard.tab"),
    ("key_key.saveToolbarActivator", "key.keyboard.unknown"),
    ("key_key.loadToolbarActivator", "key.keyboard.unknown"),
    ("key_key.spectatorOutlines", "key.keyboard.unknown")
];

fn default_keys_file(app: &tauri::AppHandle) -> PathBuf {
    crate::settings::user_data_dir(app).join("default-keys.json")
}

fn read_stored_keys(app: &tauri::AppHandle) -> Value {
    std::fs::read(default_keys_file(app))
        .ok()
        .and_then(|raw| serde_json::from_slice::<Value>(&raw).ok())
        .unwrap_or(json!({}))
}

fn default_keys(app: &tauri::AppHandle) -> Value {
    let stored = read_stored_keys(app);
    let mut out = serde_json::Map::new();
    for (id, def) in VANILLA_KEYBINDS {
        let v = stored[*id].as_str().filter(|s| BIND_RE_OK(s)).unwrap_or(def);
        out.insert(id.to_string(), json!(v));
    }
    Value::Object(out)
}

fn persist_keys(app: &tauri::AppHandle, keys: &Value) {
    let file = default_keys_file(app);
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(keys) {
        let _ = std::fs::write(file, bytes);
    }
}

/// 对应原版 keybindings.ts mergeKeysIntoOptions + syncKeysToGameDir：
/// 把默认键位写进实例 options.txt 的 key_* 项，其余行原样保留。返回是否有改动。
pub fn sync_keys_to_game_dir(app: &tauri::AppHandle, game_dir: &str) -> bool {
    let keys = default_keys(app);
    let file = std::path::Path::new(game_dir).join("options.txt");
    let before = std::fs::read_to_string(&file).unwrap_or_default();
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut out: Vec<String> = Vec::new();
    for line in before.split('\n') {
        let id = line.split(':').next().unwrap_or("");
        let value = keys[id].as_str().filter(|_| id.starts_with("key_"));
        match value {
            Some(v) if !seen.contains(id) => {
                seen.insert(id);
                out.push(format!("{id}:{v}"));
            }
            Some(_) => {} // 重复行直接丢弃（原版同语义）
            None => out.push(line.to_string()),
        }
    }
    for (id, def) in VANILLA_KEYBINDS {
        let current = keys[*id].as_str().unwrap_or(def);
        if !seen.contains(id) {
            out.push(format!("{id}:{current}"));
        }
    }
    let after = format!("{}\n", out.join("\n").trim_end_matches('\n'));
    if after == before {
        return false;
    }
    std::fs::write(&file, after).is_ok()
}

// ---------------- 默认游戏选项（defaultGameOptions.ts） ----------------

// ---------------- 默认资源包（defaultResourcePacks.ts） ----------------

fn default_packs_file(app: &tauri::AppHandle) -> PathBuf {
    crate::settings::user_data_dir(app).join("default-resource-packs.json")
}

fn load_packs(app: &tauri::AppHandle) -> Vec<Value> {
    std::fs::read(default_packs_file(app))
        .ok()
        .and_then(|raw| serde_json::from_slice::<Value>(&raw).ok())
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
}

fn save_packs(app: &tauri::AppHandle, list: &[Value]) {
    let file = default_packs_file(app);
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(list) {
        let _ = std::fs::write(file, bytes);
    }
}

// ---------------- 玩偶计数（mascots.ts） ----------------

fn mascots_file(app: &tauri::AppHandle) -> PathBuf {
    crate::settings::user_data_dir(app).join("mascot-counts.json")
}

fn mascot_state(app: &tauri::AppHandle) -> Value {
    std::fs::read(mascots_file(app))
        .ok()
        .and_then(|raw| serde_json::from_slice::<Value>(&raw).ok())
        .unwrap_or_else(|| json!({ "counts": {}, "order": [], "sound": { "muted": false, "volume": 0.5 } }))
}

fn save_mascots(app: &tauri::AppHandle, state: &Value) {
    let file = mascots_file(app);
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(state) {
        let _ = std::fs::write(file, bytes);
    }
}

// ---------------- 退出历史（exitHistory.ts + exitJournal.ts） ----------------

fn exit_history_file(app: &tauri::AppHandle) -> PathBuf {
    crate::settings::user_data_dir(app).join("exit-history.json")
}

pub(crate) fn load_exit_history(app: &tauri::AppHandle) -> Vec<Value> {
    std::fs::read(exit_history_file(app))
        .ok()
        .and_then(|raw| serde_json::from_slice::<Value>(&raw).ok())
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
}

fn save_exit_history(app: &tauri::AppHandle, list: &[Value]) {
    let file = exit_history_file(app);
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(list) {
        let _ = std::fs::write(file, bytes);
    }
}

// ---------------- 插件（plugins.ts：userData/plugins/<id>/） ----------------

fn plugins_root(app: &tauri::AppHandle) -> PathBuf {
    crate::settings::user_data_dir(app).join("plugins")
}

fn list_plugins(app: &tauri::AppHandle) -> Value {
    let root = plugins_root(app);
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&root) {
        for e in entries.flatten() {
            if !e.path().is_dir() {
                continue;
            }
            let manifest_path = e.path().join("plugin.json");
            let Ok(raw) = std::fs::read(&manifest_path) else { continue };
            let Ok(mut info) = serde_json::from_slice::<Value>(&raw) else { continue };
            if let Some(obj) = info.as_object_mut() {
                obj.entry("id".to_string()).or_insert_with(|| json!(e.file_name().to_string_lossy()));
                obj.insert("dir".to_string(), json!(e.path().to_string_lossy()));
            }
            out.push(info);
        }
    }
    Value::Array(out)
}

// ---------------- 通道分发 ----------------

pub fn invoke(app: &tauri::AppHandle, channel: &str, args: &[Value]) -> Option<Result<Value, String>> {
    match channel {
        // 默认按键
        "keys:getDefault" => Some(Ok(default_keys(app))),
        "keys:setDefault" => Some((|| {
            let id = args.first().and_then(|v| v.as_str()).unwrap_or("");
            let bind = args.get(1).and_then(|v| v.as_str()).unwrap_or("");
            if !VANILLA_KEYBINDS.iter().any(|(k, _)| *k == id) {
                return Err("未知的键位项".into());
            }
            if !BIND_RE_OK(bind) {
                return Err("键位值格式无效".into());
            }
            let mut keys = default_keys(app);
            if let Some(obj) = keys.as_object_mut() {
                obj.insert(id.to_string(), json!(bind));
            }
            persist_keys(app, &keys);
            Ok(keys)
        })()),
        "keys:reset" => Some((|| {
            // default_keys() 是"原版默认 + 已存覆盖"的合并结果；重置必须清掉覆盖文件，
            // 否则等于把当前值原样存回，用户看到"已恢复"实际没变。
            let _ = std::fs::remove_file(default_keys_file(app));
            Ok(default_keys(app))
        })()),

        // 默认游戏选项：形状与校验语义对齐原版 defaultGameOptions.ts
        "gameOptions:get" => Some(Ok(crate::gameoptions::get(app))),
        "gameOptions:set" => Some(crate::gameoptions::set(app, &args.first().cloned().unwrap_or(json!({})))),

        // 默认资源包
        "defaultPacks:get" => Some(Ok(Value::Array(load_packs(app)))),
        "defaultPacks:import" => Some((|| {
            let files = args.first().and_then(|v| v.as_array()).cloned().unwrap_or_default();
            let mut list = load_packs(app);
            let packs_root = crate::settings::user_data_dir(app).join("default-resource-packs");
            let _ = std::fs::create_dir_all(&packs_root);
            for f in files {
                let Some(src) = f.as_str() else { continue };
                let src_path = PathBuf::from(src);
                let name = src_path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default().to_string();
                let id = format!("pack-{}", &crate::accounts::uuid_v4()[..8]);
                let dest = packs_root.join(&name);
                std::fs::copy(&src_path, &dest).map_err(|e| e.to_string())?;
                list.push(json!({
                    "id": id, "name": name.trim_end_matches(".zip").to_string(),
                    "file": dest.to_string_lossy(), "enabled": true,
                    "time": chrono::Utc::now().timestamp_millis()
                }));
            }
            save_packs(app, &list);
            Ok(Value::Array(list))
        })()),
        "defaultPacks:pick" => Some((|| {
            use tauri_plugin_dialog::DialogExt;
            let picked = app
                .dialog()
                .file()
                .add_filter("资源包", &["zip"])
                .blocking_pick_files();
            let files: Vec<Value> = picked
                .map(|ps| ps.iter().map(|p| json!(p.to_string())).collect())
                .unwrap_or_default();
            import_packs_from(app, files)
        })()),
        "defaultPacks:remove" => Some((|| {
            let id = args.first().and_then(|v| v.as_str()).unwrap_or("");
            let mut list = load_packs(app);
            if let Some(pos) = list.iter().position(|p| p["id"].as_str() == Some(id)) {
                let item = list.remove(pos);
                if let Some(file) = item["file"].as_str() {
                    let _ = std::fs::remove_file(file);
                }
            }
            save_packs(app, &list);
            Ok(Value::Array(list))
        })()),
        "defaultPacks:move" => Some((|| {
            let id = args.first().and_then(|v| v.as_str()).unwrap_or("");
            let direction = args.get(1).and_then(|v| v.as_i64()).unwrap_or(0);
            let mut list = load_packs(app);
            if let Some(pos) = list.iter().position(|p| p["id"].as_str() == Some(id)) {
                let new_pos = (pos as i64 + direction).clamp(0, list.len() as i64 - 1) as usize;
                let item = list.remove(pos);
                list.insert(new_pos, item);
            }
            save_packs(app, &list);
            Ok(Value::Array(list))
        })()),
        "defaultPacks:setEnabled" => Some((|| {
            let id = args.first().and_then(|v| v.as_str()).unwrap_or("");
            let enabled = args.get(1).and_then(|v| v.as_bool()).unwrap_or(false);
            let mut list = load_packs(app);
            if let Some(item) = list.iter_mut().find(|p| p["id"].as_str() == Some(id)) {
                item["enabled"] = json!(enabled);
            }
            save_packs(app, &list);
            Ok(Value::Array(list))
        })()),
        "defaultPacks:apply" => Some(Err("默认资源包应用到实例尚未接入".into())),

        // 玩偶
        "mascots:state" => Some(Ok(mascot_state(app))),
        "mascots:batch" => Some((|| {
            let mut state = mascot_state(app);
            let hits = args.first().and_then(|v| v.get("hits")).and_then(|h| h.as_array()).cloned().unwrap_or_default();
            let counts = state["counts"].as_object_mut().ok_or("玩偶状态损坏")?;
            for h in hits {
                let id = h.as_str().unwrap_or("kamu").to_string();
                *counts.entry(id).or_insert(json!(0)) = json!(counts.get(&h.as_str().unwrap_or("kamu").to_string()).and_then(|v| v.as_i64()).unwrap_or(0) + 1);
            }
            save_mascots(app, &state);
            Ok(state)
        })()),
        "mascots:slap" => Some((|| {
            let id = args.first().and_then(|v| v.as_str()).unwrap_or("kamu").to_string();
            let mut state = mascot_state(app);
            let counts = state["counts"].as_object_mut().ok_or("玩偶状态损坏")?;
            *counts.entry(id).or_insert(json!(0)) = json!(counts.values().filter_map(|v| v.as_i64()).sum::<i64>().max(0) + 1);
            save_mascots(app, &state);
            Ok(state)
        })()),
        "mascots:sound" => Some((|| {
            let mut state = mascot_state(app);
            let v = args.first().cloned().unwrap_or(json!({}));
            if let Some(muted) = v["muted"].as_bool() {
                state["sound"]["muted"] = json!(muted);
            }
            if let Some(volume) = v["volume"].as_f64() {
                state["sound"]["volume"] = json!(volume.clamp(0.0, 1.0));
            }
            save_mascots(app, &state);
            Ok(state)
        })()),
        "mascots:reset" => Some((|| {
            let mut state = mascot_state(app);
            if args.first().and_then(|v| v.as_bool()).unwrap_or(false) {
                state["counts"] = json!({});
            }
            save_mascots(app, &state);
            Ok(state)
        })()),

        // 退出历史
        "exitHistory:list" => Some(Ok(Value::Array(load_exit_history(app)))),
        "exitHistory:ack" => Some((|| {
            let mut list = load_exit_history(app);
            for item in list.iter_mut() {
                item["seen"] = json!(true);
            }
            save_exit_history(app, &list);
            Ok(Value::Array(list))
        })()),
        "exitHistory:clear" => Some((|| {
            save_exit_history(app, &[]);
            Ok(Value::Array(vec![]))
        })()),

        // 插件
        "plugins:list" => Some(Ok(list_plugins(app))),
        "plugins:install" => Some((|| {
            use tauri_plugin_dialog::DialogExt;
            let picked = app.dialog().file().add_filter("插件脚本", &["js"]).blocking_pick_file();
            let Some(path) = picked else { return Ok(list_plugins(app)) };
            let code = std::fs::read_to_string(path.to_string()).map_err(|e| e.to_string())?;
            // plugin.json：脚本首行 // name: xxx 或文件名
            let name = code
                .lines()
                .find_map(|l| l.trim().strip_prefix("// name:").map(str::trim))
                .map(str::to_string)
                .unwrap_or_else(|| {
                    PathBuf::from(path.to_string())
                        .file_stem()
                        .map(|n| n.to_string_lossy())
                        .unwrap_or_default()
                        .to_string()
                });
            let id = crate::accounts::uuid_v4();
            let dir = plugins_root(app).join(&id);
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            std::fs::write(dir.join("plugin.json"), serde_json::to_vec_pretty(&json!({ "id": id, "name": name, "enabled": true, "installedAt": chrono::Utc::now().timestamp_millis() })).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            std::fs::write(dir.join("main.js"), code).map_err(|e| e.to_string())?;
            Ok(list_plugins(app))
        })()),
        "plugins:setEnabled" => Some((|| {
            let id = args.first().and_then(|v| v.as_str()).unwrap_or("");
            let enabled = args.get(1).and_then(|v| v.as_bool()).unwrap_or(false);
            let manifest = plugins_root(app).join(id).join("plugin.json");
            let mut info: Value = std::fs::read(&manifest).ok().and_then(|r| serde_json::from_slice(&r).ok()).unwrap_or(json!({}));
            info["enabled"] = json!(enabled);
            std::fs::write(&manifest, serde_json::to_vec_pretty(&info).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            Ok(list_plugins(app))
        })()),
        "plugins:remove" => Some((|| {
            let id = args.first().and_then(|v| v.as_str()).unwrap_or("");
            let _ = std::fs::remove_dir_all(plugins_root(app).join(id));
            Ok(list_plugins(app))
        })()),
        "plugins:readCode" => Some((|| {
            let id = args.first().and_then(|v| v.as_str()).unwrap_or("");
            let manifest = plugins_root(app).join(id).join("plugin.json");
            let info: Value = std::fs::read(&manifest).ok().and_then(|r| serde_json::from_slice(&r).ok()).unwrap_or(json!({}));
            if info["enabled"] != true {
                return Err("仅启用的插件可读取代码".into());
            }
            let code = std::fs::read_to_string(plugins_root(app).join(id).join("main.js")).map_err(|e| e.to_string())?;
            Ok(json!(code))
        })()),
        "plugins:openDir" => Some((|| {
            let dir = plugins_root(app);
            let _ = std::fs::create_dir_all(&dir);
            std::process::Command::new("explorer.exe").arg(dir.as_os_str()).spawn().map_err(|e| e.to_string())?;
            Ok(Value::Null)
        })()),

        // 内存整理（memoryOrganizer：EnumProcesses + EmptyWorkingSet，跳过游戏与系统进程）
        "memory:organize" => Some(crate::installer::run_blocking(organize_memory)),

        _ => None,
    }
}

// ---------------- 内存整理（对应原版 core/memoryOrganizer.ts） ----------------
// 用 PSAPI 的 EmptyWorkingSet 把各进程的工作集换出，回收待机内存；
// 跳过正在运行的游戏（换出会让游戏掉帧）与系统关键进程（dwm/csrss/lsass/…）。

#[cfg(windows)]
mod mem_organize {
    use serde_json::{json, Value};
    use std::collections::BTreeMap;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::ProcessStatus::{EmptyWorkingSet, EnumProcesses};
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_QUOTA,
    };

    /// 系统目录（用于跳过 Windows 自身的进程）
    fn windows_dir() -> String {
        std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string())
    }

    /// 对应 shouldSkipMemoryProcess：pid≤4、运行中游戏、系统关键进程、Windows 目录下的进程
    fn should_skip(pid: u32, executable: &str, protected: &[u32]) -> bool {
        let path = executable.replace('\\', "/").to_lowercase();
        let name = path.rsplit('/').next().unwrap_or("").to_string();
        if pid <= 4 || protected.contains(&pid) || name.is_empty() {
            return true;
        }
        let critical = regex::Regex::new(
            r"^(javaw?|minecraft.*|dwm|csrss|lsass|smss|wininit|winlogon|services)\.exe$",
        )
        .map(|re| re.is_match(&name))
        .unwrap_or(false);
        if critical {
            return true;
        }
        let system = windows_dir().replace('\\', "/").to_lowercase();
        let system = system.trim_end_matches('/').to_string();
        !system.is_empty() && path.starts_with(&format!("{system}/"))
    }

    /// 取进程可执行文件全路径（失败返回 None）
    fn process_image(handle: HANDLE) -> Option<String> {
        let mut buf = vec![0u16; 32768];
        let mut size = buf.len() as u32;
        unsafe {
            QueryFullProcessImageNameW(
                handle,
                PROCESS_NAME_FORMAT(0),
                windows::core::PWSTR(buf.as_mut_ptr()),
                &mut size,
            )
            .ok()?;
        }
        buf.truncate(size as usize);
        Some(String::from_utf16_lossy(&buf))
    }

    pub fn run() -> Result<Value, String> {
        let started = std::time::Instant::now();
        let before = crate::sysinfo::available_memory_mb();
        let mut processed = 0u64;
        let mut skipped = 0u64;
        let mut failures: BTreeMap<String, u64> = BTreeMap::new();
        let protected = crate::launch::running_game_pids();

        // EnumProcesses：进程清单可能变大，循环扩容直到装得下
        let mut buf = vec![0u32; 1024];
        let mut needed = 0u32;
        loop {
            unsafe {
                EnumProcesses(
                    buf.as_mut_ptr(),
                    (buf.len() * std::mem::size_of::<u32>()) as u32,
                    &mut needed,
                )
                .map_err(|e| format!("无法枚举进程：{e}"))?;
            }
            if (needed as usize) < buf.len() * std::mem::size_of::<u32>() {
                break;
            }
            if buf.len() >= 262_144 {
                return Err("进程清单超过上限".into());
            }
            buf.resize(buf.len() * 2, 0);
        }
        let count = needed as usize / std::mem::size_of::<u32>();

        for (i, pid) in buf[..count].iter().copied().enumerate() {
            if i % 8 == 0 {
                std::thread::yield_now();
            }
            if pid == 0 || pid <= 4 || protected.contains(&pid) {
                skipped += 1;
                continue;
            }
            // PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_QUOTA，不提权
            let handle = match unsafe {
                OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_QUOTA, false, pid)
            } {
                Ok(h) => h,
                Err(_) => {
                    skipped += 1;
                    *failures.entry("打开进程失败（权限不足）".to_string()).or_insert(0) += 1;
                    continue;
                }
            };
            let Some(path) = process_image(handle) else {
                skipped += 1;
                *failures.entry("无法识别进程".to_string()).or_insert(0) += 1;
                unsafe {
                    let _ = CloseHandle(handle);
                }
                continue;
            };
            if should_skip(pid, &path, &protected) {
                skipped += 1;
                unsafe {
                    let _ = CloseHandle(handle);
                }
                continue;
            }
            let ok = unsafe { EmptyWorkingSet(handle) }.is_ok();
            unsafe {
                let _ = CloseHandle(handle);
            }
            if ok {
                processed += 1;
            } else {
                skipped += 1;
                *failures.entry("整理失败".to_string()).or_insert(0) += 1;
            }
        }

        // 换出后等一小段再测可用内存，否则读到的还是换出前的值
        std::thread::sleep(std::time::Duration::from_millis(300));
        let after = crate::sysinfo::available_memory_mb();
        Ok(json!({
            "applicable": true,
            "beforeMB": before,
            "afterMB": after,
            "processed": processed,
            "skipped": skipped,
            "failures": failures,
            "elapsedMs": started.elapsed().as_millis() as u64
        }))
    }
}

#[cfg(not(windows))]
mod mem_organize {
    use serde_json::{json, Value};
    pub fn run() -> Result<Value, String> {
        Ok(json!({
            "applicable": false,
            "beforeMB": 0, "afterMB": 0,
            "processed": 0, "skipped": 0,
            "failures": {}, "elapsedMs": 0
        }))
    }
}

fn organize_memory() -> Result<Value, String> {
    mem_organize::run()
}

/// 供启动流程调用（「启动前整理内存」开关）：对应原版 launch.ts 里的 organizeMemory()
pub fn run_memory_organize() -> Result<Value, String> {
    mem_organize::run()
}

fn import_packs_from(app: &tauri::AppHandle, files: Vec<Value>) -> Result<Value, String> {
    let mut list = load_packs(app);
    let packs_root = crate::settings::user_data_dir(app).join("default-resource-packs");
    let _ = std::fs::create_dir_all(&packs_root);
    for f in files {
        let Some(src) = f.as_str() else { continue };
        let src_path = PathBuf::from(src);
        let name = src_path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default().to_string();
        let id = format!("pack-{}", &crate::accounts::uuid_v4()[..8]);
        let dest = packs_root.join(&name);
        std::fs::copy(&src_path, &dest).map_err(|e| e.to_string())?;
        list.push(json!({
            "id": id, "name": name.trim_end_matches(".zip").to_string(),
            "file": dest.to_string_lossy(), "enabled": true,
            "time": chrono::Utc::now().timestamp_millis()
        }));
    }
    save_packs(app, &list);
    Ok(Value::Array(list))
}
