//! 下载引擎（对应原版 KAMUCL-1.1.21/src/main/core/ 的下载子系统）。
//!
//! 函数 → 原版来源映射：
//! - [`mirror_url`]                        ← download.ts `mirrorUrl()`（BMCLAPI 镜像改写表）
//! - [`download_candidates`]               ← download.ts `downloadCandidates()`（forgecdn
//!   mediafilez/edge 变体 + 镜像前插 + 去重）
//! - [`classify_http_status`]              ← download.ts `classifyHttpStatus()`
//! - [`note_host_failure`] / [`note_host_success`] / [`note_hosts_recovered`] / [`host_cooling`]
//!                                         ← download.ts `noteHostFailure`/`noteHostSuccess`/
//!   `noteHostsRecovered` + 候选排序中的 failures 冷却判断
//! - [`retry_after_time`]                  ← download.ts `retryAfterTime()`
//! - server cooldowns（429/503 Retry-After 共享冷却）← download.ts `serverCooldowns` +
//!   `waitForServerCooldown()`（[`wait_for_server_cooldown`]）+ [`CooldownBudget`]
//!   （download.ts `CooldownBudget`，30s 共享等待预算，暂停不计时）
//! - [`DownloadLimiter`] / [`limiter`] / [`validate_limits`] / [`configure_limiter_from_settings`]
//!                                         ← downloadLimits.ts `DownloadLimiter`（并发闸门 +
//!   令牌桶限速，全局单例）/`validateDownloadLimits`/settings.ts 132-133/256 的 configure 接线
//! - [`with_file_job`]                     ← fileJobs.ts `withFileJob`（同目标串行）
//! - [`DownloadSourcePool`]                ← downloadSources.ts（按实测成本排序候选源）
//! - [`DownloadProgressTracker`]           ← downloadProgress.ts（字节聚合 + epoch 锚点，进度不回退）
//! - [`SmoothedSpeedEstimator`]            ← downloadProgress.ts（6s 窗口速率 + ETA，暂停清零）
//! - [`receive`]                           ← download.ts `receive()`（单连接：断点续传 Range、
//!   15s 不活动中止、429 冷却登记、限速令牌、Content-Range/大小校验、写入与 fsync）
//! - [`reuse`]                             ← download.ts `reuse()`（reuseFiles/reuseDirs 本地复用）
//! - [`download_file`]                     ← download.ts `downloadFile()`（重试 + 镜像回退 +
//!   .part 临时文件 + 哈希校验 + 原子改名）
//! - [`download_all`]                      ← download.ts `downloadAll()`（worker 池、大文件优先 +
//!   每 4 个 worker 预留小文件、firstError 取消兄弟、250ms 汇报节流）
//! - [`asset_download_status`]             ← shared/assetDownloadStatus.ts `assetDownloadStatus()`
//! - [`ParallelProgress`]                  ← parallelProgress.ts `ParallelProgress`（多泳道加权
//!   合成 + 100ms 节流 + parallelStages）
//! - [`ProgressGuard`] / [`WeightedProgress`] / [`version_install_stage_range`]
//!                                         ← progress.ts `ProgressEventGuard`/
//!   `createWeightedProgressEmit`/`VERSION_INSTALL_STAGE_RANGES`
//!
//! 与原版的差异（语义保持一致的前提下）：
//! 1. 原版基于 AbortSignal 的取消/暂停 → 轮询 TaskRecord.cancelled/paused（tasks.rs 令牌模型），
//!    检查点位于块读写/文件/候选循环边界；「已取消」文案与原版一致。
//! 2. 大文件多 Range 分片引擎（download.ts `segmented()`）未移植：≥1MiB 文件退化为
//!    单连接 + `Range: bytes=offset-` 断点续传（哈希仍全量校验后原子提交）。
//! 3. 原版首响应 2.5s 切换备用源 / 慢速窗口切换（NetworkIdle watchdog 的预判中止）未移植
//!    —— 阻塞读无法预先打断；保留 15s 无数据中止（『下载网络停滞』），由
//!    download_client 的 120s 读超时兜底硬停。
//! 4. 15s 无数据看门狗在读返回后回溯判定（原版为预判定时器），暂停/限速期间不计时。
//! 5. macOS ARM64 原生库校验值解析（platformNatives.resolveNativeIntegrity）与 ≥50MiB
//!    磁盘空间预检未移植（windows-sys 未启用文件系统特性且不允许加依赖）。
//! 6. 事件经 [`crate::tasks::emit_progress`] 推 `event:progress`；ProgressEvent 字段名与
//!    shared/types.ts 逐字一致（stage/progress/overall/text/speed/etaSeconds/bytesDone/
//!    bytesTotal/indeterminate/source/taskId/taskTitle/manualFiles…），文件数进 text。
//!    downloadFile 层不发事件（与原版一致，事件由任务包装层发）；download_all 直接发。
//! 7. 失败终态错误文案与原版一致：`下载失败：{msg}\n文件：{name}\n地址：{origin+path}`；
//!    带标签批量文件失败前缀 `资源文件 {label}：`。取消统一为 Err("已取消")，
//!    调用方据 [`is_cancel_error`] 发 event:taskDone cancelled。

use crate::net::{self, fmt_mb, verify_file, Integrity};
use crate::tasks::{wait_while_paused, TaskRecord};
use reqwest::Url;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const BMCL_MAVEN_ROOT: &str = "https://bmclapi2.bangbang93.com/maven/";

// ---------------- 镜像选择（download.ts mirrorUrl/downloadCandidates） ----------------

/// 对应原版 `type MirrorPref = 'official' | 'bmclapi'`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirrorPref {
    Official,
    Bmclapi,
}

pub fn mirror_pref_of(s: &str) -> MirrorPref {
    if s == "bmclapi" {
        MirrorPref::Bmclapi
    } else {
        MirrorPref::Official
    }
}

/// 对应原版 versions.ts：`mirror === 'bmclapi' ? 'BMCLAPI 镜像' : '官方源'`。
pub fn source_text(mirror: MirrorPref) -> &'static str {
    match mirror {
        MirrorPref::Bmclapi => "BMCLAPI 镜像",
        MirrorPref::Official => "官方源",
    }
}

pub fn mirror_from_settings(app: &tauri::AppHandle) -> MirrorPref {
    let s = crate::settings::get(app);
    mirror_pref_of(s.get("mirror").and_then(|v| v.as_str()).unwrap_or("official"))
}

static FORGECDN_FILES: OnceLock<regex::Regex> = OnceLock::new();
fn forgecdn_files_re() -> &'static regex::Regex {
    FORGECDN_FILES.get_or_init(|| regex::Regex::new(r"^/files/\d+/\d+/").unwrap())
}

static MODRINTH_DATA: OnceLock<regex::Regex> = OnceLock::new();
fn modrinth_data_re() -> &'static regex::Regex {
    MODRINTH_DATA.get_or_init(|| regex::Regex::new(r"^/data/[^/]+/versions/").unwrap())
}

/// 对应原版 download.ts `mirrorUrl()`：官方域名 → BMCLAPI / mcimirror 改写，非镜像域原样返回。
pub fn mirror_url(input: &str, mirror: MirrorPref) -> String {
    if mirror == MirrorPref::Official {
        return input.to_string();
    }
    let Ok(u) = Url::parse(input) else { return input.to_string() };
    if !matches!(u.scheme(), "https" | "http") {
        return input.to_string();
    }
    let host = u.host_str().unwrap_or_default();
    let p = u.path();
    let q = u.query().map(|v| format!("?{v}")).unwrap_or_default();
    let base = "https://bmclapi2.bangbang93.com";
    match host {
        "piston-meta.mojang.com" | "piston-data.mojang.com" | "launchermeta.mojang.com" | "launcher.mojang.com" => {
            format!("{base}{p}{q}")
        }
        "resources.download.minecraft.net" => format!("{base}/assets{p}{q}"),
        "libraries.minecraft.net" | "maven.fabricmc.net" | "maven.minecraftforge.net" => {
            format!("{BMCL_MAVEN_ROOT}{}{q}", p.strip_prefix('/').unwrap_or(p))
        }
        "maven.neoforged.net" if p.starts_with("/releases/") => {
            format!("{BMCL_MAVEN_ROOT}{}{q}", p.strip_prefix("/releases/").unwrap_or(p))
        }
        "files.minecraftforge.net" if p.starts_with("/maven/") => format!("{base}{p}{q}"),
        "cdn.modrinth.com" if modrinth_data_re().is_match(p) => format!("https://mod.mcimirror.top{p}{q}"),
        "edge.forgecdn.net" | "mediafilez.forgecdn.net" if forgecdn_files_re().is_match(p) => {
            format!("https://mod.mcimirror.top{p}{q}")
        }
        _ => input.to_string(),
    }
}

/// 对应原版 download.ts `downloadCandidates()`：每个 url 展开为
/// [镜像变体?, mediafilez 变体, 原始 url, edge 变体]，跨任务去重保序。
pub fn download_candidates(urls: &[String], mirror: MirrorPref) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for url in urls {
        let mut variants: Vec<String> = vec![url.clone()];
        if let Ok(mut u) = Url::parse(url) {
            if u.scheme() == "https"
                && matches!(u.host_str(), Some("edge.forgecdn.net") | Some("mediafilez.forgecdn.net"))
                && forgecdn_files_re().is_match(u.path())
            {
                if u.set_host(Some("mediafilez.forgecdn.net")).is_ok() {
                    variants.insert(0, u.to_string());
                }
                if u.set_host(Some("edge.forgecdn.net")).is_ok() {
                    variants.push(u.to_string());
                }
            }
        }
        let mut list: Vec<String> = Vec::new();
        if mirror != MirrorPref::Official {
            list.push(mirror_url(url, mirror));
        }
        list.extend(variants);
        for v in list {
            if !out.contains(&v) {
                out.push(v);
            }
        }
    }
    out
}

// ---------------- HTTP 失败分类与主机健康（download.ts） ----------------

/// 对应原版 `HttpFailureKind`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpFailureKind {
    Unavailable,
    Transient,
    Fatal,
}

/// 对应原版 download.ts `classifyHttpStatus()`。
pub fn classify_http_status(status: u16) -> HttpFailureKind {
    if matches!(status, 404 | 410) {
        return HttpFailureKind::Unavailable;
    }
    if matches!(status, 304 | 408 | 425 | 429) || ((500..600).contains(&status) && !matches!(status, 501 | 505)) {
        HttpFailureKind::Transient
    } else {
        HttpFailureKind::Fatal
    }
}

fn host_failures() -> &'static Mutex<HashMap<String, (u32, u64)>> {
    static M: OnceLock<Mutex<HashMap<String, (u32, u64)>>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(HashMap::new()))
}

fn server_cooldowns() -> &'static Mutex<HashMap<String, u64>> {
    static M: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn mono_ms() -> f64 {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    EPOCH.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0
}

