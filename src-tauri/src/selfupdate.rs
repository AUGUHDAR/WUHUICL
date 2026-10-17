//! 自更新（对应原版 core/selfUpdate + applyUpdate 的通道合同面）。
//! WUHUICL 独立版本线：update:check 查 GitHub Releases（AUGUHDAR/WUHUICL）。
//! 下载走 GitHub 直连 + ghproxy 国内镜像双候选（多候选引擎自动择优/换源），
//! SHA-256 按 Release asset digest 校验；apply 写替换脚本，进程退出后完成替换并重启。

use serde_json::{json, Value};
use tauri::{Emitter, Manager};

const REPO_API: &str = "https://api.github.com/repos/AUGUHDAR/WUHUICL/releases";
/// 国内镜像前缀（gh-proxy 系），与 GitHub 直连组成下载候选
const GH_MIRRORS: &[&str] = &["https://ghproxy.net/", "https://gh-proxy.com/"];

fn current_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// update:check → UpdateCheckResult {ok, hasUpdate, release?, reason?, fromCache?}
fn check(app: &tauri::AppHandle, force: bool) -> Value {
    let _ = force;
    let resp = crate::net::client()
        .get(format!("{REPO_API}/latest"))
        .header("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(15))
        .send();
    let Ok(resp) = resp else {
        return json!({ "ok": false, "hasUpdate": false, "reason": "网络不可达" });
    };
    if !resp.status().is_success() {
        return json!({ "ok": false, "hasUpdate": false, "reason": format!("HTTP {}", resp.status().as_u16()) });
    }
    let Ok(data) = resp.json::<Value>() else {
        return json!({ "ok": false, "hasUpdate": false, "reason": "响应解析失败" });
    };
    let tag = data["tag_name"].as_str().unwrap_or("").trim_start_matches(['v', 'V']).to_string();
    let has_update = is_newer(&tag, &current_version());
    let assets: Vec<Value> = data["assets"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|a| {
            let name = a["name"].as_str()?.to_string();
            if !name.to_lowercase().ends_with(".exe") {
                return None;
            }
            Some(json!({
                "name": name,
                "url": a["browser_download_url"],
                "size": a["size"].as_u64().unwrap_or(0)
            }))
        })
        .collect();
    json!({
        "ok": true,
        "hasUpdate": has_update,
        "release": if has_update { json!({
            "version": tag,
            "name": data["name"].as_str().unwrap_or(""),
            "body": data["body"].as_str().unwrap_or(""),
            "date": data["published_at"].as_str().unwrap_or(""),
            "assets": assets,
            "rollback": false
        }) } else { Value::Null }
    })
}

/// 语义化版本比较：tag > current
fn is_newer(tag: &str, current: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> {
        s.split('.')
            .map(|p| p.chars().filter(|c| c.is_ascii_digit()).collect::<String>().parse().unwrap_or(0))
            .collect()
    };
    let (a, b) = (parse(tag), parse(current));
    for i in 0..a.len().max(b.len()) {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        if x != y {
            return x > y;
        }
    }
    false
}

/// update:getConfigStatus → {configVersion, current, mismatch}
fn config_status(app: &tauri::AppHandle) -> Value {
    let s = crate::settings::get(app);
    let stored = s["configVersion"].as_i64().unwrap_or(1);
    json!({
        "configVersion": stored,
        "current": 1,
        "mismatch": if stored > 1 { json!("newer") } else { Value::Null }
    })
}

/// update:listReleases → ReleaseInfo[]
fn list_releases() -> Value {
    let Ok(resp) = crate::net::client()
        .get(REPO_API)
        .header("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(15))
        .send()
    else {
        return json!([]);
    };
    if !resp.status().is_success() {
        return json!([]);
    }
    let Ok(data) = resp.json::<Value>() else { return json!([]) };
    Value::Array(
        data.as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|r| {
                json!({
                    "version": r["tag_name"].as_str().unwrap_or("").trim_start_matches(['v', 'V']),
                    "name": r["name"].as_str().unwrap_or(""),
                    "body": r["body"].as_str().unwrap_or(""),
                    "date": r["published_at"].as_str().unwrap_or(""),
                    "rollback": false
                })
            })
            .collect(),
    )
}

/// 更新提示事件（启动检查发现新版本时推 updatePrompt——对应 index.ts 启动链）
pub fn startup_check(app: &tauri::AppHandle) {
    let handle = app.clone();
    std::thread::spawn(move || {
        let result = check(&handle, false);
        if result["ok"] == true && result["hasUpdate"] == true {
            if let Some(win) = handle.get_webview_window("main") {
                let _ = win.emit_to("main", "event:updatePrompt", result["release"].clone());
            }
        }
    });
}

// ---------------- 下载与应用（update:start / update:apply） ----------------

fn update_dir(app: &tauri::AppHandle) -> std::path::PathBuf {
    crate::settings::user_data_dir(app).join("update")
}

/// asset digest（"sha256:..."）→ hex
fn asset_digest(asset: &Value) -> Option<String> {
    let d = asset["digest"].as_str()?;
    d.strip_prefix("sha256:")
        .map(|h| h.to_lowercase())
        .filter(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// update:start(release, mode) → {taskId}；后台下载新 exe 到 update 目录。
/// 进度走 event:progress {taskId,...}，完成 event:taskDone + event:updateReady，失败 event:taskDone(ok=false)。
fn start_download(app: &tauri::AppHandle, release: &Value, _mode: &str) -> Result<Value, String> {
    let version = release["version"].as_str().unwrap_or("").to_string();
    let assets = release["assets"].as_array().ok_or("更新包清单缺失")?;
    let asset = assets
        .iter()
        .find(|a| {
            a["name"]
                .as_str()
                .map(|n| n.to_lowercase().ends_with(".exe"))
                .unwrap_or(false)
        })
        .ok_or("更新包里没有 exe")?;
    let url = asset["url"]
        .as_str()
        .ok_or("更新包下载地址缺失")?
        .to_string();
    let expected = asset_digest(asset);
    let size = asset["size"].as_u64();

    let dir = update_dir(app);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let dest = dir.join(format!("wuhuicl-{version}.exe.new"));
    let rec = crate::tasks::register_task(&format!("下载更新 {version}"), "update");
    let task_id = rec.id.clone();
    let task_id_thread = task_id.clone();

    let app_handle = app.clone();
    std::thread::spawn(move || {
        let task_id = task_id_thread;
        let mut candidates = vec![url.clone()];
        for m in GH_MIRRORS {
            candidates.push(format!("{}{}", m, url));
        }
        let mut last_err = String::new();
        let mut downloaded: Option<u64> = None;
        for (round, candidate) in candidates.iter().enumerate() {
            if crate::tasks::task_cancelled(&task_id) {
                break;
            }
            if round > 0 {
                crate::tasks::emit_progress(
                    &app_handle,
                    json!({ "taskId": task_id, "stage": "download", "progress": 0, "text": format!("换源重试 ({}/{})", round + 1, candidates.len()) }),
                );
            }
            let app2 = app_handle.clone();
            let tid = task_id.clone();
            let progress: crate::installer::FileProgress = &move |done: u64, total: Option<u64>| {
                crate::tasks::emit_progress(
                    &app2,
                    json!({
                        "taskId": tid,
                        "stage": "download",
                        "progress": total.map(|t| (done as f64 / t as f64) * 0.95).unwrap_or(0.0),
                        "bytesDone": done,
                        "bytesTotal": total.map(|t| json!(t)).unwrap_or(Value::Null),
                        "indeterminate": total.is_none(),
                        "speed": 0,
                        "text": format!("下载更新包 {}", crate::installer::fmt_mb(done))
                    }),
                );
            };
            match crate::installer::download_file(
                &app_handle,
                Some(&rec),
                None,
                candidate,
                &dest,
                None,
                size,
                "",
                Some(progress),
                &[],
                &[],
            ) {
                Ok(n) => {
                    downloaded = Some(n);
                    break;
                }
                Err(e) => last_err = e,
            }
        }
        match downloaded {
            Some(n) => {
                let ok = match &expected {
                    Some(h) => sha256_file_hex(&dest).map(|a| a == *h).unwrap_or(false),
                    None => true,
                };
                if !ok {
                    let _ = std::fs::remove_file(&dest);
                    crate::tasks::task_done(
                        &app_handle,
                        &task_id,
                        false,
                        Some("更新包 SHA-256 校验失败".into()),
                        false,
                        Some("download".into()),
                    );
                    return;
                }
                crate::tasks::task_done(&app_handle, &task_id, true, None, false, Some("download".into()));
                if let Some(win) = app_handle.get_webview_window("main") {
                    let _ = win.emit_to(
                        "main",
                        "event:updateReady",
                        json!({ "version": version, "path": dest.to_string_lossy(), "size": n }),
                    );
                }
            }
            None => {
                let _ = std::fs::remove_file(&dest);
                crate::tasks::task_done(
                    &app_handle,
                    &task_id,
                    false,
                    Some(format!("更新下载失败：{last_err}")),
                    crate::tasks::task_cancelled(&task_id),
                    Some("download".into()),
                );
            }
        }
    });
    Ok(json!({ "taskId": task_id }))
}

fn sha256_file_hex(path: &std::path::Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    Ok(hex::encode(Sha256::digest(&bytes)))
}

/// update:apply(release)：写替换脚本（等当前进程退出 → 换 exe → 重启）然后退出。
/// 原版 .kamuclupdate 的"下次启动替换"等价物；脚本内保留 .old 备份。
fn apply_update(app: &tauri::AppHandle, release: &Value) -> Result<(), String> {
    let version = release["version"].as_str().unwrap_or("");
    let dir = update_dir(app);
    let staged = dir.join(format!("wuhuicl-{version}.exe.new"));
    if !staged.exists() {
        return Err("更新包尚未下载完成".into());
    }
    let current = std::env::current_exe().map_err(|e| e.to_string())?;
    let target = current.canonicalize().unwrap_or(current);
    let bat = dir.join("apply-update.bat");
    // 等当前 PID 退出（最多 60 秒）→ 备份旧版 → 替换 → 重启
    let script = format!(
        "@echo off

setlocal

set \"TARGET={target}\"

set \"STAGED={staged}\"

for /l %%i in (1,1,60) do (

  tasklist /FI \"PID eq {pid}\" | find \"{pid}\" >nul 2>&1 || goto :replace

  timeout /t 1 /nobreak >nul

)

:replace

if exist \"%TARGET%.old\" del /f /q \"%TARGET%.old\"

copy /y \"%TARGET%\" \"%TARGET%.old\" >nul

move /y \"%STAGED%\" \"%TARGET%\" >nul || goto :fail

start \"\" \"%TARGET%\"

exit /b 0

:fail

exit /b 1

",
        target = target.to_string_lossy(),
        staged = staged.to_string_lossy(),
        pid = std::process::id(),
    );
    std::fs::write(&bat, script).map_err(|e| e.to_string())?;
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    Command::new("cmd")
        .args(["/C", &bat.to_string_lossy()])
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .spawn()
        .map_err(|e| e.to_string())?;
    std::thread::sleep(std::time::Duration::from_millis(300));
    app.exit(0);
    Ok(())
}

/// 统一入口
pub fn invoke(app: &tauri::AppHandle, channel: &str, args: &[Value]) -> Option<Result<Value, String>> {
    match channel {
        "update:check" => Some(Ok(check(
            app,
            args.first().and_then(|v| v.as_bool()).unwrap_or(false),
        ))),
        "update:getState" | "update:getPending" => Some(Ok(Value::Null)),
        "update:getConfigStatus" => Some(Ok(config_status(app))),
        "update:listReleases" => Some(Ok(list_releases())),
        "update:skip" => Some((|| {
            let version = args.first().and_then(|v| v.as_str()).unwrap_or("");
            crate::settings::save(app, json!({ "skipUpdateVersion": version }))?;
            Ok(Value::Null)
        })()),
        "update:resetSettings" => Some((|| {
            let file = crate::settings::settings_file(app);
            if file.exists() {
                let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
                let backup = file.with_file_name(format!("settings.backup-{stamp}.json"));
                let _ = std::fs::copy(&file, backup);
            }
            let mut def = crate::settings::defaults();
            def["configVersion"] = json!(1);
            let path = crate::settings::settings_file(app);
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            std::fs::write(&path, serde_json::to_vec_pretty(&def).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            Ok(Value::Null)
        })()),
        "update:start" => {
            let release = args.first().cloned().unwrap_or(Value::Null);
            let mode = args
                .get(1)
                .and_then(|v| v.as_str())
                .unwrap_or("upgrade")
                .to_string();
            Some(start_download(app, &release, &mode))
        }
        "update:apply" => {
            let release = args.first().cloned().unwrap_or(Value::Null);
            Some(apply_update(app, &release).map(|_| Value::Null))
        }
        "update:applyLocal" | "update:restoreBackup" => {
            Some(Err("备份恢复未启用：更新脚本自带 .old 回滚".into()))
        }
        "update:applyPending" => Some(Ok(Value::Bool(false))),
        "update:pickLocalFile" => Some(Ok(Value::Null)),
        _ => None,
    }
}
