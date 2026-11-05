//! VoxLink CLI 取件层：按平台选资产、多源竞速解析最新 release、后台静默更新、空闲时无感替换。
//!
//! 设计口径（与上游作者确认）：
//! - CLI 不自更新，启动器负责追上游最新 release；上游发版频繁且多为连通率/域名级重要变更，
//!   所以检查要勤、全程后台，绝不允许为检查或下载阻塞界面。
//! - 版本号一律以 release tag 为准；CLI `--version` 自报的是构建标签（如 `1.0.0-beta`），
//!   只做诊断展示，不进版本判断，也不覆盖 release tag。
//! - SHA256SUMS 尽力校验：拿得到就校验（防传输损坏），拿不到不拦更新（HTTPS 本身有完整性），
//!   校验链不可用绝不能成为卡在旧版的理由。
//! - 替换已安装的二进制前确认联机会话空闲（Windows 下文件被运行中进程锁住），忙则记为待替换，
//!   下一次空闲时机（后台循环 / 进页 / 启动）自动完成替换。

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

pub const OWNER_REPO: &str = "AUGUHDAR/VoxLink-app";
const SUMS: &str = "SHA256SUMS.txt";
const LATEST_API_GH: &str = "https://api.github.com/repos/AUGUHDAR/VoxLink-app/releases/latest";
const LATEST_PAGE_GH: &str = "https://github.com/AUGUHDAR/VoxLink-app/releases/latest";
const LATEST_API_GITEE: &str =
    "https://gitee.com/api/v5/repos/AUGUHDAR/VoxLink-app/releases/latest";
const GH_ASSET: &str = "https://github.com/AUGUHDAR/VoxLink-app/releases/download/";
const GITEE_ASSET: &str = "https://gitee.com/AUGUHDAR/VoxLink-app/releases/download/";
const MAX_BYTES: u64 = 32 * 1024 * 1024;
/// 后台常态检查节奏；进页兜底检查用更短的间隔（失败重试更勤）。
const CHECK_PERIOD_MS: u64 = 60 * 60_000;
const CHECK_MIN_INTERVAL_MS: u64 = 30 * 60_000;
const CHECK_RETRY_INTERVAL_MS: u64 = 5 * 60_000;
const TAG_TIMEOUT_MS: u64 = 6_000;
const SUMS_TIMEOUT_MS: u64 = 8_000;
const PROBE_TIMEOUT: Duration = Duration::from_secs(8);

/// 上游发布的 CLI 资产按目标平台选择；未发布的平台明确报错，不偷偷回落别的架构。
fn asset_for(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("windows", "x86_64") => Some("VoxLink-CLI-windows-amd64.exe"),
        ("macos", "x86_64") => Some("VoxLink-CLI-darwin-amd64"),
        ("macos", "aarch64") => Some("VoxLink-CLI-darwin-arm64"),
        ("linux", "x86_64") => Some("VoxLink-CLI-linux-amd64"),
        ("linux", "aarch64") => Some("VoxLink-CLI-linux-arm64"),
        _ => None,
    }
}

pub fn platform_asset() -> Result<&'static str, String> {
    asset_for(std::env::consts::OS, std::env::consts::ARCH).ok_or_else(|| {
        format!(
            "VoxLink 未发布 {}/{} 平台的联机组件",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    })
}

/// 本地落盘文件名：Windows 沿用资产名（与旧缓存无缝衔接），Unix 用统一短名并补执行位。
fn local_name() -> &'static str {
    if cfg!(windows) {
        "VoxLink-CLI-windows-amd64.exe"
    } else {
        "VoxLink-CLI"
    }
}

pub fn dir_for(app: &tauri::AppHandle) -> PathBuf {
    crate::settings::user_data_dir(app).join("voxlink")
}

pub fn binary_path(app: &tauri::AppHandle) -> PathBuf {
    dir_for(app).join(local_name())
}

fn part_path(app: &tauri::AppHandle) -> PathBuf {
    binary_path(app).with_extension("part")
}

fn meta_path(app: &tauri::AppHandle) -> PathBuf {
    dir_for(app).join("cli-meta.json")
}

pub fn read_meta(app: &tauri::AppHandle) -> Value {
    fs::read_to_string(meta_path(app))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({}))
}

fn write_meta(app: &tauri::AppHandle, meta: &Value) {
    if let Some(p) = meta_path(app).parent() {
        let _ = fs::create_dir_all(p);
    }
    if let Ok(s) = serde_json::to_string_pretty(meta) {
        let _ = fs::write(meta_path(app), s);
    }
}