fn origin_of(url: &str) -> String {
    Url::parse(url)
        .map(|u| u.origin().ascii_serialization())
        .unwrap_or_else(|_| url.to_string())
}

fn local_time(ms: u64) -> String {
    chrono::DateTime::from_timestamp_millis(ms as i64)
        .map(|dt| dt.with_timezone(&chrono::Local).format("%Y/%m/%d %H:%M:%S").to_string())
        .unwrap_or_default()
}

/// 对应原版 download.ts `retryAfterTime()`。
pub fn retry_after_time(value: Option<&str>, now: u64) -> u64 {
    let Some(v) = value else { return now + 30_000 };
    if v.is_empty() {
        return now;
    }
    let until = if let Ok(sec) = v.trim().parse::<f64>() {
        if sec.is_finite() {
            now + (sec.max(0.0) * 1000.0) as u64
        } else {
            now + 30_000
        }
    } else if let Ok(dt) = chrono::DateTime::parse_from_rfc2822(v.trim()) {
        dt.timestamp_millis().max(0) as u64
    } else {
        now + 30_000
    };
    until.max(now)
}

/// 对应原版 download.ts `noteHostFailure()`。
pub fn note_host_failure(url: &str) {
    let key = origin_of(url);
    let mut m = host_failures().lock().unwrap();
    let count = m.get(&key).map(|(c, _)| *c).unwrap_or(0) + 1;
    let until = if count < 2 { 0 } else { now_ms() + (count as u64 * 60_000).min(600_000) };
    m.insert(key, (count, until));
}

/// 对应原版 download.ts `noteHostSuccess()`。
pub fn note_host_success(url: &str) {
    host_failures().lock().unwrap().remove(&origin_of(url));
}

/// 对应原版 download.ts `noteHostsRecovered()`。
pub fn note_hosts_recovered() {
    host_failures().lock().unwrap().clear();
}

/// 候选排序用：该源当前处于失败冷却或限流冷却（对应原版排序键
/// `Math.max(failures.get(origin)?.until ?? 0, serverCooldowns.get(origin) ?? 0) > Date.now()`）。
fn host_cooling(url: &str) -> bool {
    let key = origin_of(url);
    let now = now_ms();
    let failures_until = host_failures().lock().unwrap().get(&key).map(|(_, u)| *u).unwrap_or(0);
    let cooldown = server_cooldowns().lock().unwrap().get(&key).copied().unwrap_or(0);
    failures_until.max(cooldown) > now
}

/// 限流等待状态（对应原版 download.ts `DownloadWaitState`）。
#[derive(Debug, Clone)]
pub struct WaitState {
    pub reason: String,   // "rate-limit"
    pub source: String,   // origin
    pub retry_at: u64,    // epoch ms
    pub action: String,   // "waiting" | "switching"
    pub file: Option<String>,
}

/// 对应原版 download.ts `CooldownBudget`：一个文件的全部重试/分片共享 30s 等待预算，
/// 暂停期间不计时。轮询采样替代原版的 50ms setInterval。
pub struct CooldownBudget {
    limit_ms: u64,
    st: Mutex<(u32, u64, Instant)>, // users, waited_ms, sampled_at
    paused: Arc<AtomicBool>,
}

impl CooldownBudget {
    pub fn new(rec: Arc<TaskRecord>) -> Self {
        Self::from_paused(rec.paused.clone())
    }

    pub fn from_paused(paused: Arc<AtomicBool>) -> Self {
        Self { limit_ms: 30_000, st: Mutex::new((0, 0, Instant::now())), paused }
    }

    fn sample(&self) {
        let mut st = self.st.lock().unwrap();
        let now = Instant::now();
        if st.0 > 0 && !self.paused.load(Ordering::SeqCst) {
            st.1 += (now - st.2).as_millis() as u64;
        }
        st.2 = now;
    }

    pub fn remaining(&self) -> u64 {
        self.sample();
        self.limit_ms.saturating_sub(self.st.lock().unwrap().1)
    }

    pub fn enter(&self) {
        self.sample();
        self.st.lock().unwrap().0 += 1;
    }

    pub fn leave(&self) {
        self.sample();
        let mut st = self.st.lock().unwrap();
        st.0 = st.0.saturating_sub(1);
    }
}

fn can_avoid_cooldown(url: &str, alternatives: &[String]) -> bool {
    let origin = origin_of(url);
    let now = now_ms();
    let cd = server_cooldowns().lock().unwrap();
    alternatives
        .iter()
        .any(|other| origin_of(other) != origin && cd.get(&origin_of(other)).copied().unwrap_or(0) <= now)
}

/// 对应原版 download.ts `waitForServerCooldown()`：等到该源限流冷却结束；
/// 有可切换替代源或预算不足时抛 DownloadRateLimitError（文案原样）。
fn wait_for_server_cooldown(
    url: &str,
    rec: &TaskRecord,
    budget: Option<&CooldownBudget>,
    alternatives: &[String],
    on_wait: Option<&(dyn Fn(&WaitState, bool) + Sync)>,
) -> Result<(), XferErr> {
    wait_while_paused(rec);
    check_cancel(rec).map_err(|_| XferErr::Cancelled)?;
    let owned;
    let budget: &CooldownBudget = match budget {
        Some(b) => b,
        None => {
            owned = CooldownBudget::from_paused(rec.paused.clone());
            &owned
        }
    };
    let mut until = server_cooldowns().lock().unwrap().get(&origin_of(url)).copied().unwrap_or(0);
    if until <= now_ms() {
        return Ok(());
    }
    let switching = can_avoid_cooldown(url, alternatives);
    let mut state = WaitState {
        reason: "rate-limit".to_string(),
        source: origin_of(url),
        retry_at: until,
        action: if switching { "switching" } else { "waiting" }.to_string(),
        file: None,
    };
    if let Some(cb) = on_wait {
        cb(&state, true);
    }
    budget.enter();
    let result = loop {
        wait_while_paused(rec);
        if check_cancel(rec).is_err() {
            break Err(XferErr::Cancelled);
        }
        until = server_cooldowns().lock().unwrap().get(&origin_of(url)).copied().unwrap_or(0);
        state.retry_at = until;
        let remaining = until.saturating_sub(now_ms());
        if remaining == 0 {
            break Ok(());
        }
        if switching || remaining > budget.remaining() {
            break Err(XferErr::RateLimit { url: url.to_string(), retry_at: until });
        }
        let sleep_ms = 100u64.min(remaining).min(budget.remaining().max(1));
        if abortable_delay(sleep_ms, rec).is_err() {
            break Err(XferErr::Cancelled);
        }
    };
    budget.leave();
    if let Some(cb) = on_wait {
        cb(&state, false);
    }
    result
}

// ---------------- 内部错误类型（对应原版 DownloadHttpError/InvalidContent/DownloadRateLimitError） ----------------

#[derive(Debug)]
enum XferErr {
    Cancelled,
    RateLimit { url: String, retry_at: u64 },
    Http { status: u16, url: String },
    Invalid(String),
    Network(String),
    Disk(String),
}

impl XferErr {
    fn message(&self) -> String {
        match self {
            XferErr::Cancelled => "已取消".to_string(),
            XferErr::RateLimit { url, retry_at } => {
                format!("下载源限流，可稍后重试（最早 {}）：{}", local_time(*retry_at), origin_of(url))
            }
            XferErr::Http { status, url } => format!("HTTP {status}: {url}"),
            XferErr::Invalid(r) => r.clone(),
            XferErr::Network(m) => m.clone(),
            XferErr::Disk(m) => m.clone(),
        }
    }

    fn url(&self) -> Option<&str> {
        match self {
            XferErr::RateLimit { url, .. } | XferErr::Http { url, .. } => Some(url),
            _ => None,
        }
    }

    fn from_str_msg(msg: String) -> Self {
        if msg == "已取消" || msg.contains("取消") {
            XferErr::Cancelled
        } else {
            XferErr::Network(msg)
        }
    }
}

fn check_cancel(rec: &TaskRecord) -> Result<(), String> {
    if rec.cancelled.load(Ordering::SeqCst) {
        Err("已取消".to_string())
    } else {
        Ok(())
    }
}

/// 可取消的退避等待（对应原版 tasks.ts `abortableDelay`；取消即时返回，暂停不阻断）。
fn abortable_delay(ms: u64, rec: &TaskRecord) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_millis(ms);
    while Instant::now() < deadline {
        check_cancel(rec)?;
        let remain = deadline.duration_since(Instant::now());
        std::thread::sleep(remain.min(Duration::from_millis(20)));
    }
    check_cancel(rec)
}

fn basename(p: &Path) -> String {
    p.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.to_string_lossy().into_owned())
}

fn normalize_path(p: &Path) -> String {
    let abs = std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    if cfg!(windows) {
        abs.to_string_lossy().to_lowercase()
    } else {
        abs.to_string_lossy().into_owned()
    }
}

fn part_path(dest: &Path) -> PathBuf {
    let mut os = dest.as_os_str().to_os_string();
    os.push(".part");
    PathBuf::from(os)
}

// ---------------- 限速 / 并发闸门（downloadLimits.ts DownloadLimiter） ----------------

fn validate_pair(threads: i64, kbps: i64) -> Result<(), String> {
    if !(1..=64).contains(&threads) {
        return Err("最大线程数必须是 1–64 的整数".to_string());
    }
    if !(0..=1048576).contains(&kbps) {
        return Err("速度限制必须是 0–1048576 KiB/s 的整数，0 表示不限速".to_string());
    }
    Ok(())
}

/// 对应原版 downloadLimits.ts `validateDownloadLimits()`：校验 settings（含
/// downloadThreads/downloadSpeedKBps 的对象），错误文案原样。
pub fn validate_limits(value: &Value) -> Result<(), String> {
    let threads = value
        .get("downloadThreads")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| "最大线程数必须是 1–64 的整数".to_string())?;
    let kbps = value
        .get("downloadSpeedKBps")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| "速度限制必须是 0–1048576 KiB/s 的整数，0 表示不限速".to_string())?;
    validate_pair(threads, kbps)
}

/// 所有下载共用一个并发闸门与字节预算（对应原版 `downloadLimiter` 全局单例）。
/// 跨安装任务也不会叠加突破限制。
pub struct DownloadLimiter {
    st: Mutex<LimiterState>,
    cv: Condvar,
}

struct LimiterState {
    threads: usize,
    kbps: u32,
    active: usize,
    tokens: f64,
    sampled_at: Instant,
}

impl DownloadLimiter {
    pub fn new() -> Self {
        Self {
            st: Mutex::new(LimiterState { threads: 16, kbps: 0, active: 0, tokens: 0.0, sampled_at: Instant::now() }),
            cv: Condvar::new(),
        }
    }

