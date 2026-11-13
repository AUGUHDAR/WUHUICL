//! 自更新（对应原版 core/selfUpdate + applyUpdate 的通道合同面）。
//! WUHUICL 独立版本线：update:check 同时查 GitHub Releases 与 Gitee 发行版——两源
//! 并发等满后取 tag 更大者（不能先到先赢：Gitee 镜像常滞后，会钉死版本）。下载按
//! updateSource 设置走 GitHub 直连 / Gitee / 两源同开竞争（先完成者胜，另一方切断），
//! SHA-256 按 Release asset digest 校验；apply 写替换脚本，进程退出后完成替换并重启。
//! 检查/列表/跳过三端通用；应用内替换仅 Windows（macOS/Linux 弹窗引导去发布页）。
//!
//! tag 规范：**纯数字版本号**（如 `1.0.0`），禁止 v 前缀/beta 等后缀——一个版本号对应
//! 一个版本，预发布信息只允许写在正文里。

use serde_json::{json, Value};
use std::path::PathBuf;
use tauri::{Emitter, Manager};

const REPO_API: &str = "https://api.github.com/repos/AUGUHDAR/WUHUICL/releases";
/// Gitee 镜像仓（发布时同步上传同名资产）；tag 与 GitHub 一致（纯数字）
const GITEE_API: &str = "https://gitee.com/api/v5/repos/AUGUHDAR/WUHUICL/releases";
/// GitHub 直连下载基址（浏览器下载地址）
const GH_DOWNLOAD: &str = "https://github.com/AUGUHDAR/WUHUICL/releases/download";
/// Gitee 下载基址
const GITEE_DOWNLOAD: &str = "https://gitee.com/AUGUHDAR/WUHUICL/releases/download";

/// 读取玩家设置的更新来源：'auto' | 'github' | 'gitee'。
/// 旧档位兼容：direct → github，mirror（镜像优先）→ gitee——自定义加速镜像已下线，
/// Gitee 承接国内直连。
#[cfg(windows)]
fn update_source(app: &tauri::AppHandle) -> String {
    match crate::settings::get(app)["updateSource"]
        .as_str()
        .unwrap_or("auto")
    {
        "github" | "direct" => "github".to_string(),
        "gitee" | "mirror" => "gitee".to_string(),
        _ => "auto".to_string(),
    }
}

/// 按 updateSource 设置拼出下载候选（label, url）；github=仅 GitHub，gitee=仅 Gitee，
/// auto = 两源同开竞速。
#[cfg(windows)]
fn download_candidates(app: &tauri::AppHandle, version: &str, name: &str) -> Vec<(String, String)> {
    let source = update_source(app);
    let gh = format!("{GH_DOWNLOAD}/{version}/{}", name);
    let gitee = format!("{GITEE_DOWNLOAD}/{version}/{}", name);
    let mut out: Vec<(String, String)> = Vec::new();
    if source != "gitee" {
        out.push(("GitHub".into(), gh));
    }
    if source != "github" {
        out.push(("Gitee".into(), gitee));
    }
    out
}

