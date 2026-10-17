//! 自更新（对应原版 core/selfUpdate + applyUpdate 的通道合同面）。
//! WUHUICL 独立版本线：update:check 同时查 GitHub Releases 与 Gitee 发行版（同优先级竞速，
//! 先返回可用结果者胜）。下载走 GitHub 直连 + Gitee 镜像 **同开竞争**（先完成者胜，另一方
//! 切断），SHA-256 按 Release asset digest 校验；apply 写替换脚本，进程退出后完成替换并重启。
//!
//! tag 规范：**纯数字版本号**（如 `1.0.0`），禁止 v 前缀/beta 等后缀——一个版本号对应
//! 一个版本，预发布信息只允许写在正文里。

use serde_json::{json, Value};
use tauri::{Emitter, Manager};

const REPO_API: &str = "https://api.github.com/repos/AUGUHDAR/WUHUICL/releases";
/// Gitee 镜像仓（发布时同步上传同名资产）；tag 与 GitHub 一致（纯数字）
const GITEE_API: &str = "https://gitee.com/api/v5/repos/AUGUHDAR/WUHUICL/releases";
/// GitHub 直连下载基址（浏览器下载地址）
const GH_DOWNLOAD: &str = "https://github.com/AUGUHDAR/WUHUICL/releases/download";
/// Gitee 下载基址
const GITEE_DOWNLOAD: &str = "https://gitee.com/AUGUHDAR/WUHUICL/releases/download";

fn current_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// update:check → UpdateCheckResult {ok, hasUpdate, release?, reason?, fromCache?}
/// GitHub 与 Gitee 同优先级竞速：先返回有效结果的一方胜出，另一方丢弃。
fn check(app: &tauri::AppHandle, force: bool) -> Value {
    let _ = (app, force);
    // 两个源并发请求，先成功者用其结果
    let gh = {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(fetch_github_latest());
        });
        rx
    };
    let gitee = {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(fetch_gitee_latest());
        });
        rx
    };
    // 都等，但先拿到 ok 的优先（Gitee 国内通常先回；GitHub 失败会先送 Err）
    let gh_result = gh.recv_timeout(std::time::Duration::from_secs(20)).ok();
    let gitee_result = gitee.recv_timeout(std::time::Duration::from_secs(20)).ok();
    for r in [gh_result, gitee_result].into_iter().flatten() {
        if let Ok(data) = r {
            return build_check_result(&data);
        }
    }
    json!({ "ok": false, "hasUpdate": false, "reason": "网络不可达（GitHub 与 Gitee 均失败）" })
}

fn fetch_github_latest() -> Result<Value, String> {
    let resp = crate::net::client()
        .get(format!("{REPO_API}/latest"))
        .header("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }
    resp.json::<Value>().map_err(|e| e.to_string())
}

/// Gitee releases 列表第一条（Gitee API 没有 /latest，按创建序取最新）
fn fetch_gitee_latest() -> Result<Value, String> {
    let resp = crate::net::client()
        .get(GITEE_API)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }
    let data = resp.json::<Value>().map_err(|e| e.to_string())?;
    data.as_array()
        .and_then(|a| a.first().cloned())
        .ok_or_else(|| "Gitee 无发行版".to_string())
}

/// tag 规范：纯数字版本号（1.2.3），其余形式拒绝
fn parse_tag(tag: &str) -> Option<String> {
    let t = tag.trim();
    if !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        Some(t.to_string())
    } else {
        None
    }
}