    /// 对应原版 `configure()`：校验 + 重置令牌 + 唤醒排队者。
    pub fn configure(&self, threads: i64, kbps: i64) -> Result<(), String> {
        validate_pair(threads, kbps)?;
        {
            let mut st = self.st.lock().unwrap();
            st.threads = threads.max(1) as usize;
            st.kbps = kbps.max(0) as u32;
            st.tokens = 0.0;
            st.sampled_at = Instant::now();
        }
        self.cv.notify_all();
        Ok(())
    }

    pub fn max_concurrent(&self) -> usize {
        self.st.lock().unwrap().threads
    }

    /// 当前是否启用了限速（原版 `isThrottling`：慢速连接检测在限速时跳过）。
    pub fn is_throttling(&self) -> bool {
        self.st.lock().unwrap().kbps > 0
    }

    /// 对应原版 `acquire()`：拿一个请求槽位；等待期间可被取消（暂停不清空队列位置）。
    pub fn acquire(self: &Arc<Self>, rec: Option<&TaskRecord>) -> Result<LimiterSlot, String> {
        if let Some(rec) = rec {
            wait_while_paused(rec);
            if rec.cancelled.load(Ordering::SeqCst) {
                return Err("已取消".to_string());
            }
        }
        let mut st = self.st.lock().unwrap();
        loop {
            if st.active < st.threads {
                st.active += 1;
                return Ok(LimiterSlot { limiter: self.clone() });
            }
            if let Some(rec) = rec {
                if rec.cancelled.load(Ordering::SeqCst) {
                    return Err("已取消".to_string());
                }
            }
            let (guard, _) = self.cv.wait_timeout(st, Duration::from_millis(100)).unwrap();
            st = guard;
        }
    }

    /// 对应原版 `consume()`：令牌桶限速，容量 0.15s、补充速率 kbps*1024 B/s。
    pub fn consume(&self, bytes: usize, rec: Option<&TaskRecord>) -> Result<(), String> {
        let mut remaining = bytes as f64;
        while remaining > 0.0 {
            if let Some(rec) = rec {
                if rec.cancelled.load(Ordering::SeqCst) {
                    return Err("已取消".to_string());
                }
            }
            let rate = {
                let mut st = self.st.lock().unwrap();
                let rate = st.kbps as f64 * 1024.0;
                if rate <= 0.0 {
                    return Ok(());
                }
                let now = Instant::now();
                let dt_ms = now.duration_since(st.sampled_at).as_secs_f64() * 1000.0;
                st.tokens = (rate * 0.15).min(st.tokens + dt_ms * rate / 1000.0);
                st.sampled_at = now;
                let take = remaining.min(st.tokens.floor());
                st.tokens -= take;
                remaining -= take;
                rate
            };
            if remaining > 0.0 {
                let delay_ms = 100.0f64.min(5.0f64.max(remaining / rate * 1000.0));
                match rec {
                    Some(rec) => abortable_delay(delay_ms as u64, rec)?,
                    None => std::thread::sleep(Duration::from_millis(delay_ms as u64)),
                }
            }
        }
        Ok(())
    }
}

impl Default for DownloadLimiter {
    fn default() -> Self {
        Self::new()
    }
}

/// 请求槽位守卫：Drop 时归还并唤醒队列（对应原版 acquire 返回的 release()）。
pub struct LimiterSlot {
    limiter: Arc<DownloadLimiter>,
}

impl Drop for LimiterSlot {
    fn drop(&mut self) {
        {
            let mut st = self.limiter.st.lock().unwrap();
            st.active = st.active.saturating_sub(1);
        }
        self.limiter.cv.notify_all();
    }
}

/// 全局限速器单例（对应原版 `downloadLimiter` 导出）。其他模块可读
/// max_concurrent()/is_throttling()；写侧走 configure_limiter_from_settings。
pub fn limiter() -> Arc<DownloadLimiter> {
    static LIMITER: OnceLock<Arc<DownloadLimiter>> = OnceLock::new();
    LIMITER.get_or_init(|| Arc::new(DownloadLimiter::new())).clone()
}

/// settings:set 保存线程/限速后调用（对应原版 settings.ts:256
/// `if (patch.downloadThreads !== undefined || patch.downloadSpeedKBps !== undefined) downloadLimiter.configure(merged)`）。
pub fn configure_limiter_from_settings(app: &tauri::AppHandle) {
    let s = crate::settings::get(app);
    let threads = s.get("downloadThreads").and_then(|v| v.as_i64()).unwrap_or(16);
    let kbps = s.get("downloadSpeedKBps").and_then(|v| v.as_i64()).unwrap_or(0);
    // 对应原版 settings.ts:132：校验失败回落默认值
    if validate_pair(threads, kbps).is_ok() {
        let _ = limiter().configure(threads, kbps);
    } else {
        let _ = limiter().configure(16, 0);
    }
}

/// 进程内首次使用时按当前设置初始化（对应原版 settings.ts:132-133 的加载期 configure）。
pub fn init_limiter_from_settings(app: &tauri::AppHandle) {
    static INIT: OnceLock<()> = OnceLock::new();
    INIT.get_or_init(|| configure_limiter_from_settings(app));
}

// ---------------- 文件任务串行（fileJobs.ts withFileJob） ----------------

fn file_jobs() -> &'static Mutex<HashMap<String, Arc<(Mutex<u32>, Condvar)>>> {
    static M: OnceLock<Mutex<HashMap<String, Arc<(Mutex<u32>, Condvar)>>>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(HashMap::new()))
}

fn file_job_key(dest: &Path) -> String {
    // 对应原版 fileJobs.ts fileJobKey()：win32 解析后小写化
    normalize_path(dest)
}

/// 同一目标文件串行（对应原版 `withFileJob`）；等待期间可取消。
pub fn with_file_job<T>(dest: &Path, rec: Option<&TaskRecord>, action: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    let key = file_job_key(dest);
    let slot = {
        let mut m = file_jobs().lock().unwrap();
        m.entry(key.clone()).or_insert_with(|| Arc::new((Mutex::new(0u32), Condvar::new()))).clone()
    };
    let (counter, cv) = &*slot;
    {
        let mut guard = counter.lock().unwrap();
        loop {
            if *guard == 0 {
                *guard = 1;
                break;
            }
            if let Some(rec) = rec {
                if rec.cancelled.load(Ordering::SeqCst) {
                    return Err("已取消".to_string());
                }
            }
            let (g, _) = cv.wait_timeout(guard, Duration::from_millis(120)).unwrap();
            guard = g;
        }
    }
    let result = action();
    {
        let mut guard = counter.lock().unwrap();
        *guard = 0;
        cv.notify_all();
    }
    // 无其他持有者时清理登记（对应原版 tails.delete）
    {
        let mut m = file_jobs().lock().unwrap();
        if Arc::strong_count(&slot) == 1 && m.get(&key).map(|s| Arc::ptr_eq(s, &slot)).unwrap_or(false) {
            m.remove(&key);
        }
    }
    result
}

// ---------------- 下载源学习池（downloadSources.ts DownloadSourcePool） ----------------

#[derive(Debug, Clone, Copy, Default)]
struct SourceSample {
    trials: u32,
    samples: u32,
    headers_ms: f64,
    bytes_per_ms: f64,
    failures: u32,
}

fn source_key(url: &str) -> String {
    match Url::parse(url) {
        Ok(u) => {
            let p = u.path();
            let suffix = if p.starts_with("/assets/") {
                "/assets"
            } else if p.starts_with("/maven/") {
                "/maven"
            } else {
                ""
            };
            format!("{}{}", u.origin().ascii_serialization(), suffix)
        }
        Err(_) => url.to_string(),
    }
}

/// 对应原版 downloadSources.ts `DownloadSourcePool`：每个来源采样两次后按预测
/// 请求成本排序；观察值批量内有效，不跨网络/设置保存。
pub struct DownloadSourcePool {
    samples: HashMap<String, SourceSample>,
}

pub type SourcePoolRef = Arc<Mutex<DownloadSourcePool>>;

impl DownloadSourcePool {
    pub fn new() -> Self {
        Self { samples: HashMap::new() }
    }

    fn sample_of(&self, url: &str) -> SourceSample {
        *self.samples.get(&source_key(url)).unwrap_or(&SourceSample::default())
    }

    fn sample_mut(&mut self, url: &str) -> &mut SourceSample {
        self.samples.entry(source_key(url)).or_default()
    }

    /// 对应原版 `order()`：先让未试过的候选源探路一次，再按成本升序。
    pub fn order(&mut self, urls: &[String], size: u64) -> Vec<String> {
        if urls.len() < 2 {
            return urls.to_vec();
        }
        let trial: Option<String> = urls
            .iter()
            .find(|u| {
                let s = self.sample_of(u);
                s.trials < 2 && s.failures == 0
            })
            .cloned();
        let cost = |s: &SourceSample| {
            if s.samples > 0 {
                s.headers_ms + size as f64 / s.bytes_per_ms.max(1.0) + s.failures as f64 * 15_000.0
            } else {
                2500.0 + s.failures as f64 * 15_000.0
            }
        };
        let mut scored: Vec<(usize, String, SourceSample)> =
            urls.iter().enumerate().map(|(i, u)| (i, u.clone(), self.sample_of(u))).collect();
        scored.sort_by(|a, b| {
            let a_trial = trial.as_deref() == Some(a.1.as_str());
            let b_trial = trial.as_deref() == Some(b.1.as_str());
            b_trial
                .cmp(&a_trial)
                .then_with(|| cost(&a.2).partial_cmp(&cost(&b.2)).unwrap_or(std::cmp::Ordering::Equal))
                .then_with(|| a.0.cmp(&b.0))
        });
        let winner = scored[0].1.clone();
        self.sample_mut(&winner).trials += 1;
        scored.into_iter().map(|(_, u, _)| u).collect()
    }

    /// 对应原版 `observe()`：EWMA 记录首字节与吞吐（首次全量、其后 0.25 权重）。
    pub fn observe(&mut self, url: &str, headers_ms: f64, body_ms: f64, bytes: u64) {
        if bytes == 0 {
            return;
        }
        let s = self.sample_mut(url);
        let weight = if s.samples > 0 { 0.25 } else { 1.0 };
        s.headers_ms += (headers_ms - s.headers_ms) * weight;
        s.bytes_per_ms += (bytes as f64 / body_ms.max(1.0) - s.bytes_per_ms) * weight;
        s.samples += 1;
        s.failures = s.failures.saturating_sub(1);
    }

    /// 对应原版 `fail()`。
    pub fn fail(&mut self, url: &str) {
        let s = self.sample_mut(url);
        s.failures = (s.failures + 1).min(4);
    }
}