fn current_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// update:check → UpdateCheckResult {ok, hasUpdate, release?, reason?, fromCache?}
/// GitHub 与 Gitee 两源并发、等满各自 20s 上限后合并：都成功取 tag 更大者，单成功用单者。
/// 不能先到先赢——Gitee 镜像常滞后一两个版本（voxlink 侧同款教训），先回的滞后源
/// 会把版本钉死，用户永远收不到更新提示。
fn check(_app: &tauri::AppHandle, _force: bool) -> Value {
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
    let mut latest: Option<(Vec<u64>, Value)> = None;
    for rx in [gh, gitee] {
        let Ok(Ok(data)) = rx.recv_timeout(std::time::Duration::from_secs(20)) else {
            continue;
        };
        let Some(tag) = parse_tag(data["tag_name"].as_str().unwrap_or("")) else {
            continue;
        };
        let key = tag_key(&tag);
        if latest.as_ref().map_or(true, |(k, _)| key > *k) {
            latest = Some((key, data));
        }
    }
    match latest {
        Some((_, data)) => build_check_result(&data),
        None => json!({ "ok": false, "hasUpdate": false, "reason": "网络不可达（GitHub 与 Gitee 均失败）" }),
    }
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

/// Gitee releases 列表（Gitee API 没有 /latest；列表排序无合同保证，按 created_at 取最新）
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
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .max_by(|a, b| {
            a["created_at"]
                .as_str()
                .unwrap_or("")
                .cmp(b["created_at"].as_str().unwrap_or(""))
        })
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

/// tag → 数值段向量（逐段比较用；入参已经过 parse_tag，只会是纯数字）
fn tag_key(tag: &str) -> Vec<u64> {
    tag.split('.').map(|p| p.parse::<u64>().unwrap_or(0)).collect()
}

/// 当前平台的更新资产过滤（check/list 结果只带本平台拿得动的包）
fn asset_matches_platform(name: &str) -> bool {
    let n = name.to_lowercase();
    #[cfg(windows)]
    {
        n.ends_with(".exe")
    }
    #[cfg(target_os = "macos")]
    {
        n.ends_with(".dmg") || (n.ends_with(".zip") && n.contains("macos"))
    }
    #[cfg(target_os = "linux")]
    {
        n.ends_with(".appimage") || n.ends_with(".deb")
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        let _ = n;
        false
    }
}

/// 平台过滤后挑主资产（macOS 优先 dmg，Linux 优先 AppImage；assets 已按平台筛过则取首个）
fn pick_main_asset(assets: &[Value]) -> Option<Value> {
    let list: Vec<&Value> = assets
        .iter()
        .filter(|a| {
            a["name"]
                .as_str()
                .map(asset_matches_platform)
                .unwrap_or(false)
        })
        .collect();
    if list.is_empty() {
        return None;
    }
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    let prefer = |suffix: &str| -> Option<Value> {
        list.iter()
            .find(|a| {
                a["name"]
                    .as_str()
                    .map(|n| n.to_lowercase().ends_with(suffix))
                    .unwrap_or(false)
            })
            .map(|a| (*a).clone())
    };
    #[cfg(target_os = "macos")]
    return prefer(".dmg")
        .or_else(|| prefer(".zip"))
        .or_else(|| Some(list[0].clone()));
    #[cfg(target_os = "linux")]
    return prefer(".appimage").or_else(|| Some(list[0].clone()));
    #[cfg(windows)]
    return Some(list[0].clone());
}

/// 资产 JSON → 精简条目（name/url/size/digest；Gitee 无 digest，由 size 校验兜底）
fn slim_assets(data: &Value) -> Vec<Value> {
    data["assets"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|a| {
            let name = a["name"].as_str()?.to_string();
            Some(json!({
                "name": name,
                "url": a["browser_download_url"].as_str().map(str::to_string)
                    .unwrap_or_default(),
                "size": a["size"].as_u64().unwrap_or(0),
                "digest": a["digest"].as_str().unwrap_or("")
            }))
        })
        .collect()
}

/// 把 GitHub/Gitee 的 release JSON 归一为 UpdateCheckResult；release 段按前端 ReleaseInfo
/// 合同出（publishedAt/assetUrl/assetSize/assetName），另带平台过滤后的 assets 供
/// update:start 取 digest 与双源下载 URL。
fn build_check_result(data: &Value) -> Value {
    let tag = parse_tag(data["tag_name"].as_str().unwrap_or("")).unwrap_or_default();
    let has_update = is_newer(&tag, &current_version());
    let assets = slim_assets(data);
    let main = pick_main_asset(&assets);
    json!({
        "ok": true,
        "hasUpdate": has_update,
        "release": if has_update { json!({
            "version": tag,
            "publishedAt": data["published_at"].as_str()
                .or_else(|| data["created_at"].as_str()).unwrap_or(""),
            "body": data["body"].as_str().unwrap_or(""),
            "assetUrl": main.as_ref().and_then(|a| a["url"].as_str()).unwrap_or(""),
            "assetSize": main.as_ref().and_then(|a| a["size"].as_u64()).unwrap_or(0),
            "assetName": main.as_ref().and_then(|a| a["name"].as_str()).unwrap_or(""),
            "assets": assets,
            "rollback": false
        }) } else { Value::Null }
    })
}

/// 语义化版本比较：tag > current
fn is_newer(tag: &str, current: &str) -> bool {
    let (a, b) = (tag_key(tag), tag_key(current));
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

/// update:listReleases → ReleaseInfo[]（历史版本回退候选；同样按前端合同出，
/// 带平台过滤后的 assets——Windows 回退流程会把整条 release 原样传给 update:start）
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
    let Ok(data) = resp.json::<Value>() else {
        return json!([]);
    };
    Value::Array(
        data.as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|r| {
                let tag = parse_tag(r["tag_name"].as_str().unwrap_or(""))?;
                let assets = slim_assets(&r);
                let main = pick_main_asset(&assets);
                Some(json!({
                    "version": tag,
                    "publishedAt": r["published_at"].as_str().unwrap_or(""),
                    "body": r["body"].as_str().unwrap_or(""),
                    "assetUrl": main.as_ref().and_then(|a| a["url"].as_str()).unwrap_or(""),
                    "assetSize": main.as_ref().and_then(|a| a["size"].as_u64()).unwrap_or(0),
                    "assetName": main.as_ref().and_then(|a| a["name"].as_str()).unwrap_or(""),
                    "assets": assets,
                    "rollback": false
                }))
            })
            .collect(),
    )
}