fn meta_version(meta: &Value) -> String {
    meta["version"].as_str().unwrap_or("").to_string()
}

// ---------------- 更新状态（界面经 vlcli:updateState / vlcli:event 读取） ----------------

static UPDATE_STATE: Mutex<Option<Value>> = Mutex::new(None);
/// 后台工作重入闸：同一时刻至多一条检查/下载流水线。
static WORKING: AtomicBool = AtomicBool::new(false);

fn current_state() -> Value {
    UPDATE_STATE
        .lock()
        .unwrap()
        .clone()
        .unwrap_or_else(|| json!({ "phase": "idle" }))
}

fn set_state(app: &AppHandle, state: Value) {
    *UPDATE_STATE.lock().unwrap() = Some(state.clone());
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.emit_to(
            "main",
            "vlcli:event",
            json!({ "kind": "update", "data": state }),
        );
    }
}

fn patch_state(app: &AppHandle, patch: Value) {
    let mut merged = match UPDATE_STATE.lock().unwrap().clone() {
        Some(v) => v,
        None => json!({ "phase": "idle" }),
    };
    if let (Some(obj), Some(p)) = (merged.as_object_mut(), patch.as_object()) {
        for (k, v) in p {
            obj.insert(k.clone(), v.clone());
        }
    }
    set_state(app, merged);
}

pub fn update_state(app: &tauri::AppHandle) -> Value {
    let mut state = current_state();
    let meta = read_meta(app);
    if let Some(obj) = state.as_object_mut() {
        obj.insert("version".into(), json!(meta_version(&meta)));
        obj.insert(
            "lastCheckAt".into(),
            meta["checkedAt"].as_u64().unwrap_or(0).into(),
        );
        obj.insert("lastError".into(), meta["lastError"].clone());
        obj.insert("pending".into(), meta["pending"].clone());
    }
    state
}

fn emit_download_progress(app: &AppHandle, bytes: u64, total: Option<u64>) {
    let pct = total.filter(|t| *t > 0).map(|t| (bytes * 100 / t).min(99));
    patch_state(
        app,
        json!({ "phase": "downloading", "bytes": bytes, "total": total,
                "progress": pct, "text": format!("正在后台更新联机组件（{} KB）", bytes / 1024) }),
    );
}

// ---------------- 最新版解析：三源并行，取版本最大者 ----------------
//
// 版本解析不能像下载那样"先到先赢"：镜像可能滞后（实测 Gitee 曾停在 1.0.0 而 GitHub 已 1.0.1），
// 谁先回谁赢会让滞后镜像把版本钉死。下载源随意（内容一致），版本号必须取全部成功源的最大值。

fn latest_tag() -> Result<String, String> {
    let (tx, rx) = mpsc::channel::<Result<String, String>>();
    let sources: [fn() -> Result<String, String>; 3] =
        [tag_from_gh_api, tag_from_gitee_api, tag_from_gh_redirect];
    for src in sources {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(src());
        });
    }
    drop(tx);
    let mut best: Option<String> = None;
    let mut last = "更新源暂时都连不上".to_string();
    let mut got = 0usize;
    while got < sources.len() {
        match rx.recv_timeout(Duration::from_millis(TAG_TIMEOUT_MS + 1_000)) {
            Ok(Ok(tag)) => {
                got += 1;
                if best.as_ref().is_none_or(|b| version_newer(&tag, b)) {
                    best = Some(tag);
                }
            }
            Ok(Err(e)) => {
                got += 1;
                last = e;
            }
            Err(_) => break,
        }
    }
    best.ok_or(last)
}

/// 语义版本比较：剥掉可选的 v 前缀，逐段数值比（缺段按 0）；不含数字的 tag 无法定序，不判更新。
fn version_newer(candidate: &str, current: &str) -> bool {
    let key = |tag: &str| -> Option<Vec<u64>> {
        let t = tag.trim().trim_start_matches(['v', 'V']);
        let parts: Vec<u64> = t
            .split(|c: char| !c.is_ascii_digit())
            .map(|p| p.parse::<u64>().ok())
            .collect::<Option<Vec<_>>>()?;
        // "beta" 这类整段非数字 split 出来的空段解析全失败，Option 聚合已挡掉；再挡空 tag
        if parts.is_empty() || t.is_empty() {
            None
        } else {
            Some(parts)
        }
    };
    let (Some(a), Some(b)) = (key(candidate), key(current)) else {
        return false;
    };
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    false
}