impl Default for DownloadSourcePool {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------- 进度聚合（downloadProgress.ts） ----------------

/// 对应原版 `DownloadProgressSnapshot`。
#[derive(Debug, Clone)]
pub struct DownloadProgressSnapshot {
    pub completed_files: u64,
    pub total_files: u64,
    pub bytes_done: u64,
    /// None 表示仍有文件总大小未知。
    pub bytes_total: Option<u64>,
    /// None 时调用方必须显示不确定进度。
    pub fraction: Option<f64>,
    pub indeterminate: bool,
}

struct ProgressEntry {
    expected: Option<u64>,
    transferred: u64,
    complete: bool,
}

/// 对应原版 downloadProgress.ts `DownloadProgressTracker`：按每个文件「已持久化字节」
/// 聚合并发下载进度；entry 只增不减；动态新增文件时建立新 epoch，百分比不回退。
pub struct DownloadProgressTracker {
    entries: Vec<ProgressEntry>,
    sealed: bool,
    last_fraction: f64,
    anchor_fraction: f64,
    anchor_done: u64,
    anchor_remaining: Option<u64>,
    has_determinate_epoch: bool,
}

impl DownloadProgressTracker {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            sealed: false,
            last_fraction: 0.0,
            anchor_fraction: 0.0,
            anchor_done: 0,
            anchor_remaining: None,
            has_determinate_epoch: false,
        }
    }

    pub fn add(&mut self, expected: Option<u64>) -> Result<usize, String> {
        if self.last_fraction >= 1.0 {
            return Err("已完成的进度任务不能再添加文件".to_string());
        }
        let was_determinate = self.is_determinate();
        let done_before = if self.sealed { self.bytes_done() } else { 0 };
        if self.sealed && was_determinate {
            self.snapshot();
        }
        let index = self.entries.len();
        self.entries.push(ProgressEntry { expected, transferred: 0, complete: false });
        if self.sealed && was_determinate {
            self.anchor_fraction = self.last_fraction;
            self.anchor_done = done_before;
            self.anchor_remaining = self.total_expected().map(|t| t.saturating_sub(done_before));
            self.has_determinate_epoch = true;
        }
        Ok(index)
    }

    /// 初始任务发现完成后调用；seal 前只报告不确定进度。
    pub fn seal(&mut self) {
        self.sealed = true;
        self.ensure_epoch();
    }

    /// 热路径：只更新 entry，聚合延迟到汇报定时器（对应原版 `record`）。
    pub fn record(&mut self, index: usize, transferred: u64, discovered_total: u64) {
        let entry = self.entry(index);
        if entry.expected.is_none() && discovered_total > 0 {
            entry.expected = Some(discovered_total);
        }
        let capped = match entry.expected {
            Some(e) => transferred.min(e),
            None => transferred,
        };
        entry.transferred = entry.transferred.max(capped);
    }

    pub fn record_complete(&mut self, index: usize, actual_size: u64) {
        let entry = self.entry(index);
        if entry.expected.is_none() && actual_size > 0 {
            entry.expected = Some(actual_size);
        }
        match entry.expected {
            Some(e) => entry.transferred = entry.transferred.max(e),
            None => entry.transferred = entry.transferred.max(actual_size),
        }
        entry.complete = true;
    }

    pub fn snapshot(&mut self) -> DownloadProgressSnapshot {
        let bytes_done = self.bytes_done();
        let bytes_total = self.total_expected();
        let complete = !self.entries.is_empty() && self.entries.iter().all(|e| e.complete);
        if !self.sealed || bytes_total.is_none() {
            return DownloadProgressSnapshot {
                completed_files: self.completed_files(),
                total_files: self.entries.len() as u64,
                bytes_done,
                bytes_total: None,
                fraction: if complete { Some(1.0) } else { None },
                indeterminate: !complete,
            };
        }
        self.ensure_epoch();
        let fraction = if complete {
            1.0
        } else if self.anchor_remaining.map(|r| r == 0).unwrap_or(true) {
            self.anchor_fraction
        } else {
            let completed_since_anchor = bytes_done.saturating_sub(self.anchor_done) as f64;
            self.anchor_fraction
                + (1.0 - self.anchor_fraction) * (1.0f64).min(completed_since_anchor / self.anchor_remaining.unwrap() as f64)
        };
        self.last_fraction = self.last_fraction.max(clamp01(fraction));
        DownloadProgressSnapshot {
            completed_files: self.completed_files(),
            total_files: self.entries.len() as u64,
            bytes_done,
            bytes_total,
            fraction: Some(self.last_fraction),
            indeterminate: false,
        }
    }

    fn ensure_epoch(&mut self) {
        if !self.sealed || !self.is_determinate() {
            return;
        }
        if self.anchor_remaining.is_some() {
            return;
        }
        let total = self.total_expected().unwrap_or(0);
        if self.has_determinate_epoch {
            self.anchor_remaining = Some(total.saturating_sub(self.anchor_done));
        } else {
            self.anchor_fraction = 0.0;
            self.anchor_done = 0;
            self.anchor_remaining = Some(total);
            self.has_determinate_epoch = true;
        }
    }

    fn entry(&mut self, index: usize) -> &mut ProgressEntry {
        self.entries.get_mut(index).unwrap_or_else(|| panic!("未知下载进度项: {index}"))
    }

    fn is_determinate(&self) -> bool {
        self.sealed && self.entries.iter().all(|e| e.expected.is_some())
    }

    fn total_expected(&self) -> Option<u64> {
        if self.entries.iter().any(|e| e.expected.is_none()) {
            return None;
        }
        Some(self.entries.iter().filter_map(|e| e.expected).sum())
    }

    fn bytes_done(&self) -> u64 {
        self.entries.iter().map(|e| e.transferred).sum()
    }

    fn completed_files(&self) -> u64 {
        self.entries.iter().filter(|e| e.complete).count() as u64
    }
}

impl Default for DownloadProgressTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// 对应原版 `SpeedSnapshot`。
#[derive(Debug, Clone, Copy)]
pub struct SpeedSnapshot {
    pub speed_bps: u64,
    pub eta_seconds: Option<u64>,
}

/// 对应原版 downloadProgress.ts `SmoothedSpeedEstimator`：时间窗速率，只统计线上字节
/// （缓存命中/续传字节不计入）；暂停清零；250ms 内复用上次发布值。
pub struct SmoothedSpeedEstimator {
    points: Vec<(f64, u64)>,
    last_progress: f64,
    last: SpeedSnapshot,
}

impl SmoothedSpeedEstimator {
    pub fn new() -> Self {
        Self { points: Vec::new(), last_progress: 0.0, last: SpeedSnapshot { speed_bps: 0, eta_seconds: None } }
    }

    pub fn sample(&mut self, bytes: u64, remaining: Option<u64>, now_ms: f64, paused: bool) -> SpeedSnapshot {
        let end = self.points.last().copied();
        if paused || end.map(|(at, b)| bytes < b || now_ms < at).unwrap_or(false) {
            self.points.clear();
            self.last = SpeedSnapshot { speed_bps: 0, eta_seconds: None };
            return self.last;
        }
        let Some((end_at, end_bytes)) = end else {
            self.points.push((now_ms, bytes));
            self.last_progress = now_ms;
            return self.last;
        };
        if bytes > end_bytes {
            self.last_progress = now_ms;
        }
        if now_ms - end_at < 250.0 {
            return self.last;
        }
        self.points.push((now_ms, bytes));
        while self.points.len() > 2 && self.points[1].0 <= now_ms - 6000.0 {
            self.points.remove(0);
        }
        let (start_at, start_bytes) = self.points[0];
        let duration = now_ms - start_at;
        let stalled = now_ms - self.last_progress >= 4000.0;
        let rate = if duration >= 1000.0 && !stalled {
            bytes.saturating_sub(start_bytes) as f64 * 1000.0 / duration
        } else {
            0.0
        };
        let eta = match remaining {
            Some(r) if r > 0 && rate > 0.0 && duration >= 3000.0 => Some((r as f64 / rate).ceil() as u64),
            _ => None,
        };
        self.last = SpeedSnapshot { speed_bps: rate.max(0.0).round() as u64, eta_seconds: eta };
        self.last
    }
}

impl Default for SmoothedSpeedEstimator {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------- 单请求传输（download.ts receive()） ----------------

struct WaitArgs<'a> {
    budget: &'a CooldownBudget,
    alternatives: &'a [String],
    on_wait: Option<&'a (dyn Fn(&WaitState, bool) + Sync)>,
}

static CONTENT_RANGE: OnceLock<regex::Regex> = OnceLock::new();
fn content_range_re() -> &'static regex::Regex {
    CONTENT_RANGE.get_or_init(|| regex::Regex::new(r"^bytes (\d+)-(\d+)/(\d+)$").unwrap())
}

/// 响应头阶段看门狗：有备用源时 2.5s、否则 30s（对应原版
/// transferTimeouts.alternativeHeadersMs / undici headersTimeout）。
/// 超时后工作线程自行收尾（有界工作，最多 10 跳）。
fn open_stream_bounded(
    url: &str,
    headers: &[(String, String)],
    cancel: Option<Arc<AtomicBool>>,
    timeout_ms: u64,
) -> Result<reqwest::blocking::Response, XferErr> {
    let (tx, rx) = std::sync::mpsc::channel::<Result<reqwest::blocking::Response, String>>();
    let url = url.to_string();
    let headers = headers.to_vec();
    let cancel_for_thread = cancel.clone();
    std::thread::spawn(move || {
        let _ = tx.send(net::open_stream(&url, &headers, cancel_for_thread.as_deref()));
    });
    match rx.recv_timeout(Duration::from_millis(timeout_ms)) {
        Ok(r) => r.map_err(XferErr::from_str_msg),
        Err(_) => Err(XferErr::Network("下载网络停滞".to_string())),
    }
}

/// 对应原版 download.ts `receive()`：一个请求独占一个限速槽位、私有文件句柄。
/// 返回已写入字节数。原版的 Range 分片参数不适用（未移植 segmented）。
fn receive(
    url: &str,
    temporary: &Path,
    expected: &Integrity,
    rec: &TaskRecord,
    progress: Option<&(dyn Fn(u64, u64, u64) + Sync)>,
    pool: Option<&SourcePoolRef>,
    observation_url: &str,
    has_alternative: bool,
    wait: Option<&WaitArgs<'_>>,
) -> Result<u64, XferErr> {
    let result = receive_inner(url, temporary, expected, rec, progress, pool, observation_url, has_alternative, wait);
    // 对应原版 catch：取消/网络/HTTP/内容错误都记为源失败（磁盘错误除外）
    if let Err(e) = &result {
        if !matches!(e, XferErr::Disk(_)) {
            if let Some(p) = pool {
                p.lock().unwrap().fail(observation_url);
            }
        }
    }
    result
}