/// 后台静默更新检测：发现新版本时只在顶栏点亮一个提示，不弹窗、不打断。
/// 「无感」是主导目标：没更新 → 什么都不发生；有更新 → 一个可忽略的红点。
/// 三端都查；macOS/Linux 点开后在弹窗里引导去发布页（应用内替换仅 Windows）。
/// 用户点过「跳过此版本」的版本不再提示（设置页手动检查不受限）。
pub fn startup_check(app: &tauri::AppHandle) {
    let handle = app.clone();
    std::thread::spawn(move || {
        // 先缓 6 秒再查：等主窗口显示、前端挂上 event:updatePrompt 监听。
        // 检查太快完成时事件发进还没人听的窗口，发后即焚——提示就永远不出现了。
        std::thread::sleep(std::time::Duration::from_secs(6));
        let result = check(&handle, false);
        if result["ok"] == true && result["hasUpdate"] == true {
            let version = result["release"]["version"].as_str().unwrap_or("");
            let skipped = crate::settings::get(&handle)
                .get("skipUpdateVersion")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if !skipped.is_empty() && version == skipped {
                return;
            }
            // 事件名沿用 updatePrompt（前端只有一个监听点），但语义已改为「后台发现」，
            // 由 App.vue 写进 store.availableUpdate 点亮顶栏，而不是直接开弹窗。
            if let Some(win) = handle.get_webview_window("main") {
                let _ = win.emit_to("main", "event:updatePrompt", result["release"].clone());
            }
        }
    });
}

// ---------------- 下载与应用（update:start / update:apply） ----------------
// 以下流程 = Windows 便携 exe 的就地替换（cmd/bat + tasklist 轮询），全部仅 Windows

#[cfg(windows)]
fn update_dir(app: &tauri::AppHandle) -> std::path::PathBuf {
    crate::settings::user_data_dir(app).join("update")
}