fn tag_from_gh_api() -> Result<String, String> {
    let page = crate::net::get_text(LATEST_API_GH, TAG_TIMEOUT_MS)?;
    tag_of_json(&page).ok_or_else(|| "GitHub 版本接口返回异常".to_string())
}

fn tag_from_gitee_api() -> Result<String, String> {
    let page = crate::net::get_text(LATEST_API_GITEE, TAG_TIMEOUT_MS)?;
    tag_of_json(&page).ok_or_else(|| "Gitee 版本接口返回异常".to_string())
}

/// 不走 API：releases/latest 页面会 302 到 tag 地址，从 Location 里摘 tag。
/// 零配额限制，API 被限流时这条通常还活着。
fn tag_from_gh_redirect() -> Result<String, String> {
    let resp = crate::net::download_client()
        .get(LATEST_PAGE_GH)
        .timeout(Duration::from_millis(TAG_TIMEOUT_MS))
        .send()
        .map_err(|e| format!("GitHub 版本页连接失败：{e}"))?;
    let location = resp
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| "GitHub 版本页没有返回跳转地址".to_string())?
        .to_string();
    let tag = location
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("");
    if tag.is_empty() {
        return Err("GitHub 版本页跳转地址无法解读".into());
    }
    Ok(tag.to_string())
}

fn tag_of_json(page: &str) -> Option<String> {
    let v: Value = serde_json::from_str(page).ok()?;
    let tag = v["tag_name"].as_str()?.trim().to_string();
    if tag.is_empty() {
        None
    } else {
        Some(tag)
    }
}

/// SUMS 尽力获取：双源竞速，拿不到返回 None（不拦更新）。
fn fetch_sums(tag: &str, asset: &str) -> Option<String> {
    let (tx, rx) = mpsc::channel::<Option<String>>();
    for base in [GH_ASSET, GITEE_ASSET] {
        let tx = tx.clone();
        let url = format!("{base}{tag}/{SUMS}");
        std::thread::spawn(move || {
            let text = crate::net::get_text(&url, SUMS_TIMEOUT_MS).ok();
            let _ = tx.send(text);
        });
    }
    drop(tx);
    for _ in 0..2 {
        match rx.recv_timeout(Duration::from_millis(SUMS_TIMEOUT_MS + 1_000)) {
            Ok(Some(text)) => {
                if let Some(sha) = expected_sha(&text, asset) {
                    return Some(sha);
                }
            }
            Ok(None) => {}
            Err(_) => break,
        }
    }
    None
}

pub fn expected_sha(sums: &str, asset: &str) -> Option<String> {
    for line in sums.lines() {
        let mut it = line.split_whitespace();
        let (Some(hash), Some(name)) = (it.next(), it.next()) else {
            continue;
        };
        let clean = name.trim_start_matches('*').replace('\\', "/");
        if clean.rsplit('/').next() == Some(asset) && hash.len() == 64 {
            return Some(hash.to_ascii_lowercase());
        }
    }
    None
}

pub fn sha256_of(path: &Path) -> Result<String, String> {
    let file = fs::File::open(path).map_err(|e| format!("读取失败：{}", e))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    let mut reader = std::io::BufReader::new(file);
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("读取失败：{}", e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

// ---------------- 就绪判定（毫秒级，纯本地；网络一律后台） ----------------

/// 就绪判定只看本地：有二进制且知道版本 → 立即就绪；没有 → 后台获取。
/// force=true 时额外立刻发起一次后台检查（设置页「检查更新」）。
pub fn ensure(app: &tauri::AppHandle, force: bool) -> Value {
    if let Err(e) = platform_asset() {
        return json!({ "ok": false, "error": e });
    }
    let target = binary_path(app);
    let meta = read_meta(app);
    let version = meta_version(&meta);
    if target.is_file() {
        if version.is_empty() {
            // 有二进制但版本未知（meta 丢失等）：后台对 SHA 认领或刷新，界面先放行
            kick(app, false);
            return json!({ "ok": true, "ready": true, "version": "", "identifying": true,
                "path": target.to_string_lossy() });
        }
        kick(app, force);
        return json!({ "ok": true, "ready": true, "version": version,
            "path": target.to_string_lossy() });
    }
    kick(app, force);
    json!({ "ok": true, "ready": false, "downloading": true, "version": version })
}

/// 发起一次后台流水线；已有流水线在跑则跳过（进行中的一轮本身就会完成检查与下载）。
fn kick(app: &tauri::AppHandle, force: bool) {
    let app = app.clone();
    if !WORKING.swap(true, Ordering::SeqCst) {
        std::thread::spawn(move || {
            run_pass(&app, force);
            WORKING.store(false, Ordering::SeqCst);
        });
    }
}

/// 启动后台循环：启动稍歇一口气后开跑，之后常态周期检查。
pub fn start_background(app: AppHandle) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(15));
        loop {
            if !WORKING.swap(true, Ordering::SeqCst) {
                run_pass(&app, false);
                WORKING.store(false, Ordering::SeqCst);
            }
            std::thread::sleep(Duration::from_millis(CHECK_PERIOD_MS));
        }
    });
}