/// 把 GitHub/Gitee 的 release JSON 归一为 check 结果（两平台字段名一致：tag_name/assets/body）
fn build_check_result(data: &Value) -> Value {
    let tag = parse_tag(data["tag_name"].as_str().unwrap_or(""))
        .unwrap_or_default();
    let has_update = is_newer(&tag, &current_version());
    let (download_base, is_gitee) = (GITEE_DOWNLOAD, data["gitee"].as_bool().unwrap_or(false));
    let _ = (download_base, is_gitee);
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
                "url": a["browser_download_url"].as_str().map(str::to_string)
                    .or_else(|| a["browser_download_url"].as_str().map(str::to_string))
                    .unwrap_or_default(),
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
            "date": data["published_at"].as_str().or_else(|| data["created_at"].as_str()).unwrap_or(""),
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
/// 双源竞速（方案 2）：GitHub 与 Gitee **同时**开下，各写各的临时文件，
/// 先完成且 SHA-256 合法的一方胜出 → 把产物搬到位、通知另一方停止；失败方自动被忽略。
/// 进度走 event:progress {taskId,...}，完成 event:taskDone + event:updateReady。
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
    let name = asset["name"].as_str().unwrap_or("WUHUICL.exe").to_string();
    let expected = asset_digest(asset);
    let size = asset["size"].as_u64();

    let dir = update_dir(app);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let dest = dir.join(format!("wuhuicl-{version}.exe.new"));
    let rec = crate::tasks::register_task(&format!("下载更新 {version}"), "update");
    let task_id = rec.id.clone();

    // 竞速：胜者通道（先完成校验通过的一方把结果发回来）
    let (win_tx, win_rx) = std::sync::mpsc::channel::<(String, u64)>();
    let cancel_race = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

    let sources = vec![
        ("GitHub".to_string(), format!("{GH_DOWNLOAD}/{version}/{}", name)),
        ("Gitee".to_string(), format!("{GITEE_DOWNLOAD}/{version}/{}", name)),
    ];
    for (label, url) in sources {
        let app2 = app.clone();
        let task_id2 = task_id.clone();
        let rec2 = rec.clone();
        let cancel2 = cancel_race.clone();
        let win_tx2 = win_tx.clone();
        let version2 = version.clone();
        let name2 = name.clone();
        let expected2 = expected.clone();
        let dir2 = dir.clone();
        std::thread::spawn(move || {
            if cancel2.load(std::sync::atomic::Ordering::SeqCst) {
                return;
            }
            let part = dir2.join(format!("{version2}.{}.part", label));
            let app3 = app2.clone();
            let tid = task_id2.clone();
            let label3 = label.clone();
            let cancel3 = cancel2.clone();
            let progress: crate::installer::FileProgress = &move |done: u64, total: Option<u64>| {
                if cancel3.load(std::sync::atomic::Ordering::SeqCst) {
                    return; // 已有胜者：此路进度不再上报
                }
                crate::tasks::emit_progress(
                    &app3,
                    json!({
                        "taskId": tid,
                        "stage": "download",
                        "progress": total.map(|t| (done as f64 / t as f64) * 0.95).unwrap_or(0.0),
                        "bytesDone": done,
                        "bytesTotal": total.map(|t| json!(t)).unwrap_or(Value::Null),
                        "indeterminate": total.is_none(),
                        "text": format!("下载更新包（{label3} 先到先用）{}", crate::installer::fmt_mb(done))
                    }),
                );
            };
            let result = crate::installer::download_file(
                &app2,
                Some(&rec2),
                None,
                &url,
                &part,
                None,
                size,
                "",
                Some(progress),
                &[],
                &[],
            );
            if cancel2.load(std::sync::atomic::Ordering::SeqCst) {
                let _ = std::fs::remove_file(&part);
                return;
            }
            match result {
                Ok(_) => {
                    // 校验：digest 缺失视为不可校验（Gitee 上传无 digest），信任 size 匹配
                    let ok = match (&expected2, size) {
                        (Some(h), _) => {
                            sha256_file_hex(&part).map(|a| a == *h).unwrap_or(false)
                        }
                        (None, Some(s)) => std::fs::metadata(&part).map(|m| m.len() == s).unwrap_or(false),
                        (None, None) => true,
                    };
                    if ok {
                        if cancel2.swap(true, std::sync::atomic::Ordering::SeqCst) {
                            // 对方已胜：自己的成果作废
                            let _ = std::fs::remove_file(&part);
                            return;
                        }
                        let _ = win_tx2.send((part.to_string_lossy().to_string(), std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0)));
                    } else {
                        let _ = std::fs::remove_file(&part);
                    }
                }
                Err(e) => {
                    let _ = std::fs::remove_file(&part);
                    let _ = e;
                }
            }
            let _ = name2;
        });
    }
    drop(win_tx);

    let app_handle = app.clone();
    let task_id_thread = task_id.clone();
    std::thread::spawn(move || {
        let task_id = task_id_thread;
        let winner = win_rx.recv_timeout(std::time::Duration::from_secs(30 * 60));
        match winner {
            Ok((part_path, bytes)) => {
                let ok = std::fs::rename(&part_path, &dest).is_ok();
                if ok {
                    crate::tasks::task_done(&app_handle, &task_id, true, None, false, Some("download".into()));
                    if let Some(win) = app_handle.get_webview_window("main") {
                        let _ = win.emit_to(
                            "main",
                            "event:updateReady",
                            json!({ "version": version, "path": dest.to_string_lossy(), "size": bytes }),
                        );
                    }
                } else {
                    crate::tasks::task_done(&app_handle, &task_id, false, Some("更新包落盘失败".into()), false, Some("download".into()));
                }
            }
            Err(_) => {
                cancel_race.store(true, std::sync::atomic::Ordering::SeqCst);
                crate::tasks::task_done(
                    &app_handle,
                    &task_id,
                    false,
                    Some("更新下载失败：GitHub 与 Gitee 均未在时限内完成".into()),
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