fn receive_inner(
    url: &str,
    temporary: &Path,
    expected: &Integrity,
    rec: &TaskRecord,
    progress: Option<&(dyn Fn(u64, u64, u64) + Sync)>,
    pool: Option<&SourcePoolRef>,
    observation_url: &str,
    has_alternative: bool,
    wait: Option<&WaitArgs<'_>>,
) -> Result<u64, XferErr> {
    wait_while_paused(rec);
    check_cancel(rec).map_err(|_| XferErr::Cancelled)?;
    let (budget, alternatives, on_wait) = match wait {
        Some(w) => (Some(w.budget), w.alternatives, w.on_wait),
        None => (None, &[] as &[String], None),
    };
    // 跨文件（含并行分片）共享的源限流冷却
    wait_for_server_cooldown(url, rec, budget, alternatives, on_wait)?;
    let _slot = limiter().acquire(Some(rec)).map_err(XferErr::from_str_msg)?;

    let mut offset: u64 = std::fs::metadata(temporary).map(|m| m.len()).unwrap_or(0);
    // 无哈希时跨源续传无法建立表示同一性（对应原版 offset = 0）
    if !expected.has_hash() {
        offset = 0;
    }
    let required = expected.size;
    if offset > 0 && required == Some(offset) {
        match verify_file(temporary, expected, Some(rec.cancelled.as_ref())) {
            Ok(None) => {
                if let Some(cb) = progress {
                    cb(offset, required.unwrap_or(0), 0);
                }
                return Ok(offset);
            }
            Ok(Some(_)) => {}
            Err(e) => return Err(XferErr::from_str_msg(e)),
        }
    }
    if let Some(req) = required {
        if offset >= req {
            offset = 0;
        }
    }

    let mut headers: Vec<(String, String)> = vec![("accept-encoding".to_string(), "identity".to_string())];
    if offset > 0 {
        headers.push(("Range".to_string(), format!("bytes={offset}-")));
    }
    let header_start = Instant::now();
    let resp = open_stream_bounded(
        url,
        &headers,
        Some(rec.cancelled.clone()),
        if has_alternative { 2_500 } else { 30_000 },
    )?;
    let headers_ms = header_start.elapsed().as_secs_f64() * 1000.0;

    let status = resp.status().as_u16();
    if status == 429 || (status == 503 && resp.headers().contains_key("retry-after")) {
        // 对应原版 429 分支：登记服务器冷却 + 同步报告一次等待状态
        {
            let mut cd = server_cooldowns().lock().unwrap();
            if cd.len() > 128 {
                let now = now_ms();
                cd.retain(|_, until| *until > now);
            }
        }
        let until = retry_after_time(resp.headers().get("retry-after").and_then(|v| v.to_str().ok()), now_ms());
        let resp_url = resp.url().to_string();
        let mut hosts = vec![origin_of(url), origin_of(&resp_url)];
        hosts.dedup();
        {
            let mut cd = server_cooldowns().lock().unwrap();
            for h in hosts {
                let prev = cd.get(&h).copied().unwrap_or(0);
                cd.insert(h, prev.max(until));
            }
        }
        let switching = can_avoid_cooldown(url, alternatives);
        let state = WaitState {
            reason: "rate-limit".to_string(),
            source: origin_of(&resp_url),
            retry_at: until,
            action: if switching { "switching" } else { "waiting" }.to_string(),
            file: None,
        };
        if let Some(cb) = on_wait {
            cb(&state, true);
            cb(&state, false);
        }
    }
    if !(200..300).contains(&status) {
        return Err(XferErr::Http { status, url: url.to_string() });
    }

    let mut total: u64 = required.unwrap_or(0);
    if status == 206 {
        let cr = resp.headers().get("content-range").and_then(|v| v.to_str().ok()).unwrap_or("");
        let parsed = content_range_re().captures(cr).and_then(|c| {
            Some((c[1].parse::<u64>().ok()?, c[2].parse::<u64>().ok()?, c[3].parse::<u64>().ok()?))
        });
        let Some((start, end, tot)) = parsed else {
            return Err(XferErr::Invalid("Content-Range 不正确".to_string()));
        };
        if start != offset || end < start || end >= tot {
            return Err(XferErr::Invalid("Content-Range 不正确".to_string()));
        }
        if let Some(size) = required {
            if tot != size {
                return Err(XferErr::Invalid("响应总大小不符".to_string()));
            }
        }
        total = tot;
    } else {
        // 200：服务器忽略 Range（仅断点续传时可能）→ 从头写（对应原版 offset = 0）
        offset = 0;
        let length = resp
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        if total == 0 && length > 0 {
            total = length;
        }
    }

    let mut file = if offset > 0 {
        std::fs::OpenOptions::new().append(true).open(temporary)
    } else {
        std::fs::OpenOptions::new().write(true).create(true).truncate(true).open(temporary)
    }
    .map_err(|e| XferErr::Disk(e.to_string()))?;

    if let Some(cb) = progress {
        cb(offset, total, 0);
    }

    // 读循环：响应体在独立线程读取，主循环用 recv_timeout 做不活动看门狗
    // （对应原版 15s inactivityMs 中止『下载网络停滞』；暂停/限速期间不计时），
    // 同时让取消/暂停在读取阻塞期间也能即时生效。
    let (tx, rx) = std::sync::mpsc::channel::<Result<Vec<u8>, String>>();
    std::thread::spawn(move || {
        let mut resp = resp;
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            match resp.read(&mut buf) {
                Ok(0) => {
                    let _ = tx.send(Ok(Vec::new()));
                    break;
                }
                Ok(n) => {
                    if tx.send(Ok(buf[..n].to_vec())).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(e.to_string()));
                    break;
                }
            }
        }
    });

    let mut done: u64 = offset;
    let mut transferred: u64 = 0;
    let mut idle_ms: u64 = 0;
    // bodyMs 对应原版 reader.read() 等待时长（排除限速/暂停时间），供源池测速
    let mut body_ms: f64 = 0.0;
    let mut last_event = Instant::now();
    loop {
        let was_paused = rec.paused.load(Ordering::SeqCst);
        wait_while_paused(rec);
        if was_paused {
            last_event = Instant::now();
        }
        check_cancel(rec).map_err(|_| XferErr::Cancelled)?;
        let budget_slice = 15_000u64.saturating_sub(idle_ms).min(250).max(1);
        match rx.recv_timeout(Duration::from_millis(budget_slice)) {
            Ok(Ok(chunk)) => {
                idle_ms = 0;
                if chunk.is_empty() {
                    break; // EOF
                }
                body_ms += last_event.elapsed().as_secs_f64() * 1000.0;
                limiter().consume(chunk.len(), Some(rec)).map_err(XferErr::from_str_msg)?;
                last_event = Instant::now();
                wait_while_paused(rec);
                file.write_all(&chunk).map_err(|_| XferErr::Disk("写入文件失败".to_string()))?;
                done += chunk.len() as u64;
                transferred += chunk.len() as u64;
                if total > 0 && done > total {
                    return Err(XferErr::Invalid("响应数据超出声明大小".to_string()));
                }
                if let Some(cb) = progress {
                    cb(done, total, chunk.len() as u64);
                }
                if total > 0 && done == total {
                    break;
                }
            }
            Ok(Err(e)) => return Err(XferErr::Network(e)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                // 对应原版 watchdog 跳过条件：限速中的任务不计时（暂停已在循环头处理）
                if !limiter().is_throttling() {
                    idle_ms += budget_slice;
                }
                if idle_ms >= 15_000 {
                    return Err(XferErr::Network("下载网络停滞".to_string()));
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Err(XferErr::Network("下载网络停滞".to_string()));
            }
        }
    }
    if total > 0 && done != total {
        return Err(XferErr::Invalid("下载大小不符".to_string()));
    }
    file.sync_data().map_err(|e| XferErr::Disk(e.to_string()))?;
    if !limiter().is_throttling() && rec.paused.load(Ordering::SeqCst) == false {
        if let Some(p) = pool {
            p.lock().unwrap().observe(observation_url, headers_ms, body_ms, transferred);
        }
    }
    Ok(done)
}

// ---------------- 本地复用（download.ts reuse()） ----------------