/// 一条完整的后台流水线：结算待替换 → 需要时解析最新版 → 需要时下载替换。
fn run_pass(app: &AppHandle, force_check: bool) {
    settle_pending(app);
    let meta = read_meta(app);
    let target = binary_path(app);
    let cached = meta_version(&meta);
    let now = crate::download::now_ms();
    let checked_at = meta["checkedAt"].as_u64().unwrap_or(0);
    let failed_before = !meta["lastError"].as_str().unwrap_or("").is_empty();
    let interval = if failed_before {
        CHECK_RETRY_INTERVAL_MS
    } else {
        CHECK_MIN_INTERVAL_MS
    };
    let need_check = force_check
        || !target.is_file()
        || cached.is_empty()
        || now >= checked_at.saturating_add(interval);
    if !need_check {
        return;
    }
    set_state(app, json!({ "phase": "checking" }));
    let tag = match latest_tag() {
        Ok(t) => t,
        Err(e) => {
            let mut m = meta.clone();
            m["lastError"] = json!(e);
            write_meta(app, &m);
            patch_state(app, json!({ "phase": "failed", "message": e }));
            return;
        }
    };
    let mut m = meta.clone();
    m["checkedAt"] = json!(now);
    m["lastError"] = Value::Null;
    patch_state(app, json!({ "phase": "latest", "latest": tag }));

    let sha = match platform_asset() {
        Ok(a) => fetch_sums(&tag, a),
        Err(_) => None,
    };
    // 二进制已存在且就是最新版：只刷新检查时间
    if target.is_file() && !cached.is_empty() && cached == tag {
        write_meta(app, &m);
        return;
    }
    // 二进制存在但版本未知/落后：先试着用 SHA 认领（可能本地已是新版，只是 meta 丢了）
    if target.is_file() {
        if let Some(expected) = sha.as_deref() {
            if sha256_of(&target).map(|a| a == expected).unwrap_or(false) {
                m["version"] = json!(tag);
                m["sha256"] = json!(expected);
                write_meta(app, &m);
                patch_state(
                    app,
                    json!({ "phase": "latest", "latest": tag, "version": tag }),
                );
                return;
            }
        }
    }
    match download_and_install(app, &tag, sha.as_deref()) {
        Ok(InstallOutcome::Installed(bytes)) => {
            m["version"] = json!(tag);
            if let Some(expected) = sha.as_deref() {
                m["sha256"] = json!(expected);
            }
            m["updatedAt"] = json!(crate::download::now_ms());
            m["bytes"] = json!(bytes);
            write_meta(app, &m);
            patch_state(
                app,
                json!({ "phase": "latest", "latest": tag, "version": tag }),
            );
        }
        Ok(InstallOutcome::Deferred(bytes)) => {
            // 会话进行中，.part 留存待替换（meta 里已记 pending，由空闲时机结算）
            m["bytes"] = json!(bytes);
            m["updatedAt"] = json!(crate::download::now_ms());
            write_meta(app, &m);
        }
        Err(e) => {
            m["lastError"] = json!(e.clone());
            write_meta(app, &m);
            patch_state(app, json!({ "phase": "failed", "message": e }));
        }
    }
}