/// asset digest（"sha256:..."）→ hex
#[cfg(windows)]
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
#[cfg(windows)]
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
    // 慢速提示（原版 applyUpdate.ts：bps>0 且 <100KB/s 持续 30s 推一次 event:updateSlowHint）：
    // 多源竞速共享「已提示」位，任一路先触发即全局只提示一次
    let slow_hint_sent = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

    let sources = download_candidates(app, &version, &name);
    for (label, url) in sources {
        let app2 = app.clone();
        let task_id2 = task_id.clone();
        let rec2 = rec.clone();
        let cancel2 = cancel_race.clone();
        let win_tx2 = win_tx.clone();
        let slow_hint2 = slow_hint_sent.clone();
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
            // (上次采样时刻, 上次字节数, 连续慢速起点)
            let slow_state3 = std::sync::Arc::new(std::sync::Mutex::new((
                std::time::Instant::now(),
                0u64,
                None::<std::time::Instant>,
            )));
            let slow_hint3 = slow_hint2.clone();
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
                // 慢速窗口判定：每秒采样一次速率，bps=0（完全停滞）不提示，交给竞速超时
                let now = std::time::Instant::now();
                let mut g = slow_state3.lock().unwrap();
                let dt = now.duration_since(g.0).as_secs_f64();
                if dt >= 1.0 {
                    let bps = ((done.saturating_sub(g.1)) as f64 / dt) as u64;
                    let slow_since = if bps > 0 && bps < 100 * 1024 {
                        *g = (now, done, g.2.or(Some(now)));
                        g.2
                    } else {
                        *g = (now, done, None);
                        None
                    };
                    drop(g);
                    if let Some(start) = slow_since {
                        if now.duration_since(start) >= std::time::Duration::from_secs(30)
                            && !slow_hint3.swap(true, std::sync::atomic::Ordering::SeqCst)
                        {
                            if let Some(win) = app3.get_webview_window("main") {
                                let _ = win.emit_to(
                                    "main",
                                    "event:updateSlowHint",
                                    json!({ "taskId": tid }),
                                );
                            }
                        }
                    }
                }
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
                        (Some(h), _) => sha256_file_hex(&part).map(|a| a == *h).unwrap_or(false),
                        (None, Some(s)) => std::fs::metadata(&part)
                            .map(|m| m.len() == s)
                            .unwrap_or(false),
                        (None, None) => true,
                    };
                    if ok {
                        if cancel2.swap(true, std::sync::atomic::Ordering::SeqCst) {
                            // 对方已胜：自己的成果作废
                            let _ = std::fs::remove_file(&part);
                            return;
                        }
                        let _ = win_tx2.send((
                            part.to_string_lossy().to_string(),
                            std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0),
                        ));
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
                    crate::tasks::task_done(
                        &app_handle,
                        &task_id,
                        true,
                        None,
                        false,
                        Some("download".into()),
                    );
                    if let Some(win) = app_handle.get_webview_window("main") {
                        let _ = win.emit_to(
                            "main",
                            "event:updateReady",
                            json!({ "version": version, "path": dest.to_string_lossy(), "size": bytes }),
                        );
                    }
                } else {
                    crate::tasks::task_done(
                        &app_handle,
                        &task_id,
                        false,
                        Some("更新包落盘失败".into()),
                        false,
                        Some("download".into()),
                    );
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

#[cfg(windows)]
fn sha256_file_hex(path: &std::path::Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    // 流式读取：本地安装包可能非常大，一次性 read 全文件会直接把 32 位堆打爆
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut reader = std::io::BufReader::with_capacity(1024 * 1024, file);
    let mut hasher = Sha256::new();
    std::io::copy(&mut reader, &mut hasher).map_err(|e| e.to_string())?;
    Ok(hex::encode(Sha256::digest(&hasher.finalize())))
}

/// update:apply(release)：写替换脚本（等当前进程退出 → 换 exe → 重启）然后退出。
/// 原版 .kamuclupdate 的"下次启动替换"等价物；脚本内保留 .old 备份。
#[cfg(windows)]
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

/// update:pickLocalFile：选择本地 exe 安装包并校验（版本号解析 + 与 Release 资产的 SHA-256 对照）
#[cfg(windows)]
fn pick_local_file(app: &tauri::AppHandle) -> Result<Value, String> {
    use tauri_plugin_dialog::DialogExt;
    let picked = app
        .dialog()
        .file()
        .add_filter("WUHUICL 安装包", &["exe"])
        .blocking_pick_file();
    let Some(path) = picked else {
        return Ok(Value::Null); // 用户取消：前端静默
    };
    let file_path = path.to_string();
    let meta = std::fs::metadata(&file_path).map_err(|_| "安装包文件不存在".to_string())?;
    if !meta.is_file() {
        return Err("所选路径不是文件".into());
    }
    // 安装包正常 ≤ 200MB；超限直接拒绝，避免校验阶段吃光内存
    if meta.len() > 200 * 1024 * 1024 {
        return Err(format!(
            "文件大小 {:.0} MB 超出安装包正常范围（≤200MB）",
            meta.len() as f64 / 1048576.0
        ));
    }
    let name = PathBuf::from(&file_path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    // 文件名解析版本：wuhuicl-1.2.3.exe / WUHUICL-1.2.3.exe / 1.2.3.exe
    let lower = name.to_lowercase();
    let version = lower
        .trim_end_matches(".exe")
        .split(['-', '_'])
        .find_map(|part| {
            let t = part.trim_start_matches('v');
            let ok = !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit() || b == b'.');
            ok.then(|| t.to_string())
        })
        .unwrap_or_default();
    let version_ok = !version.is_empty() && is_newer(&version, &current_version());
    // SHA-256 对照：能联网拿到当前版本线 Release 资产 digest 才能比对
    let mut sha256 = "unknown";
    let mut detail = String::new();
    if let Ok(data) = crate::net::client()
        .get(format!("{REPO_API}/latest"))
        .header("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(15))
        .send()
    {
        if data.status().is_success() {
            if let Ok(rel) = data.json::<Value>() {
                let digest = rel["assets"]
                    .as_array()
                    .and_then(|a| {
                        a.iter()
                            .find(|a| {
                                a["name"]
                                    .as_str()
                                    .map(|n| n.to_lowercase().ends_with(".exe"))
                                    .unwrap_or(false)
                            })
                            .and_then(|a| a["digest"].as_str())
                    })
                    .and_then(|d| d.strip_prefix("sha256:"))
                    .map(str::to_lowercase);
                if let Some(expected) = digest {
                    match sha256_file_hex(std::path::Path::new(&file_path)) {
                        Ok(actual) => {
                            sha256 = if actual == expected {
                                "match"
                            } else {
                                "mismatch"
                            };
                            if sha256 == "mismatch" {
                                detail =
                                    "与官方发布的 SHA-256 不一致：文件可能损坏或被改动，请重新下载"
                                        .into();
                            }
                        }
                        Err(e) => detail = e,
                    }
                } else {
                    detail = "官方发布缺少 SHA-256 校验值，无法比对".into();
                }
            }
        } else {
            detail = format!("无法连接 GitHub 校验（HTTP {}）", data.status().as_u16());
        }
    } else {
        detail = "无法连接 GitHub 校验，请检查网络".into();
    }
    Ok(json!({
        "filePath": file_path,
        "fileName": name,
        "fileSize": meta.len(),
        "version": version,
        "versionOk": version_ok,
        "sha256": sha256,
        "detail": detail
    }))
}

/// update:applyLocal：本地安装包走与 update:apply 相同的备份-替换-重启脚本
#[cfg(windows)]
fn apply_local_file(app: &tauri::AppHandle, check: &Value) -> Result<(), String> {
    let path = check["filePath"].as_str().unwrap_or("");
    if path.is_empty() {
        return Err("缺少安装包路径，请重新选择".into());
    }
    let staged = PathBuf::from(path);
    if !staged.is_file() {
        return Err("安装包文件不存在，请重新选择".into());
    }
    let current = std::env::current_exe().map_err(|e| e.to_string())?;
    let target = current.canonicalize().unwrap_or(current);
    let dir = update_dir(app);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let bat = dir.join("apply-local-update.bat");
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
pub fn invoke(
    app: &tauri::AppHandle,
    channel: &str,
    args: &[Value],
) -> Option<Result<Value, String>> {
    // 平台无关：检查/列表/跳过/配置状态在任何平台都可用（macOS/Linux 提示更新后引导发布页）
    match channel {
        "update:check" => {
            return Some(Ok(check(
                app,
                args.first().and_then(|v| v.as_bool()).unwrap_or(false),
            )));
        }
        "update:getState" | "update:getPending" => return Some(Ok(Value::Null)),
        "update:getConfigStatus" => return Some(Ok(config_status(app))),
        "update:listReleases" => return Some(Ok(list_releases())),
        "update:skip" => {
            let version = args.first().and_then(|v| v.as_str()).unwrap_or("");
            return Some(
                crate::settings::save(app, json!({ "skipUpdateVersion": version }))
                    .map(|_| Value::Null),
            );
        }
        "update:resetSettings" => {
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
            let payload = serde_json::to_vec_pretty(&def).map_err(|e| e.to_string());
            return Some(
                payload
                    .and_then(|bytes| std::fs::write(&path, bytes).map_err(|e| e.to_string()))
                    .map(|_| Value::Null),
            );
        }
        _ => {}
    }
    // 应用内自更新 = Windows 便携 exe 的就地替换流程（cmd/bat + tasklist）。
    // macOS/Linux 安装形态不同（dmg/zip、AppImage/deb），暂不支持，前端弹窗引导去发布页。
    #[cfg(windows)]
    match channel {
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
        "update:applyPending" => Some(Ok(Value::Bool(false))),
        "update:pickLocalFile" => Some(pick_local_file(app)),
        "update:applyLocal" => {
            let check = args.first().cloned().unwrap_or(Value::Null);
            Some(apply_local_file(app, &check).map(|_| Value::Null))
        }
        "update:restoreBackup" => Some(Err("备份恢复未启用：更新脚本自带 .old 回滚".into())),
        _ => None,
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        match channel {
            "update:start" | "update:apply" | "update:applyPending" | "update:pickLocalFile"
            | "update:applyLocal" | "update:restoreBackup" => Some(Err(
                "macOS/Linux 暂不支持应用内更新：请到 GitHub/Gitee 发布页下载新版本".into(),
            )),
            _ => None,
        }
    }
}