fn reuse(dest: &Path, expected: &Integrity, roots: &[PathBuf], files: &[PathBuf], rec: &TaskRecord) -> Result<bool, String> {
    if !expected.has_hash() {
        return Ok(false);
    }
    let dest_part = part_path(dest);
    let dest_norm = normalize_path(dest);
    let copy = |file: &Path| -> Result<bool, String> {
        if normalize_path(file) == dest_norm {
            return Ok(false);
        }
        // 绑定文件夹可能离线/只读：仅作为可选缓存，失败静默跳过（取消除外）
        match verify_file(file, expected, Some(rec.cancelled.as_ref())) {
            Ok(None) => {}
            Ok(Some(_)) => return Ok(false),
            Err(e) => {
                check_cancel(rec)?;
                let _ = e;
                return Ok(false);
            }
        }
        if let Err(e) = std::fs::copy(file, &dest_part) {
            if e.kind() == std::io::ErrorKind::NotFound {
                return Ok(false);
            }
            return Err(e.to_string());
        }
        if verify_file(&dest_part, expected, Some(rec.cancelled.as_ref()))?.is_some() {
            let _ = std::fs::remove_file(&dest_part);
            return Ok(false);
        }
        check_cancel(rec)?;
        std::fs::rename(&dest_part, dest).map_err(|e| e.to_string())?;
        Ok(true)
    };
    // 复用候选文件去重（对应原版 new Set(files)）
    let mut seen: HashSet<String> = HashSet::new();
    for file in files {
        if !seen.insert(normalize_path(file)) {
            continue;
        }
        if copy(file)? {
            return Ok(true);
        }
    }
    // 目录扫描：深度 8、访问上限 10000、同扩展名（对应原版 reuse() 的 roots 遍历）
    let own = dest.parent().map(normalize_path).unwrap_or_default();
    let extension = dest.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let mut queue: VecDeque<(PathBuf, usize)> = roots
        .iter()
        .map(|r| (std::path::absolute(r).unwrap_or_else(|_| r.clone()), 0usize))
        .collect();
    let mut visited = 0usize;
    while let Some((dir, depth)) = queue.pop_front() {
        if visited >= 10000 {
            break;
        }
        visited += 1;
        if normalize_path(&dir) == own {
            continue;
        }
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_dir() && depth < 8 {
                queue.push_back((path.clone(), depth + 1));
            }
            if !ft.is_file() {
                continue;
            }
            let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            if ext != extension {
                continue;
            }
            if copy(&path)? {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

// ---------------- 单文件下载（download.ts downloadFile） ----------------

/// 进度回调：(done, total, 本次线上字节 wire)。
pub type ProgressSink<'a> = &'a (dyn Fn(u64, u64, u64) + Sync);

/// 对应原版 downloadFile 的可选完整性/复用/重试参数。
pub struct DownloadFileOpts<'a> {
    pub sha1: Option<&'a str>,
    pub sha512: Option<&'a str>,
    pub sha256: Option<&'a str>,
    pub size: Option<u64>,
    /// 备用地址（原版 task.urls / alternatives）
    pub alternatives: &'a [String],
    pub reuse_dirs: &'a [PathBuf],
    pub reuse_files: &'a [PathBuf],
    pub mirror: MirrorPref,
    /// 0 = 默认 4 次
    pub max_attempts: u32,
    pub source_pool: Option<&'a SourcePoolRef>,
    pub progress: Option<ProgressSink<'a>>,
    /// 限流等待状态回调（active=false 表示等待结束）
    pub on_wait: Option<&'a (dyn Fn(&WaitState, bool) + Sync)>,
}

impl Default for DownloadFileOpts<'_> {
    fn default() -> Self {
        Self {
            sha1: None,
            sha512: None,
            sha256: None,
            size: None,
            alternatives: &[],
            reuse_dirs: &[],
            reuse_files: &[],
            mirror: MirrorPref::Official,
            max_attempts: 4,
            source_pool: None,
            progress: None,
            on_wait: None,
        }
    }
}

/// 对应原版 download.ts `downloadFile()`：带重试、镜像回退、哈希校验、
/// .part 临时文件 + 原子改名。不发事件（与原版一致，进度由回调交给调用方）。
pub fn download_file(
    _app: &tauri::AppHandle,
    rec: &Arc<TaskRecord>,
    url: &str,
    dest: &Path,
    opts: &DownloadFileOpts<'_>,
) -> Result<(), String> {
    with_file_job(dest, Some(rec.as_ref()), || download_file_inner(rec, url, dest, opts))
}

fn download_file_inner(rec: &Arc<TaskRecord>, url: &str, dest: &Path, opts: &DownloadFileOpts<'_>) -> Result<(), String> {
    check_cancel(rec)?;
    let expected = Integrity {
        sha1: opts.sha1.map(str::to_string),
        sha512: opts.sha512.map(str::to_string),
        sha256: opts.sha256.map(str::to_string),
        size: opts.size,
    };
    let temporary = part_path(dest);
    let attempts = if opts.max_attempts == 0 { 4 } else { opts.max_attempts.clamp(1, 4) };
    let budget = CooldownBudget::new(rec.clone());
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let result: Result<(), String> = (|| {
        // 1. 目标已完整 → 直接完成
        if verify_file(dest, &expected, Some(rec.cancelled.as_ref()))?.is_none() {
            let size = std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0);
            if let Some(cb) = opts.progress {
                cb(size, size, 0);
            }
            return Ok(());
        }
        // 2. .part 已完整 → 采纳
        if expected.has_hash() && verify_file(&temporary, &expected, Some(rec.cancelled.as_ref()))?.is_none() {
            check_cancel(rec)?;
            std::fs::rename(&temporary, dest).map_err(|e| e.to_string())?;
            return Ok(());
        }
        // 3. 其他实例/文件夹的同哈希文件复用
        if reuse(dest, &expected, opts.reuse_dirs, opts.reuse_files, rec)? {
            let size = std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0);
            if let Some(cb) = opts.progress {
                cb(size, size, 0);
            }
            return Ok(());
        }
        // 4.（磁盘空间预检未移植，见文件头差异说明）
        let mut supplied: Vec<String> = Vec::with_capacity(1 + opts.alternatives.len());
        supplied.push(url.to_string());
        supplied.extend(opts.alternatives.iter().cloned());
        let supplied = download_candidates(&supplied, opts.mirror);
        let mut candidates = match opts.source_pool {
            Some(pool) => pool.lock().unwrap().order(&supplied, expected.size.unwrap_or(64 * 1024)),
            None => supplied,
        };
        // 冷却中的源排后（稳定排序，对应原版 sort 键）
        candidates.sort_by_key(|u| host_cooling(u));
        let mut last_error: XferErr = XferErr::Network("没有下载地址".to_string());
        for (index, source) in candidates.iter().enumerate() {
            let source = source.as_str();
            let fallback = index + 1 < candidates.len();
            let wait_args = WaitArgs {
                budget: &budget,
                alternatives: &candidates[index + 1..],
                on_wait: opts.on_wait,
            };
            for attempt in 0..attempts {
                check_cancel(rec)?;
                match receive(source, &temporary, &expected, rec, opts.progress, opts.source_pool, source, fallback, Some(&wait_args)) {
                    Ok(bytes) => {
                        if let Some(reason) = verify_file(&temporary, &expected, Some(rec.cancelled.as_ref()))? {
                            if let Some(pool) = opts.source_pool {
                                pool.lock().unwrap().fail(source);
                            }
                            let _ = std::fs::remove_file(&temporary);
                            last_error = XferErr::Invalid(reason);
                            break;
                        }
                        check_cancel(rec)?;
                        std::fs::rename(&temporary, dest).map_err(|e| e.to_string())?;
                        note_host_success(source);
                        if let Some(cb) = opts.progress {
                            cb(bytes, bytes, 0);
                        }
                        return Ok(());
                    }
                    Err(e) => {
                        if rec.cancelled.load(Ordering::SeqCst) {
                            return Err("已取消".to_string());
                        }
                        last_error = e;
                        match &last_error {
                            XferErr::RateLimit { .. } => break,
                            XferErr::Invalid(_) => {
                                let _ = std::fs::remove_file(&temporary);
                                break;
                            }
                            XferErr::Http { status, .. } if classify_http_status(*status) != HttpFailureKind::Transient => break,
                            _ => {
                                note_host_failure(source);
                                // 有健康替代源时先切换，不重复撞同一个连接
                                if fallback {
                                    break;
                                }
                                if attempt + 1 < attempts {
                                    abortable_delay(100u64 << attempt, rec)?;
                                }
                            }
                        }
                    }
                }
            }
        }
        // 终态错误组合（对应原版尾部 throw：下载失败：msg\n文件：…\n地址：origin+path）
        let mut address = last_error.url().unwrap_or(url).to_string();
        if let Ok(u) = Url::parse(&address) {
            address = format!("{}{}", u.origin().ascii_serialization(), u.path());
        }
        Err(format!(
            "下载失败：{}\n文件：{}\n地址：{}",
            last_error.message(),
            basename(dest),
            address
        ))
    })();
    // 无论成败（含取消）都清理 .part（对应原版 finally 的 rm force）
    let _ = std::fs::remove_file(&temporary);
    result
}

// ---------------- 批量并行下载（download.ts downloadAll） ----------------

/// 对应原版 `DownloadTask`。
#[derive(Debug, Clone, Default)]
pub struct DownloadTask {
    pub label: Option<String>,
    pub url: String,
    pub urls: Vec<String>,
    pub dest: PathBuf,
    pub sha1: Option<String>,
    pub sha512: Option<String>,
    pub sha256: Option<String>,
    pub size: Option<u64>,
    pub reuse_dirs: Vec<PathBuf>,
    pub reuse_files: Vec<PathBuf>,
}

/// 批量进度的 text 文案风格（对应 versions.ts / modpacks.ts 各调用点的文案）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchTextKind {
    /// `下载依赖库 {d}/{t}`
    Libraries,
    /// shared/assetDownloadStatus.ts 文案（资源文件）
    Assets,
    /// `下载游戏本体 {fmtMB(bytesDone)}`
    ClientJar,
    /// modpacks.ts 整合包文案
    Modpack,
}

pub struct BatchOptions {
    /// ProgressEvent.stage
    pub stage: String,
    pub kind: BatchTextKind,
    pub mirror: MirrorPref,
    /// ProgressEvent.source（默认按 mirror 生成）
    pub source_text: String,
    /// None = settings.downloadThreads（经 limiter 读取）
    pub concurrency: Option<usize>,
    /// Some((a,b)) 时 overall = a + (b-a)*fraction（对应 weighted ranges 的直接用法）
    pub overall_range: Option<(f64, f64)>,
    /// manualFiles 透传（modpack 手动补文件，对应原版 emit({...event, manualFiles})）
    pub manual_files: Option<Value>,
}

impl BatchOptions {
    pub fn new(stage: &str, mirror: MirrorPref, kind: BatchTextKind) -> Self {
        Self {
            stage: stage.to_string(),
            kind,
            mirror,
            source_text: source_text(mirror).to_string(),
            concurrency: None,
            overall_range: None,
            manual_files: None,
        }
    }
}

/// 对应原版 shared/assetDownloadStatus.ts `assetDownloadStatus()`（文案逐字）。
pub fn asset_download_status(done: u64, total: u64, active_files: &[String], paused: bool, waits: &[WaitState], now: u64) -> String {
    let base = format!("下载资源文件 {done}/{total}");
    if done >= total || paused {
        return base;
    }
    if waits.iter().any(|w| w.action == "switching") {
        return format!("{base} · 下载源限流，切换备用来源");
    }
    let waiting: Vec<u64> = waits.iter().filter(|w| w.action == "waiting" && w.retry_at > now).map(|w| w.retry_at).collect();
    if !waiting.is_empty() {
        let min_retry = waiting.iter().copied().min().unwrap_or(now);
        let seconds = min_retry.saturating_sub(now).div_ceil(1000);
        return format!("{base} · 下载源限流，{seconds} 秒后重试");
    }
    if total > done && total - done <= 2 && !active_files.is_empty() {
        let names: Vec<String> = active_files
            .iter()
            .take(2)
            .map(|name| {
                let cleaned: String = name
                    .chars()
                    .map(|c| if c == '\r' || c == '\n' || c == '\t' { ' ' } else { c })
                    .collect();
                cleaned.chars().take(180).collect()
            })
            .collect();
        return format!("{base} · 正在处理 {}", names.join("、"));
    }
    base
}

fn batch_text(kind: &BatchTextKind, completed: u64, total_files: u64, bytes_done: u64, bytes_total: Option<u64>, active: &[String], paused: bool, waits: &[WaitState]) -> String {
    match kind {
        BatchTextKind::Libraries => format!("下载依赖库 {}/{}", completed, total_files),
        BatchTextKind::Assets => asset_download_status(completed, total_files, active, paused, waits, now_ms()),
        BatchTextKind::ClientJar => format!("下载游戏本体 {}", fmt_mb(bytes_done)),
        BatchTextKind::Modpack => match bytes_total {
            Some(bt) => {
                let suffix = if !active.is_empty() && active.len() <= 2 {
                    format!(" · {}", active.join("、"))
                } else {
                    String::new()
                };
                format!("下载整合包文件 {}/{}（{}/{}）{}", completed, total_files, fmt_mb(bytes_done), fmt_mb(bt), suffix)
            }
            None => format!("下载整合包文件 {}/{}", completed, total_files),
        },
    }
}

struct BatchShared {
    tracker: Mutex<DownloadProgressTracker>,
    estimator: Mutex<SmoothedSpeedEstimator>,
    network_bytes: AtomicU64,
    active: Mutex<HashMap<usize, String>>,
    waits: Mutex<Vec<(WaitState, usize)>>,
    abort: AtomicBool,
    done: AtomicBool,
    first_error: Mutex<Option<String>>,
}