/// 把上次没换成的 .part 换成正主（联机会话空闲时才能动文件）。
fn settle_pending(app: &AppHandle) -> bool {
    let meta = read_meta(app);
    let Some(pending) = meta["pending"].as_str().map(str::to_string) else {
        return false;
    };
    if crate::vlcli::session::alive() {
        return false;
    }
    let part = part_path(app);
    if !part.is_file() {
        let mut m = meta.clone();
        m["pending"] = Value::Null;
        write_meta(app, &m);
        return false;
    }
    let expected = meta["pendingSha"].as_str();
    if let Some(expected) = expected {
        match sha256_of(&part) {
            Ok(actual) if actual == expected => {}
            _ => {
                let _ = fs::remove_file(&part);
                let mut m = meta.clone();
                m["pending"] = Value::Null;
                write_meta(app, &m);
                return false;
            }
        }
    }
    match install_binary(app, &part) {
        Ok(()) => {
            let mut m = meta.clone();
            m["version"] = json!(pending);
            if let Some(sha) = expected {
                m["sha256"] = json!(sha);
            }
            m["pending"] = Value::Null;
            m["pendingSha"] = Value::Null;
            m["updatedAt"] = json!(crate::download::now_ms());
            write_meta(app, &m);
            patch_state(app, json!({ "phase": "latest", "version": pending }));
            true
        }
        Err(_) => false,
    }
}

/// 下载结果：已落位 / 会话忙先留存待替换。
enum InstallOutcome {
    Installed(u64),
    Deferred(u64),
}

/// 下载到 .part 并尝试落位；会话忙时留在原地记为待替换。
fn download_and_install(
    app: &AppHandle,
    tag: &str,
    sha: Option<&str>,
) -> Result<InstallOutcome, String> {
    let asset = platform_asset()?;
    set_state(
        app,
        json!({ "phase": "downloading", "latest": tag, "bytes": 0, "total": Value::Null }),
    );
    let urls = download_urls(tag, asset);
    let bytes = fetch(&urls, &part_path(app), sha, app)?;
    if crate::vlcli::session::alive() {
        // 正在房间里：先记待替换，等空闲时机（后台循环/下次进页/重启）自动完成
        let mut m = read_meta(app);
        m["pending"] = json!(tag);
        m["pendingSha"] = json!(sha);
        write_meta(app, &m);
        patch_state(
            app,
            json!({ "phase": "pending", "latest": tag,
            "message": "新版本已下载，联机空闲时自动替换" }),
        );
        return Ok(InstallOutcome::Deferred(bytes));
    }
    install_binary(app, &part_path(app))?;
    Ok(InstallOutcome::Installed(bytes))
}

fn install_binary(app: &AppHandle, part: &Path) -> Result<(), String> {
    let target = binary_path(app);
    if let Some(p) = target.parent() {
        fs::create_dir_all(p).map_err(|e| format!("无法创建组件目录：{}", e))?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(part, fs::Permissions::from_mode(0o755));
    }
    fs::rename(part, &target).map_err(|e| format!("组件落位失败：{}", e))?;
    Ok(())
}

/// 双源下载顺序：健康的源排前面（GH 优先，冷却中则 Gitee 先行）。
fn download_urls(tag: &str, asset: &str) -> Vec<String> {
    let mut urls = vec![
        format!("{GH_ASSET}{tag}/{asset}"),
        format!("{GITEE_ASSET}{tag}/{asset}"),
    ];
    if crate::download::host_cooling(&urls[0]) && !crate::download::host_cooling(&urls[1]) {
        urls.reverse();
    }
    urls
}

fn fetch(
    urls: &[String],
    target: &Path,
    sha: Option<&str>,
    app: &AppHandle,
) -> Result<u64, String> {
    let parent = target.parent().ok_or("组件路径异常")?;
    fs::create_dir_all(parent).map_err(|e| format!("无法创建组件目录：{}", e))?;
    let mut last = String::from("联机组件下载失败");
    for url in urls {
        match download_one(url, target, sha, app) {
            Ok(n) => return Ok(n),
            Err(e) => {
                crate::download::note_host_failure(url);
                last = e;
            }
        }
    }
    Err(last)
}