impl BatchShared {
    fn new(tasks: &[DownloadTask]) -> Self {
        let mut tracker = DownloadProgressTracker::new();
        for t in tasks {
            let _ = tracker.add(t.size);
        }
        tracker.seal();
        Self {
            tracker: Mutex::new(tracker),
            estimator: Mutex::new(SmoothedSpeedEstimator::new()),
            network_bytes: AtomicU64::new(0),
            active: Mutex::new(HashMap::new()),
            waits: Mutex::new(Vec::new()),
            abort: AtomicBool::new(false),
            done: AtomicBool::new(false),
            first_error: Mutex::new(None),
        }
    }
}

struct BatchCtx<'a> {
    app: &'a tauri::AppHandle,
    rec: &'a Arc<TaskRecord>,
    tasks: &'a [DownloadTask],
    stage: &'a str,
    kind: &'a BatchTextKind,
    source_text: &'a str,
    mirror: MirrorPref,
    overall_range: Option<(f64, f64)>,
    manual_files: Option<&'a Value>,
    shared: &'a BatchShared,
    pool: &'a SourcePoolRef,
}

impl BatchCtx<'_> {
    fn emit(&self, fraction: Option<f64>, text: &str, speed: SpeedSnapshot, bytes_done: u64, bytes_total: Option<u64>, indeterminate: bool) {
        let mut ev = serde_json::Map::new();
        ev.insert("taskId".to_string(), Value::String(self.rec.id.clone()));
        ev.insert("taskTitle".to_string(), Value::String(self.rec.title.clone()));
        ev.insert("stage".to_string(), Value::String(self.stage.to_string()));
        ev.insert("progress".to_string(), json!(fraction.unwrap_or(0.0)));
        ev.insert("text".to_string(), Value::String(text.to_string()));
        ev.insert("speed".to_string(), json!(speed.speed_bps));
        if let Some(eta) = speed.eta_seconds {
            ev.insert("etaSeconds".to_string(), json!(eta));
        }
        ev.insert("bytesDone".to_string(), json!(bytes_done));
        if let Some(bt) = bytes_total {
            ev.insert("bytesTotal".to_string(), json!(bt));
        }
        ev.insert("indeterminate".to_string(), json!(indeterminate));
        ev.insert("source".to_string(), Value::String(self.source_text.to_string()));
        if let Some((r0, r1)) = self.overall_range {
            ev.insert("overall".to_string(), json!(r0 + (r1 - r0) * fraction.unwrap_or(0.0)));
        }
        if let Some(mf) = self.manual_files {
            ev.insert("manualFiles".to_string(), mf.clone());
        }
        crate::tasks::emit_progress(self.app, Value::Object(ev));
    }

    fn report(&self) {
        let snap = self.shared.tracker.lock().unwrap().snapshot();
        let paused = self.rec.paused.load(Ordering::SeqCst);
        let remaining = snap.bytes_total.map(|t| t.saturating_sub(snap.bytes_done));
        let speed = {
            let mut est = self.shared.estimator.lock().unwrap();
            est.sample(self.shared.network_bytes.load(Ordering::SeqCst), remaining, mono_ms(), paused)
        };
        let total_files = self.tasks.len() as u64;
        let active: Vec<String> = self.shared.active.lock().unwrap().values().cloned().collect();
        let waits: Vec<WaitState> = {
            let w = self.shared.waits.lock().unwrap();
            w.iter()
                .map(|(state, index)| {
                    let mut s = state.clone();
                    if s.file.is_none() {
                        s.file = Some(
                            self.tasks
                                .get(*index)
                                .map(|t| t.label.clone().unwrap_or_else(|| basename(&t.dest)))
                                .unwrap_or_default(),
                        );
                    }
                    s
                })
                .collect()
        };
        let text = batch_text(self.kind, snap.completed_files, total_files, snap.bytes_done, snap.bytes_total, &active, paused, &waits);
        self.emit(snap.fraction, &text, speed, snap.bytes_done, snap.bytes_total, snap.indeterminate);
    }

    /// 空任务列表的完成事件（对应原版 `progress?.(0,0,0,{completedFiles:0,…,fraction:1,…})`）。
    fn emit_empty(&self) {
        let text = batch_text(self.kind, 0, 0, 0, Some(0), &[], false, &[]);
        self.emit(Some(1.0), &text, SpeedSnapshot { speed_bps: 0, eta_seconds: None }, 0, Some(0), false);
    }
}

/// 对应原版 download.ts `downloadAll()`：并行 worker 池 + 大文件优先调度 +
/// 全局限速 + 250ms 节流汇报。任何文件失败 → 取消兄弟 worker（firstError）；
/// 任务取消 → 返回 Err("已取消")，由调用方发 event:taskDone cancelled。
pub fn download_all(app: &tauri::AppHandle, rec: &Arc<TaskRecord>, tasks: Vec<DownloadTask>, opts: BatchOptions) -> Result<(), String> {
    init_limiter_from_settings(app);
    check_cancel(rec)?;
    let concurrency = opts.concurrency.unwrap_or_else(|| limiter().max_concurrent());
    let workers = tasks.len().min(concurrency.max(1));
    let pool: SourcePoolRef = Arc::new(Mutex::new(DownloadSourcePool::new()));
    let shared = BatchShared::new(&tasks);
    let ctx = BatchCtx {
        app,
        rec,
        tasks: &tasks,
        stage: &opts.stage,
        kind: &opts.kind,
        source_text: &opts.source_text,
        mirror: opts.mirror,
        overall_range: opts.overall_range,
        manual_files: opts.manual_files.as_ref(),
        shared: &shared,
        pool: &pool,
    };
    if tasks.is_empty() {
        ctx.emit_empty();
        return Ok(());
    }
    // 初始汇报（对应原版进入 try 前的 report()）
    ctx.report();

    // 大文件优先；每 4 个 worker 预留 1 个给小文件（对应原版 order/take）
    let task_size = |t: &DownloadTask| t.size.unwrap_or(0);
    let mut order: Vec<usize> = (0..tasks.len()).collect();
    order.sort_by(|&a, &b| task_size(&tasks[b]).cmp(&task_size(&tasks[a])));
    let mixed = workers >= 4 && order.iter().any(|&i| task_size(&tasks[i]) >= 1024 * 1024);
    let large: Vec<usize> = if mixed {
        order.iter().copied().filter(|&i| task_size(&tasks[i]) >= 1024 * 1024).collect()
    } else {
        order.clone()
    };
    let mut small: Vec<usize> = if mixed {
        order.iter().copied().filter(|&i| task_size(&tasks[i]) < 1024 * 1024).collect()
    } else {
        Vec::new()
    };
    small.reverse();
    let cursor = Mutex::new((0usize, 0usize));
    let take = |worker: usize| -> Option<usize> {
        let mut c = cursor.lock().unwrap();
        if mixed && worker % 4 == 3 && c.1 < small.len() {
            let i = small[c.1];
            c.1 += 1;
            return Some(i);
        }
        if c.0 < large.len() {
            let i = large[c.0];
            c.0 += 1;
            return Some(i);
        }
        if c.1 < small.len() {
            let i = small[c.1];
            c.1 += 1;
            return Some(i);
        }
        None
    };

    std::thread::scope(|scope| {
        // 250ms 汇报定时器（对应原版 setInterval(report, 250)）
        let reporter = scope.spawn(|| loop {
            std::thread::sleep(Duration::from_millis(250));
            if ctx.shared.done.load(Ordering::SeqCst) {
                break;
            }
            ctx.report();
        });
        let mut handles = Vec::new();
        for worker in 0..workers {
            let ctx = &ctx;
            let take = &take;
            handles.push(scope.spawn(move || {
                loop {
                    if ctx.shared.abort.load(Ordering::SeqCst) || ctx.rec.cancelled.load(Ordering::SeqCst) {
                        return;
                    }
                    wait_while_paused(ctx.rec);
                    let Some(index) = take(worker) else { return };
                    let task = &ctx.tasks[index];
                    let label = task.label.clone().unwrap_or_else(|| basename(&task.dest));
                    ctx.shared.active.lock().unwrap().insert(index, label);
                    let progress = move |done: u64, total: u64, wire: u64| {
                        // wire 字节只计网络传输（对应原版 networkBytes += wire）
                        ctx.shared.network_bytes.fetch_add(wire, Ordering::SeqCst);
                        ctx.shared.tracker.lock().unwrap().record(index, done, total);
                    };
                    let on_wait = move |state: &WaitState, active: bool| {
                        {
                            let mut w = ctx.shared.waits.lock().unwrap();
                            if active {
                                w.push((state.clone(), index));
                            } else {
                                w.retain(|(s, i)| !(*i == index && s.source == state.source && s.action == state.action));
                            }
                        }
                        ctx.report();
                    };
                    let progress_ref: &(dyn Fn(u64, u64, u64) + Sync) = &progress;
                    let on_wait_ref: &(dyn Fn(&WaitState, bool) + Sync) = &on_wait;
                    let fopts = DownloadFileOpts {
                        sha1: task.sha1.as_deref(),
                        sha512: task.sha512.as_deref(),
                        sha256: task.sha256.as_deref(),
                        size: task.size,
                        alternatives: &task.urls,
                        reuse_dirs: &task.reuse_dirs,
                        reuse_files: &task.reuse_files,
                        mirror: ctx.mirror,
                        max_attempts: 4,
                        source_pool: Some(ctx.pool),
                        progress: Some(progress_ref),
                        on_wait: Some(on_wait_ref),
                    };
                    match download_file(ctx.app, ctx.rec, &task.url, &task.dest, &fopts) {
                        Ok(()) => {
                            let size = std::fs::metadata(&task.dest).map(|m| m.len()).unwrap_or(0);
                            ctx.shared.tracker.lock().unwrap().record_complete(index, size);
                            ctx.shared.active.lock().unwrap().remove(&index);
                        }
                        Err(e) => {
                            ctx.shared.active.lock().unwrap().remove(&index);
                            let cancelled = ctx.rec.cancelled.load(Ordering::SeqCst);
                            // 带标签的资源文件失败加前缀（对应原版 `资源文件 ${label}：`）
                            let failure = if task.label.is_some() && !ctx.shared.abort.load(Ordering::SeqCst) && !cancelled {
                                format!("资源文件 {}：{}", task.label.as_deref().unwrap_or(""), e)
                            } else {
                                e
                            };
                            let mut fe = ctx.shared.first_error.lock().unwrap();
                            if fe.is_none() {
                                *fe = Some(failure);
                            }
                            drop(fe);
                            ctx.shared.abort.store(true, Ordering::SeqCst);
                            return;
                        }
                    }
                }
            }));
        }
        for h in handles {
            let _ = h.join();
        }
        ctx.shared.done.store(true, Ordering::SeqCst);
        let _ = reporter.join();
    });

    // 对应原版收尾顺序：throwIfAborted → firstError → report()
    if rec.cancelled.load(Ordering::SeqCst) {
        return Err("已取消".to_string());
    }
    if let Some(err) = shared.first_error.lock().unwrap().clone() {
        return Err(err);
    }
    ctx.report();
    Ok(())
}

// ---------------- 进度事件整形（parallelProgress.ts / progress.ts） ----------------

/// 对应原版 parallelProgress.ts 的 `Lane`。
#[derive(Debug, Clone)]
pub struct Lane {
    pub id: String,
    pub label: String,
    pub weight: f64,
}

/// 对应原版 parallelProgress.ts `ParallelProgress`：独立泳道加权合成为一个总进度，
/// update 按 100ms 节流发布，done/waiting 立即发布。直接推 `event:progress`
/// （附 taskId/taskTitle；extra 可透传 manualFiles 等）。
pub struct ParallelProgress {
    app: tauri::AppHandle,
    task_id: String,
    task_title: String,
    lanes: Vec<Lane>,
    events: HashMap<String, Value>,
    completed: HashSet<String>,
    waiting_text: HashMap<String, String>,
    published_at: Option<Instant>,
    text: String,
    range: (f64, f64),
    extra: Option<Value>,
}

impl ParallelProgress {
    pub fn new(app: &tauri::AppHandle, rec: &Arc<TaskRecord>, lanes: &[(&str, &str, f64)], text: &str, range: (f64, f64)) -> Self {
        Self {
            app: app.clone(),
            task_id: rec.id.clone(),
            task_title: rec.title.clone(),
            lanes: lanes
                .iter()
                .map(|(id, label, weight)| Lane { id: id.to_string(), label: label.to_string(), weight: *weight })
                .collect(),
            events: HashMap::new(),
            completed: HashSet::new(),
            waiting_text: HashMap::new(),
            published_at: None,
            text: text.to_string(),
            range,
            extra: None,
        }
    }

    /// 额外字段（如 manualFiles）合并进每次发布的事件。
    pub fn set_extra(&mut self, extra: Value) {
        self.extra = Some(extra);
    }

    pub fn update(&mut self, id: &str, event: Value) {
        self.waiting_text.remove(id);
        let previous = self.events.get(id).cloned();
        let children_changed = parallel_children_changed(previous.as_ref(), &event);
        let stage_changed = previous.as_ref().and_then(|p| p.get("stage")) != event.get("stage");
        self.events.insert(id.to_string(), event);
        let elapsed_ok = self.published_at.map(|t| t.elapsed().as_millis() >= 100).unwrap_or(true);
        if previous.is_none() || stage_changed || children_changed || elapsed_ok {
            self.publish();
        }
    }

    pub fn done(&mut self, id: &str) {
        self.waiting_text.remove(id);
        self.completed.insert(id.to_string());
        self.publish();
    }

    pub fn waiting(&mut self, id: &str, text: &str) {
        self.waiting_text.insert(id.to_string(), text.to_string());
        self.publish();
    }

    fn publish(&mut self) {
        self.published_at = Some(Instant::now());
        let mut stages: Vec<Value> = Vec::new();
        let mut weighted = 0.0f64;
        let mut speed = 0.0f64;
        for lane in &self.lanes {
            let event = self.events.get(&lane.id);
            let done = self.completed.contains(&lane.id);
            let raw = event
                .map(|e| {
                    e.get("overall")
                        .and_then(|v| v.as_f64())
                        .or_else(|| e.get("progress").and_then(|v| v.as_f64()))
                        .unwrap_or(0.0)
                })
                .unwrap_or(0.0);
            let progress = if done { 1.0 } else { 0.999f64.min(clamp01(raw)) };
            weighted += lane.weight * progress;
            if !done {
                if let Some(sp) = event.and_then(|e| e.get("speed")).and_then(|v| v.as_f64()) {
                    if sp.is_finite() {
                        speed += sp.max(0.0);
                    }
                }
            }
            let children = if done {
                None
            } else {
                event
                    .and_then(|e| e.get("parallelStages"))
                    .and_then(|v| v.as_array())
                    .filter(|a| !a.is_empty())
                    .cloned()
            };
            match children {
                Some(children) => {
                    for mut child in children {
                        if let Some(map) = child.as_object_mut() {
                            let cid = map.get("id").and_then(|v| v.as_str()).unwrap_or_default().to_string();
                            map.insert("id".to_string(), Value::String(format!("{}/{}", lane.id, cid)));
                        }
                        stages.push(child);
                    }
                }
                None => {
                    let ev_text = event.and_then(|e| e.get("text")).and_then(|v| v.as_str()).map(str::to_string);
                    let text = if done {
                        ev_text.unwrap_or_else(|| "已就绪".to_string())
                    } else {
                        self.waiting_text.get(&lane.id).cloned().or(ev_text).unwrap_or_else(|| "等待准备".to_string())
                    };
                    let mut stage = serde_json::Map::new();
                    stage.insert("id".to_string(), Value::String(lane.id.clone()));
                    if let Some(st) = event.and_then(|e| e.get("stage")).filter(|v| !v.is_null()) {
                        stage.insert("stage".to_string(), st.clone());
                    }
                    stage.insert("label".to_string(), Value::String(lane.label.clone()));
                    stage.insert("text".to_string(), Value::String(text));
                    stage.insert("progress".to_string(), json!(progress));
                    stage.insert(
                        "state".to_string(),
                        Value::String(if done { "done" } else if event.is_some() { "running" } else { "waiting" }.to_string()),
                    );
                    if !done {
                        if let Some(sp) = event.and_then(|e| e.get("speed")).filter(|v| !v.is_null()) {
                            stage.insert("speed".to_string(), sp.clone());
                        }
                        let ind = event.and_then(|e| e.get("indeterminate")).and_then(|v| v.as_bool()).unwrap_or(false);
                        stage.insert("indeterminate".to_string(), json!(ind));
                    }
                    stages.push(Value::Object(stage));
                }
            }
        }
        let total_weight: f64 = self.lanes.iter().map(|l| l.weight).sum();
        let progress = if total_weight > 0.0 {
            self.range.0 + (self.range.1 - self.range.0) * weighted / total_weight
        } else {
            self.range.1
        };
        let mut event = serde_json::Map::new();
        event.insert("stage".to_string(), Value::String("parallel".to_string()));
        event.insert("progress".to_string(), json!(progress));
        event.insert("overall".to_string(), json!(progress));
        event.insert("text".to_string(), Value::String(self.text.clone()));
        if speed > 0.0 {
            event.insert("speed".to_string(), json!(speed));
        }
        event.insert("parallelStages".to_string(), Value::Array(stages));
        event.insert("taskId".to_string(), Value::String(self.task_id.clone()));
        event.insert("taskTitle".to_string(), Value::String(self.task_title.clone()));
        if let Some(extra) = &self.extra {
            if let Some(ex) = extra.as_object() {
                for (k, v) in ex {
                    event.insert(k.clone(), v.clone());
                }
            }
        }
        crate::tasks::emit_progress(&self.app, Value::Object(event));
    }
}

fn parallel_children_changed(previous: Option<&Value>, next: &Value) -> bool {
    let plen = previous
        .and_then(|p| p.get("parallelStages"))
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    let nlen = next.get("parallelStages").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
    if plen != nlen {
        return true;
    }
    match (
        previous.and_then(|p| p.get("parallelStages")).and_then(|v| v.as_array()),
        next.get("parallelStages").and_then(|v| v.as_array()),
    ) {
        (Some(pa), Some(na)) => na.iter().enumerate().any(|(index, child)| match pa.get(index) {
            Some(before) => {
                before.get("id") != child.get("id")
                    || before.get("stage") != child.get("stage")
                    || before.get("state") != child.get("state")
                    || before.get("indeterminate") != child.get("indeterminate")
            }
            None => true,
        }),
        _ => false,
    }
}

pub fn clamp01(v: f64) -> f64 {
    v.max(0.0).min(1.0)
}

/// 对应原版 progress.ts `VERSION_INSTALL_STAGE_RANGES`。
pub fn version_install_stage_range(stage: &str) -> Option<(f64, f64)> {
    match stage {
        "version-json" => Some((0.0, 0.04)),
        "libraries" => Some((0.04, 0.38)),
        "client" => Some((0.38, 0.55)),
        "assets" => Some((0.55, 0.82)),
        "loader" => Some((0.82, 0.96)),
        "loader-dependencies" => Some((0.905, 0.94)),
        "loader-process" => Some((0.94, 0.95)),
        "fabric-api" => Some((0.96, 0.995)),
        "done" => Some((1.0, 1.0)),
        _ => None,
    }
}

/// 对应原版 progress.ts `createWeightedProgressEmit()`：把阶段进度映射为任务总进度，
/// overall 只增不减。对构造好的 ProgressEvent（serde_json::Value）调用 [`WeightedProgress::apply`]。
pub struct WeightedProgress {
    last: f64,
}

impl WeightedProgress {
    pub fn new() -> Self {
        Self { last: 0.0 }
    }

    pub fn apply(&mut self, event: Value) -> Value {
        let stage = event.get("stage").and_then(|v| v.as_str()).unwrap_or("");
        let range = version_install_stage_range(stage);
        let stage_progress = clamp01(event.get("progress").and_then(|v| v.as_f64()).filter(|v| v.is_finite()).unwrap_or(0.0));
        let candidate = match event.get("overall").and_then(|v| v.as_f64()) {
            Some(o) if o.is_finite() => o,
            _ => range.map(|(a, b)| a + (b - a) * stage_progress).unwrap_or(self.last),
        };
        self.last = self.last.max(clamp01(candidate));
        let mut e = event;
        if let Some(map) = e.as_object_mut() {
            map.insert("overall".to_string(), json!(self.last));
        }
        e
    }
}

impl Default for WeightedProgress {
    fn default() -> Self {
        Self::new()
    }
}

/// 对应原版 progress.ts `ProgressEventGuard`：IPC 出口最后防线——overall 只增不减、
/// 清除 NaN/Infinity/负 ETA。
pub struct ProgressGuard {
    last: f64,
}

impl ProgressGuard {
    pub fn new() -> Self {
        Self { last: 0.0 }
    }

    pub fn normalize(&mut self, event: Value) -> Value {
        let raw = event
            .get("overall")
            .and_then(|v| v.as_f64())
            .or_else(|| event.get("progress").and_then(|v| v.as_f64()));
        let candidate = match raw {
            Some(v) if v.is_finite() => clamp01(v),
            _ => self.last,
        };
        self.last = self.last.max(candidate);
        let mut e = event;
        if let Some(map) = e.as_object_mut() {
            map.insert("overall".to_string(), json!(self.last));
            match map.get("etaSeconds").and_then(|v| v.as_f64()) {
                Some(eta) if eta.is_finite() && eta >= 0.0 => {
                    map.insert("etaSeconds".to_string(), json!(eta.round()));
                }
                _ => {
                    map.remove("etaSeconds");
                }
            }
        }
        e
    }
}

impl Default for ProgressGuard {
    fn default() -> Self {
        Self::new()
    }
}

/// 取消错误统一判定（对应原版 tasks.ts `isCancelError`）。
pub fn is_cancel_error(msg: &str) -> bool {
    msg == "已取消" || msg.contains("取消")
}