fn download_one(
    url: &str,
    target: &Path,
    sha: Option<&str>,
    app: &AppHandle,
) -> Result<u64, String> {
    let resp = crate::net::open_stream(url, &[], None)?;
    let total = resp
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    if let Some(t) = total {
        if t > MAX_BYTES {
            return Err("组件体积异常，已中止".into());
        }
    }
    let mut file = fs::File::create(target).map_err(|e| format!("无法写入组件：{}", e))?;
    let mut stream = resp;
    let mut done = 0u64;
    let mut last_emit = 0u64;
    let mut buf = [0u8; 1 << 16];
    loop {
        let n = stream
            .read(&mut buf)
            .map_err(|e| format!("下载中断：{}", e))?;
        if n == 0 {
            break;
        }
        if done + n as u64 > MAX_BYTES {
            return Err("组件体积异常，已中止".into());
        }
        std::io::Write::write_all(&mut file, &buf[..n]).map_err(|e| format!("写入失败：{}", e))?;
        done += n as u64;
        if done - last_emit >= 131_072 {
            last_emit = done;
            emit_download_progress(app, done, total);
        }
    }
    std::io::Write::flush(&mut file).map_err(|e| format!("写入失败：{}", e))?;
    drop(file);
    if let Some(expected) = sha {
        let actual = sha256_of(target)?;
        if actual != expected {
            let _ = fs::remove_file(target);
            return Err("组件校验不通过，已丢弃".into());
        }
    }
    crate::download::note_host_success(url);
    Ok(done)
}

/// 读组件自报的版本与能力，纯本地调用。自报值只作诊断，版本判断一律以 release tag 为准。
pub fn probe(app: &tauri::AppHandle) -> Value {
    let target = binary_path(app);
    if !target.is_file() {
        return json!({ "ok": false, "error": "组件尚未就绪" });
    }
    match crate::vlcli::cmd::run(&target, &["--version".into()], PROBE_TIMEOUT) {
        Ok(v) => v,
        Err(e) => json!({ "ok": false, "error": e.message }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sum_lines_in_both_styles() {
        let sums = "da1dee4d081a4d62e82455e857d08393a7fa47875c35f3cde1a0541435079ad8 *VoxLink-CLI-windows-amd64.exe\n\
                    1111111111111111111111111111111111111111111111111111111111111111  other.txt\n";
        assert_eq!(
            expected_sha(sums, "VoxLink-CLI-windows-amd64.exe").unwrap(),
            "da1dee4d081a4d62e82455e857d08393a7fa47875c35f3cde1a0541435079ad8"
        );
        assert_eq!(expected_sha(sums, "missing.exe"), None);
    }

    #[test]
    fn rejects_short_hash() {
        let sums = "abc  VoxLink-CLI-windows-amd64.exe\n";
        assert!(expected_sha(sums, "VoxLink-CLI-windows-amd64.exe").is_none());
    }

    #[test]
    fn matches_windows_path_in_sums() {
        let sums = "ab1dee4d081a4d62e82455e857d08393a7fa47875c35f3cde1a0541435079ad8  sub\\VoxLink-CLI-windows-amd64.exe\n";
        assert!(expected_sha(sums, "VoxLink-CLI-windows-amd64.exe").is_some());
    }

    #[test]
    fn platform_assets_match_published_targets() {
        // 上游 1.0.1 实际发布五个 CLI 平台（SHA256SUMS.txt 实测）；windows/arm64 未发布
        let published = [
            ("windows", "x86_64"),
            ("macos", "x86_64"),
            ("macos", "aarch64"),
            ("linux", "x86_64"),
            ("linux", "aarch64"),
        ];
        for (os, arch) in published {
            assert!(asset_for(os, arch).is_some(), "{os}/{arch} 应当有资产");
        }
        assert!(asset_for("windows", "aarch64").is_none());
        assert!(asset_for("android", "x86_64").is_none());
    }

    #[test]
    fn tag_parser_reads_release_json() {
        assert_eq!(
            tag_of_json(r#"{"tag_name":"1.0.1","name":"1.0.1"}"#).as_deref(),
            Some("1.0.1")
        );
        assert_eq!(tag_of_json(r#"{"tag_name":"  "}"#), None);
        assert_eq!(tag_of_json("not json"), None);
    }

    #[test]
    fn version_compare_prefers_semver_max() {
        // 滞后镜像不得把版本钉回去：1.0.1 必须赢过 1.0.0
        assert!(version_newer("1.0.1", "1.0.0"));
        assert!(!version_newer("1.0.0", "1.0.1"));
        assert!(version_newer("1.0.10", "1.0.9"));
        assert!(version_newer("v1.1.0", "1.0.99"));
        assert!(version_newer("1.1", "1.0.9"));
        assert!(!version_newer("1.0.1", "1.0.1"));
        // 解析不动的 tag 不参与"更新"判定
        assert!(!version_newer("beta", "1.0.0"));
        assert!(!version_newer("1.0.1", ""));
    }
}
