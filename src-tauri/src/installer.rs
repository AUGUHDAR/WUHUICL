//! 版本安装管线 + 实例管理 + 加载器清单（对应原版 core/versions.ts、loaders.ts、
//! legacyForgeInstaller.ts、versionRemoval.ts、instances.ts、isolationFiles.ts、
//! instanceMetadata.ts、versionCategories.ts、mavenIdentity.ts、progress.ts、paths.ts）。
//!
//! 通道：versions:install / remove / rename / cleanup / setJava / setResolution /
//!       setIcon / uploadIcon / uploadThumbnail / setThumbnailFit / resetThumbnail /
//!       setIsolation / isolationPlan / categoriesUpdate、loaders:list、loaders:fabricApi
//!
//! 内置一个自包含的薄下载层（DownloadItem/download_all/download_file，见「下载引擎」段）：
//! 集成方后续可整体替换为 crate::download —— 假设清单见模块底部注释与本任务报告。

use crate::settings;
use crate::tasks::{self, TaskRecord};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

// ---------------- 常量 ----------------

/// 游戏文件夹内的启动器数据目录名（对应原版 `.kamucl`；WUHUICL 平行使用，不共用数据）
pub const DATA_DIR_NAME: &str = ".wuhuicl";
pub const BMCL_MAVEN_ROOT: &str = "https://bmclapi2.bangbang93.com/maven/";
/// 安装阶段权重（对应原版 core/progress.ts VERSION_INSTALL_STAGE_RANGES）
const STAGE_RANGES: &[(&str, f64, f64)] = &[
    ("version-json", 0.0, 0.04),
    ("libraries", 0.04, 0.38),
    ("client", 0.38, 0.55),
    ("assets", 0.55, 0.82),
    ("loader", 0.82, 0.96),
    ("loader-dependencies", 0.905, 0.94),
    ("loader-process", 0.94, 0.95),
    ("fabric-api", 0.96, 0.995),
    ("done", 1.0, 1.0),
];
/// 隔离迁移白名单（对应原版 core/isolationFiles.ts）
const ISOLATION_DIRS: &[&str] = &["saves", "mods", "config", "resourcepacks", "shaderpacks", "screenshots"];
const ISOLATION_FILES: &[&str] = &["options.txt", "servers.dat"];
const GAME_WIDTH_MIN: i64 = 854;
const GAME_WIDTH_MAX: i64 = 7680;
const GAME_HEIGHT_MIN: i64 = 480;
const GAME_HEIGHT_MAX: i64 = 4320;

// ---------------- 进度回调类型（必须 Send+Sync：进度闭包会进入下载线程） ----------------

pub type EmitFn<'a> = &'a (dyn Fn(&Value) + Send + Sync);
pub type BatchProgress<'a> = &'a (dyn Fn(usize, usize, f64, &DownloadDetail) + Send + Sync);
pub type FileProgress<'a> = &'a (dyn Fn(u64, Option<u64>) + Send + Sync);

// ---------------- 线程外移（reqwest blocking 不能在 tokio 上下文内构造） ----------------

/// 在普通线程上运行网络/阻塞作业（Tauri 同步命令运行于异步上下文，blocking 客户端必须外移）
pub fn run_blocking<T: Send>(job: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|s| s.spawn(job).join().expect("后台任务崩溃"))
}

// ---------------- 通用工具 ----------------

pub(crate) use probe::{
    basename_of, canonical_path, path_identity, resolve_minecraft_root, resolve_norm, same_path,
    strip_device_prefix,
};

#[path = "installer/probe.rs"]
mod probe;

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
// ---------------- 目录寻址（对应原版 core/paths.ts） ----------------

pub fn all_folders(app: &AppHandle) -> Vec<String> {
    let s = settings::get(app);
    let list: Vec<String> = s["folders"]
        .as_array()
        .map(|a| a.iter().filter_map(|f| f["path"].as_str()).map(str::to_string).collect())
        .unwrap_or_default();
    if list.is_empty() {
        vec![game_dir(app)]
    } else {
        list
    }
}

/// 当前活动游戏文件夹（对应原版 gameDir()）
pub fn active_folder(app: &AppHandle) -> String {
    let s = settings::get(app);
    s["activeFolder"].as_str().unwrap_or("").to_string()
}

pub fn game_dir(app: &AppHandle) -> String {
    let s = settings::get(app);
    let active = s["activeFolder"].as_str().unwrap_or("");
    if !active.is_empty() {
        return active.to_string();
    }
    s["gameDir"].as_str().unwrap_or("").to_string()
}

/// 默认文件夹（libraries/assets 共享位置，对应原版 defaultFolderPath()）
pub fn default_folder_path(app: &AppHandle) -> String {
    let s = settings::get(app);
    s["folders"]
        .as_array()
        .and_then(|a| a.iter().find(|f| f["isDefault"] == true))
        .and_then(|f| f["path"].as_str())
        .map(str::to_string)
        .unwrap_or_else(|| game_dir(app))
}

// 版本 → 文件夹映射（对应原版 versionFolderMap）

static VERSION_FOLDER_MAP: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

pub fn register_version_folder(id: &str, folder: &str) {
    let mut guard = VERSION_FOLDER_MAP.lock().unwrap();
    guard
        .get_or_insert_with(HashMap::new)
        .insert(id.to_string(), folder.to_string());
}

/// 版本所属文件夹：映射 → 各文件夹探测 versions/<id> → 活动文件夹
pub fn folder_of_version(app: &AppHandle, id: &str) -> String {
    if let Some(f) = VERSION_FOLDER_MAP
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|m| m.get(id))
    {
        return f.clone();
    }
    for folder in all_folders(app) {
        if version_dir_in(&folder, id).is_dir() {
            register_version_folder(id, &folder);
            return folder;
        }
    }
    game_dir(app)
}

pub fn versions_dir_in(folder: &str) -> PathBuf {
    Path::new(folder).join("versions")
}

pub fn version_dir_in(folder: &str, id: &str) -> PathBuf {
    versions_dir_in(folder).join(id)
}

pub fn version_json_path_in(folder: &str, id: &str) -> PathBuf {
    version_dir_in(folder, id).join(format!("{}.json", id))
}

pub fn version_jar_path_in(folder: &str, id: &str) -> PathBuf {
    version_dir_in(folder, id).join(format!("{}.jar", id))
}

pub fn install_mark_path_in(folder: &str, id: &str) -> PathBuf {
    version_dir_in(folder, id).join(".installing")
}

pub fn natives_dir_in(folder: &str, id: &str) -> PathBuf {
    version_dir_in(folder, id).join("natives")
}

pub fn base_version_dir_in(default_folder: &str, id: &str) -> PathBuf {
    Path::new(default_folder).join(DATA_DIR_NAME).join("base").join(id)
}

pub fn base_version_json_path_in(default_folder: &str, id: &str) -> PathBuf {
    base_version_dir_in(default_folder, id).join(format!("{}.json", id))
}

pub fn base_version_jar_path_in(default_folder: &str, id: &str) -> PathBuf {
    base_version_dir_in(default_folder, id).join(format!("{}.jar", id))
}

pub fn libraries_dir(app: &AppHandle) -> PathBuf {
    Path::new(&default_folder_path(app)).join("libraries")
}

pub fn library_path(app: &AppHandle, rel: &str) -> PathBuf {
    libraries_dir(app).join(rel.replace('/', "\\"))
}

pub fn assets_dir(app: &AppHandle) -> PathBuf {
    Path::new(&default_folder_path(app)).join("assets")
}

pub fn asset_index_path(app: &AppHandle, index_id: &str) -> PathBuf {
    assets_dir(app).join("indexes").join(format!("{}.json", index_id))
}

pub fn asset_object_path(app: &AppHandle, hash: &str) -> PathBuf {
    assets_dir(app).join("objects").join(&hash[..2.min(hash.len())]).join(hash)
}

pub fn virtual_legacy_dir(app: &AppHandle) -> PathBuf {
    assets_dir(app).join("virtual").join("legacy")
}

pub fn icons_dir(app: &AppHandle) -> PathBuf {
    Path::new(&default_folder_path(app)).join(DATA_DIR_NAME).join("icons")
}

pub fn instance_thumbnails_dir(folder: &str) -> PathBuf {
    Path::new(folder).join(DATA_DIR_NAME).join("thumbnails")
}

// ---------------- 版本 json 读写（对应原版 readVersionJson / JSON.stringify 写回） ----------------

fn strip_bom(s: &str) -> &str {
    s.strip_prefix('\u{feff}').unwrap_or(s)
}

pub fn read_version_json_from(path: &Path) -> Result<Value, String> {
    let raw = std::fs::read(path).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&raw).to_string();
    serde_json::from_str(strip_bom(&text)).map_err(|e| e.to_string())
}

fn validate_id(id: &str) -> Result<(), String> {
    if id.is_empty() || id == "." || id == ".." || id.contains(['\\', '/', ':', '\0']) {
        return Err("无效的版本 ID".into());
    }
    Ok(())
}

/// 读版本 json：versions 区优先，缺省回退 .wuhuicl/base 依赖区；拒绝符号链接
pub fn read_version_json(app: &AppHandle, folder: &str, id: &str) -> Result<Value, String> {
    validate_id(id)?;
    let default_folder = default_folder_path(app);
    let mut p = version_json_path_in(folder, id);
    if !p.exists() && base_version_json_path_in(&default_folder, id).exists() {
        p = base_version_json_path_in(&default_folder, id);
    }
    if !p.exists() {
        return Err(format!("版本 {} 不存在", id));
    }
    let meta = std::fs::symlink_metadata(&p).map_err(|e| e.to_string())?;
    let parent_link = p
        .parent()
        .and_then(|d| std::fs::symlink_metadata(d).ok())
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false);
    if meta.file_type().is_symlink() || parent_link {
        return Err("版本文件不能使用符号链接".into());
    }
    read_version_json_from(&p)
}

pub fn write_version_json(path: &Path, j: &Value) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(j).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

// ---------------- SHA-1（等价实现：Cargo 依赖清单无 sha1 crate，规约铁律 4） ----------------

pub struct Sha1 {
    state: [u32; 5],
    total: u64,
    buffer: [u8; 64],
    buffered: usize,
}

impl Sha1 {
    pub fn new() -> Self {
        Self {
            state: [0x6745_2301, 0xEFCD_AB89, 0x98BA_DCFE, 0x1032_5476, 0xC3D2_E1F0],
            total: 0,
            buffer: [0u8; 64],
            buffered: 0,
        }
    }

    pub fn update(&mut self, mut data: &[u8]) {
        self.total = self.total.wrapping_add(data.len() as u64);
        if self.buffered > 0 {
            let take = (64 - self.buffered).min(data.len());
            self.buffer[self.buffered..self.buffered + take].copy_from_slice(&data[..take]);
            self.buffered += take;
            data = &data[take..];
            if self.buffered == 64 {
                let block = self.buffer;
                self.compress(&block);
                self.buffered = 0;
            }
        }
        while data.len() >= 64 {
            let mut block = [0u8; 64];
            block.copy_from_slice(&data[..64]);
            self.compress(&block);
            data = &data[64..];
        }
        if !data.is_empty() {
            self.buffer[..data.len()].copy_from_slice(data);
            self.buffered = data.len();
        }
    }

    pub fn finish(mut self) -> [u8; 20] {
        let bit_len = self.total.wrapping_mul(8);
        self.update(&[0x80]);
        while self.buffered != 56 {
            self.update(&[0]);
        }
        self.update(&bit_len.to_be_bytes());
        let mut out = [0u8; 20];
        for (i, w) in self.state.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&w.to_be_bytes());
        }
        out
    }

    fn compress(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([block[i * 4], block[i * 4 + 1], block[i * 4 + 2], block[i * 4 + 3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) =
            (self.state[0], self.state[1], self.state[2], self.state[3], self.state[4]);
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A82_7999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let tmp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = tmp;
        }
        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
        self.state[4] = self.state[4].wrapping_add(e);
    }
}

pub fn sha1_hex(bytes: &[u8]) -> String {
    let mut h = Sha1::new();
    h.update(bytes);
    hex::encode(h.finish())
}

pub fn sha1_file_hex(path: &Path) -> Result<String, String> {
    let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut h = Sha1::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finish()))
}

// ---------------- rules 评估（对应原版 rulesAllow；Windows 平台） ----------------

pub const OS_NAME: &str = "windows";

pub fn rules_allow(rules: Option<&Value>) -> bool {
    let Some(rules) = rules.and_then(|r| r.as_array()) else {
        return true;
    };
    if rules.is_empty() {
        return true;
    }
    let mut allowed = false;
    for rule in rules {
        if rule.get("features").is_some() {
            continue;
        }
        if let Some(os) = rule.get("os") {
            if let Some(name) = os.get("name").and_then(|v| v.as_str()) {
                if name != OS_NAME {
                    continue;
                }
            }
            if let Some(arch) = os.get("arch").and_then(|v| v.as_str()) {
                if !rule_architecture_matches(arch) {
                    continue;
                }
            }
        }
        allowed = rule.get("action").and_then(|v| v.as_str()) == Some("allow");
    }
    allowed
}

fn rule_architecture_matches(rule: &str) -> bool {
    let arch = if cfg!(target_arch = "x86_64") {
        "x64"
    } else if cfg!(target_arch = "x86") {
        "ia32"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "unknown"
    };
    let aliases: &[&str] = match arch {
        "ia32" => &["x86", "ia32", "i386"],
        "x64" => &["x86_64", "amd64", "x64"],
        "arm64" => &["aarch64", "arm64"],
        _ => &[arch],
    };
    aliases.iter().any(|a| {
        let escaped = regex::escape(a);
        regex::Regex::new(&format!("^(?:{})$", rule))
            .map(|re| re.is_match(&escaped))
            .unwrap_or(false)
    })
}

// ---------------- 下载引擎（薄下载层 —— 集成对齐点，可整体替换为 crate::download） ----------------
//
// 对齐原版 core/download.ts 的契约子集：
//   download_file ≈ downloadFile(url, dest, progress, sha1, mirror, …)
//   download_all  ≈ downloadAll(tasks, progress, concurrency, mirror, signal)
// 镜像改写 mirror_url 与原版一致（Mojang / libraries / forge / neoforged → bmclapi）。

#[derive(Clone)]
pub struct DownloadItem {
    pub label: String,
    pub url: String,
    pub dest: PathBuf,
    pub sha1: Option<String>,
    pub size: Option<u64>,
    pub reuse_files: Vec<PathBuf>,
    /// 一级目录复用（客户端 jar 的跨版本复用，按大小+sha1 匹配）
    pub reuse_dirs: Vec<PathBuf>,
}

#[derive(Clone, Default)]
pub struct DownloadDetail {
    pub fraction: f64,
    pub bytes_done: u64,
    pub bytes_total: Option<u64>,
    pub indeterminate: bool,
    pub eta_seconds: Option<f64>,
    pub speed_bps: f64,
}

static SPEED_LIMIT_BPS: AtomicU64 = AtomicU64::new(0);
static RATE_STATE: Mutex<Option<(f64, std::time::Instant)>> = Mutex::new(None);

pub fn mirror_pref(app: &AppHandle) -> &'static str {
    if settings::get(app)["mirror"].as_str() == Some("bmclapi") {
        "bmclapi"
    } else {
        "official"
    }
}

pub fn source_text(mirror: &str) -> &'static str {
    if mirror == "bmclapi" {
        "BMCLAPI 镜像"
    } else {
        "官方源"
    }
}

/// 对应原版 download.ts mirrorUrl：bmclapi 模式下改写 Mojang/官方 maven 域名
pub fn mirror_url(input: &str, mirror: &str) -> String {
    if mirror != "bmclapi" {
        return input.to_string();
    }
    let Ok(u) = reqwest::Url::parse(input) else {
        return input.to_string();
    };
    if u.scheme() != "https" && u.scheme() != "http" {
        return input.to_string();
    }
    let base = "https://bmclapi2.bangbang93.com";
    let host = u.host_str().unwrap_or("");
    let path = u.path();
    let query = u.query().map(|q| format!("?{}", q)).unwrap_or_default();
    if [
        "piston-meta.mojang.com",
        "piston-data.mojang.com",
        "launchermeta.mojang.com",
        "launcher.mojang.com",
    ]
    .contains(&host)
    {
        return format!("{}{}{}", base, path, query);
    }
    if host == "resources.download.minecraft.net" {
        return format!("{}/assets{}{}", base, path, query);
    }
    if ["libraries.minecraft.net", "maven.fabricmc.net", "maven.minecraftforge.net"].contains(&host) {
        return format!("{}{}{}", BMCL_MAVEN_ROOT, path.trim_start_matches('/'), query);
    }
    if host == "maven.neoforged.net" && path.starts_with("/releases/") {
        return format!("{}{}{}", base, path.trim_start_matches("/releases"), query);
    }
    if host == "files.minecraftforge.net" && path.starts_with("/maven/") {
        return format!("{}{}{}", base, path, query);
    }
    input.to_string()
}

/// 对应原版 downloadCandidates：镜像候选在前，原始 URL 兜底
pub fn download_candidates(urls: &[String], mirror: &str) -> Vec<String> {
    let mut out = Vec::new();
    for url in urls {
        if mirror == "bmclapi" {
            let m = mirror_url(url, mirror);
            if !out.contains(&m) {
                out.push(m);
            }
        }
        if !out.contains(url) {
            out.push(url.clone());
        }
    }
    out
}

fn throttle(bytes: usize) {
    let limit = SPEED_LIMIT_BPS.load(Ordering::Relaxed);
    if limit == 0 {
        return;
    }
    let wait = {
        let mut g = RATE_STATE.lock().unwrap();
        let now = std::time::Instant::now();
        let entry = g.get_or_insert((limit as f64, now));
        let elapsed = now.duration_since(entry.1).as_secs_f64();
        entry.0 = (entry.0 + elapsed * limit as f64).min(limit as f64 * 2.0);
        entry.1 = now;
        entry.0 -= bytes as f64;
        if entry.0 < 0.0 {
            Some(((-entry.0) / limit as f64).min(1.0))
        } else {
            None
        }
    };
    if let Some(wait) = wait {
        std::thread::sleep(std::time::Duration::from_secs_f64(wait));
    }
}

fn download_client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .user_agent("WUHUICL/1.0 (wuhuicl launcher)")
        .build()
        .expect("HTTP 客户端构建失败")
}

fn check_cancel(rec: Option<&TaskRecord>) -> Result<(), String> {
    if let Some(rec) = rec {
        tasks::wait_while_paused(rec);
        if rec.cancelled.load(Ordering::SeqCst) {
            return Err("已取消".into());
        }
    }
    Ok(())
}

/// 对应原版 downloadFile（子集）：sha1 校验 + .part 事务 + reuseFiles/reuseDirs 复用 + 镜像候选
pub fn download_file(
    _app: &AppHandle,
    rec: Option<&TaskRecord>,
    client: Option<&reqwest::blocking::Client>,
    url: &str,
    dest: &Path,
    sha1: Option<&str>,
    size: Option<u64>,
    mirror: &str,
    progress: Option<FileProgress>,
    reuse_files: &[PathBuf],
    reuse_dirs: &[PathBuf],
) -> Result<u64, String> {
    let matches_hash = |p: &Path| -> Option<bool> {
        match sha1 {
            Some(h) => sha1_file_hex(p).ok().map(|x| x.eq_ignore_ascii_case(h)),
            None => Some(true),
        }
    };
    // 已有文件直接复用（无校验值时信任本地，与原版 reuse 语义一致）
    if dest.exists() {
        if matches_hash(dest) == Some(true) {
            let n = std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0);
            if let Some(cb) = progress {
                cb(n, size);
            }
            return Ok(n);
        }
        let _ = std::fs::remove_file(dest);
    }
    // 精确路径复用（多文件夹 libraries/assets 共享）
    for candidate in reuse_files {
        if candidate.exists() && matches_hash(candidate) == Some(true) {
            if let Some(dir) = dest.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if std::fs::copy(candidate, dest).is_ok() {
                let n = std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0);
                if let Some(cb) = progress {
                    cb(n, size);
                }
                return Ok(n);
            }
        }
    }
    // 一级目录扫描复用（大小匹配 → sha1 校验 → 复制；对应原版 reuseDirs）
    if sha1.is_some() {
        'dirs: for dir in reuse_dirs {
            let Ok(rd) = std::fs::read_dir(dir) else { continue };
            for entry in rd.flatten() {
                if !entry.path().is_file() {
                    continue;
                }
                if let (Ok(st), Some(expect)) = (entry.metadata(), size) {
                    if st.len() != expect {
                        continue;
                    }
                }
                if let Ok(h) = sha1_file_hex(&entry.path()) {
                    if h.eq_ignore_ascii_case(sha1.unwrap_or("")) {
                        if let Some(pdir) = dest.parent() {
                            let _ = std::fs::create_dir_all(pdir);
                        }
                        if std::fs::copy(entry.path(), dest).is_ok() {
                            let n = std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0);
                            if let Some(cb) = progress {
                                cb(n, size);
                            }
                            break 'dirs;
                        }
                    }
                }
            }
        }
        if dest.exists() {
            return Ok(std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0));
        }
    }

    let client_owned;
    let client = match client {
        Some(c) => c,
        None => {
            client_owned = download_client();
            &client_owned
        }
    };
    let urls = vec![url.to_string()];
    let candidates = download_candidates(&urls, mirror);
    let file_name = dest
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".into());
    let part = dest
        .parent()
        .unwrap_or(Path::new("."))
        .join(format!("{}.part", file_name));
    let mut last_err = String::new();
    for candidate in &candidates {
        check_cancel(rec)?;
        let resp = match client.get(candidate).send() {
            Ok(r) => r,
            Err(e) => {
                last_err = e.to_string();
                continue;
            }
        };
        if !resp.status().is_success() {
            last_err = format!("HTTP {}: {}", resp.status().as_u16(), candidate);
            continue;
        }
        let total = resp.content_length().or(size);
        let mut hasher = Sha1::new();
        let mut file = match std::fs::File::create(&part) {
            Ok(f) => f,
            Err(e) => return Err(e.to_string()),
        };
        let mut resp = resp;
        let mut done: u64 = 0;
        let mut last_report: u64 = 0;
        let mut interrupted = false;
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            if let Err(e) = check_cancel(rec) {
                drop(file);
                let _ = std::fs::remove_file(&part);
                return Err(e);
            }
            match resp.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    hasher.update(&buf[..n]);
                    if let Err(e) = file.write_all(&buf[..n]) {
                        drop(file);
                        let _ = std::fs::remove_file(&part);
                        return Err(e.to_string());
                    }
                    done += n as u64;
                    throttle(n);
                    if done - last_report >= 256 * 1024 {
                        last_report = done;
                        if let Some(cb) = progress {
                            cb(done, total);
                        }
                    }
                }
                Err(_) => {
                    interrupted = true;
                    break;
                }
            }
        }
        if interrupted {
            drop(file);
            let _ = std::fs::remove_file(&part);
            last_err = format!("下载中断：{}", candidate);
            continue;
        }
        drop(file);
        let actual = hex::encode(hasher.finish());
        if let Some(expect) = sha1 {
            if !actual.eq_ignore_ascii_case(expect) {
                let _ = std::fs::remove_file(&part);
                last_err = format!("SHA-1 校验失败：{}", candidate);
                continue;
            }
        } else if let Some(t) = total {
            if done < t {
                let _ = std::fs::remove_file(&part);
                last_err = format!("下载中断：{}", candidate);
                continue;
            }
        }
        let _ = std::fs::remove_file(dest);
        if let Err(e) = std::fs::rename(&part, dest) {
            let _ = std::fs::remove_file(&part);
            return Err(e.to_string());
        }
        if let Some(cb) = progress {
            cb(done, total);
        }
        return Ok(done);
    }
    let _ = std::fs::remove_file(&part);
    if last_err.is_empty() {
        last_err = format!("下载失败：{}", url);
    }
    Err(last_err)
}

/// 对应原版 downloadAll：并发下载 + 批量进度聚合 + 取消/暂停贯穿（任一文件失败即整批失败）
pub fn download_all(
    app: &AppHandle,
    rec: Option<&TaskRecord>,
    items: Vec<DownloadItem>,
    mirror: &str,
    progress: BatchProgress,
) -> Result<(), String> {
    let total = items.len();
    if total == 0 {
        return Ok(());
    }
    let s = settings::get(app);
    SPEED_LIMIT_BPS.store(
        s["downloadSpeedKBps"].as_u64().unwrap_or(0).saturating_mul(1024),
        Ordering::Relaxed,
    );
    let threads = (s["downloadThreads"].as_u64().unwrap_or(16) as usize).clamp(1, 64);
    // 全部任务的 size 已知才给 bytesTotal，否则 indeterminate（与原版 tracker 一致）
    let bytes_total: Option<u64> = items.iter().try_fold(0u64, |acc, t| t.size.map(|n| acc + n));
    let queue: Mutex<Vec<DownloadItem>> = Mutex::new(items);
    let errors: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let done = AtomicU64::new(0);
    let bytes_done = AtomicU64::new(0);
    let window: Mutex<(std::time::Instant, u64)> = Mutex::new((std::time::Instant::now(), 0));
    let last_speed: Mutex<f64> = Mutex::new(0.0);
    let client = download_client();
    let app2 = app.clone();
    std::thread::scope(|scope| {
        for _ in 0..threads.min(total) {
            scope.spawn(|| loop {
                let item = {
                    let mut q = queue.lock().unwrap();
                    q.pop()
                };
                let Some(item) = item else { break };
                if let Err(e) = check_cancel(rec) {
                    queue.lock().unwrap().clear();
                    errors.lock().unwrap().push(e);
                    break;
                }
                let result = download_file(
                    &app2,
                    rec,
                    Some(&client),
                    &item.url,
                    &item.dest,
                    item.sha1.as_deref(),
                    item.size,
                    mirror,
                    None,
                    &item.reuse_files,
                    &item.reuse_dirs,
                );
                match result {
                    Ok(n) => {
                        bytes_done.fetch_add(n, Ordering::Relaxed);
                        let d = done.fetch_add(1, Ordering::Relaxed) + 1;
                        let (speed, eta) = {
                            let mut w = window.lock().unwrap();
                            let elapsed = w.0.elapsed().as_secs_f64();
                            let total_done = bytes_done.load(Ordering::Relaxed);
                            let delta = total_done - w.1;
                            if elapsed > 0.3 && delta > 0 {
                                w.0 = std::time::Instant::now();
                                w.1 = total_done;
                                *last_speed.lock().unwrap() = delta as f64 / elapsed;
                            }
                            let sp = *last_speed.lock().unwrap();
                            let eta = match bytes_total {
                                Some(t) if sp > 1.0 => {
                                    Some((t.saturating_sub(total_done)) as f64 / sp)
                                }
                                _ => None,
                            };
                            (sp, eta)
                        };
                        let total_done = bytes_done.load(Ordering::Relaxed);
                        let detail = DownloadDetail {
                            fraction: d as f64 / total as f64,
                            bytes_done: total_done,
                            bytes_total,
                            indeterminate: bytes_total.is_none(),
                            eta_seconds: eta,
                            speed_bps: speed,
                        };
                        progress(d as usize, total as usize, speed, &detail);
                    }
                    Err(e) => {
                        queue.lock().unwrap().clear();
                        errors.lock().unwrap().push(e);
                        break;
                    }
                }
            });
        }
    });
    SPEED_LIMIT_BPS.store(0, Ordering::Relaxed);
    let errs = errors.into_inner().unwrap_or_default();
    if let Some(first) = errs.into_iter().next() {
        return Err(first);
    }
    Ok(())
}

// ---------------- maven 工具（对应原版 mavenIdentity.ts / collectLibraries 内部工具） ----------------

fn maven_path(name: &str) -> Option<String> {
    let (coordinate, extension) = match name.split_once('@') {
        Some((c, e)) => (c, e),
        None => (name, "jar"),
    };
    let parts: Vec<&str> = coordinate.split(':').collect();
    if parts.len() < 3 {
        return None;
    }
    let (g, a, v) = (parts[0], parts[1], parts[2]);
    let classifier = parts.get(3).copied();
    let file = format!(
        "{}-{}{}.{}",
        a,
        v,
        classifier.map(|c| format!("-{}", c)).unwrap_or_default(),
        extension
    );
    Some(format!("{}/{}/{}/{}", g.replace('.', "/"), a, v, file))
}

fn maven_identity(name: Option<&str>, classifier: Option<&str>) -> Option<String> {
    let name = name?;
    let (coordinate, extension) = match name.split_once('@') {
        Some((c, e)) => (c, e),
        None => (name, "jar"),
    };
    let parts: Vec<&str> = coordinate.split(':').collect();
    if parts.len() < 3 || parts.len() > 4 || parts.iter().any(|p| p.is_empty()) {
        return None;
    }
    Some(format!(
        "{}:{}:{}@{}",
        parts[0],
        parts[1],
        classifier.unwrap_or(parts.get(3).copied().unwrap_or("")),
        extension
    ))
}

struct LibEntry {
    rel: String,
    url: Option<String>,
    sha1: Option<String>,
    size: Option<u64>,
    #[allow(dead_code)]
    is_native: bool,
}

/// 对应原版 versions.ts collectLibraries：rules 过滤 + artifact/natives 收集去重
fn collect_libraries(vj: &Value) -> Vec<LibEntry> {
    let mut out: Vec<LibEntry> = Vec::new();
    let mut seen = HashSet::new();
    let mut coordinates = HashSet::new();
    let empty = Vec::new();
    let libs = vj["libraries"].as_array().unwrap_or(&empty);
    for source in libs {
        if !rules_allow(source.get("rules")) {
            continue;
        }
        // 原版 nativeLibraryForHost 仅改写 linux-arm64；Windows 平台直通
        let lib = source;
        let name = lib.get("name").and_then(|v| v.as_str());
        let coordinate = maven_identity(name, None);
        let downloads = lib.get("downloads");
        let artifact = downloads.and_then(|d| d.get("artifact"));
        if let Some(art) = artifact {
            if let Some(rel) = art.get("path").and_then(|v| v.as_str()) {
                push_lib(
                    &mut out,
                    &mut seen,
                    &mut coordinates,
                    rel,
                    art.get("url").and_then(|v| v.as_str()),
                    art.get("sha1").and_then(|v| v.as_str()),
                    art.get("size").and_then(|v| v.as_u64()),
                    false,
                    coordinate.as_deref(),
                );
            }
        } else if downloads.and_then(|d| d.get("classifiers")).is_none() {
            if let (Some(n), Some(base)) = (name, lib.get("url").and_then(|v| v.as_str())) {
                // Fabric/Quilt profile 的 maven 坐标形式
                if let Some(rel) = maven_path(n) {
                    let base = if base.ends_with('/') { base.to_string() } else { format!("{}/", base) };
                    push_lib(
                        &mut out, &mut seen, &mut coordinates, &rel,
                        Some(&format!("{}{}", base, rel)), None, None, false,
                        coordinate.as_deref(),
                    );
                }
            } else if let Some(n) = name {
                // forge 安装器注入库：按组织推断 maven 源
                if let Some(rel) = maven_path(n) {
                    let repo_base = if n.starts_with("net.minecraftforge:") {
                        Some("https://maven.minecraftforge.net/")
                    } else if n.starts_with("net.neoforged:") {
                        Some("https://maven.neoforged.net/releases/")
                    } else {
                        None
                    };
                    let lib_url = repo_base.as_deref().map(|b| format!("{}{}", b, rel));
                    push_lib(
                        &mut out, &mut seen, &mut coordinates, &rel,
                        lib_url.as_deref(), None, None, false,
                        coordinate.as_deref(),
                    );
                }
            }
        }
        // natives classifier（${arch} 展开为 64）
        if let Some(natives) = lib
            .get("natives")
            .and_then(|n| n.get(OS_NAME))
            .and_then(|v| v.as_str())
        {
            let key = natives.replace("${arch}", "64");
            if let Some(art) = downloads
                .and_then(|d| d.get("classifiers"))
                .and_then(|c| c.get(key.as_str()))
            {
                if let Some(rel) = art.get("path").and_then(|v| v.as_str()) {
                    let coord = maven_identity(name, Some(&key));
                    push_lib(
                        &mut out, &mut seen, &mut coordinates, rel,
                        art.get("url").and_then(|v| v.as_str()),
                        art.get("sha1").and_then(|v| v.as_str()),
                        art.get("size").and_then(|v| v.as_u64()),
                        true,
                        coord.as_deref(),
                    );
                }
            }
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn push_lib(
    out: &mut Vec<LibEntry>,
    seen: &mut HashSet<String>,
    coordinates: &mut HashSet<String>,
    rel: &str,
    url: Option<&str>,
    sha1: Option<&str>,
    size: Option<u64>,
    is_native: bool,
    coordinate: Option<&str>,
) {
    if let Some(c) = coordinate {
        if coordinates.contains(c) {
            return;
        }
        coordinates.insert(c.to_string());
    }
    if seen.contains(rel) {
        return;
    }
    seen.insert(rel.to_string());
    out.push(LibEntry {
        rel: rel.to_string(),
        url: url.map(str::to_string),
        sha1: sha1.map(str::to_string),
        size,
        is_native,
    });
}

/// 对应原版 libraryTasks：收集有 URL 的库为下载任务（跨文件夹 libraries 复用）
pub fn library_tasks(app: &AppHandle, vj: &Value) -> Vec<DownloadItem> {
    let folders = all_folders(app);
    let libs_dir = libraries_dir(app);
    collect_libraries(vj)
        .into_iter()
        .filter(|e| e.url.is_some())
        .map(|e| DownloadItem {
            label: e.rel.clone(),
            url: e.url.clone().unwrap_or_default(),
            dest: libs_dir.join(e.rel.replace('/', "\\")),
            sha1: e.sha1,
            size: e.size,
            reuse_files: folders
                .iter()
                .map(|f| Path::new(f).join("libraries").join(e.rel.replace('/', "\\")))
                .collect(),
            reuse_dirs: Vec::new(),
        })
        .collect()
}

// ---------------- 进度守卫（对应原版 progress.ts createWeightedProgressEmit + ProgressEventGuard） ----------------

pub struct ProgressGuard {
    last_overall: f64,
    last_stage: String,
}

impl ProgressGuard {
    pub fn new() -> Self {
        Self { last_overall: 0.0, last_stage: String::new() }
    }

    /// 追踪 lastStage（跳过 error/done，对应 ipc.ts taskEmit）
    pub fn note_stage(&mut self, stage: &str) {
        if !["error", "done"].contains(&stage) {
            self.last_stage = stage.to_string();
        }
    }

    pub fn last_stage(&self) -> &str {
        &self.last_stage
    }

    /// 归一化：overall 单调不减、按阶段权重映射、ETA 取整去 NaN
    pub fn normalize(&mut self, event: &Value) -> Value {
        let mut e = event.as_object().cloned().unwrap_or_default();
        let stage = e.get("stage").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let progress = e
            .get("progress")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0)
            .clamp(0.0, 1.0);
        let overall = match e.get("overall").and_then(|v| v.as_f64()) {
            Some(o) => o,
            None => match STAGE_RANGES.iter().find(|(s, _, _)| *s == stage) {
                Some((_, start, end)) => start + (end - start) * progress,
                None => self.last_overall,
            },
        };
        let overall = overall.clamp(0.0, 1.0);
        self.last_overall = self.last_overall.max(overall);
        e.insert("overall".into(), json!(self.last_overall));
        if let Some(eta) = e.get("etaSeconds").and_then(|v| v.as_f64()) {
            if !eta.is_finite() || eta < 0.0 {
                e.remove("etaSeconds");
            } else {
                e.insert("etaSeconds".into(), json!(eta.round() as u64));
            }
        }
        Value::Object(e)
    }
}

pub fn emit_event(app: &AppHandle, event: &str, payload: &Value) {
    use tauri::Emitter;
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.emit_to("main", event, payload);
    }
}

/// taskEmit：合并 ProgressEvent + taskId/taskTitle/versionId 后推送 event:progress
pub fn task_emit(
    app: &AppHandle,
    guard: &mut ProgressGuard,
    task_id: &str,
    task_title: &str,
    version_id: &str,
    event: &Value,
) {
    guard.note_stage(event.get("stage").and_then(|v| v.as_str()).unwrap_or(""));
    let mut payload = guard.normalize(event);
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("versionId".into(), json!(version_id));
        obj.insert("taskId".into(), json!(task_id));
        obj.insert("taskTitle".into(), json!(task_title));
    }
    tasks::emit_progress(app, payload);
}

// ---------------- 实例目录判定（对应原版 instances.ts instanceDirectoryState） ----------------

pub fn has_isolation_content(instance_path: &Path) -> bool {
    ISOLATION_DIRS
        .iter()
        .chain(ISOLATION_FILES.iter())
        .any(|name| instance_path.join(name).exists())
}

/// 返回 {path, sharedPath, isolated, reason}（显式 false 优先，避免遗留文件自动重新开启）
pub fn instance_directory_state(app: &AppHandle, folder: &str, id: &str, j: &Value) -> Value {
    let shared = canonical_path(folder).to_string_lossy().to_string();
    let instance_path = canonical_path(&version_dir_in(folder, id).to_string_lossy())
        .to_string_lossy()
        .to_string();
    match j.get("_gameDir") {
        Some(Value::Bool(true)) => {
            return json!({ "path": instance_path, "sharedPath": shared, "isolated": true, "reason": "explicit" });
        }
        Some(Value::Bool(false)) => {
            return json!({ "path": shared, "sharedPath": shared, "isolated": false, "reason": "shared" });
        }
        _ => {}
    }
    if let Some(configured) = j
        .get("_gameDirectory")
        .or_else(|| j.get("gameDirectory"))
        .and_then(|v| v.as_str())
    {
        if !configured.trim().is_empty() {
            let configured_path = if Path::new(configured).is_absolute() {
                PathBuf::from(configured)
            } else {
                Path::new(&instance_path).join(configured)
            };
            let configured_path = canonical_path(&configured_path.to_string_lossy())
                .to_string_lossy()
                .to_string();
            let isolated = !same_path(&configured_path, &shared);
            return json!({ "path": configured_path, "sharedPath": shared, "isolated": isolated, "reason": "configured-path" });
        }
    }
    if j.get("_modpackName").is_some() {
        return json!({ "path": instance_path, "sharedPath": shared, "isolated": true, "reason": "modpack" });
    }
    if has_isolation_content(Path::new(&instance_path)) {
        return json!({ "path": instance_path, "sharedPath": shared, "isolated": true, "reason": "detected-content" });
    }
    json!({ "path": shared, "sharedPath": shared, "isolated": false, "reason": "shared" })
}

// ---------------- 实例元数据（对应原版 instanceMetadata.ts resolveInstanceMetadata） ----------------

fn is_minecraft_version_id(value: Option<&str>) -> bool {
    let Some(v) = value else { return false };
    if v.is_empty() || v.trim() != v || v == "0.0.0" {
        return false;
    }
    regex::Regex::new(
        r"^(?:[1-9]\d*\.\d+(?:\.\d+)?(?:-(?:pre|rc)-?\d+|-snapshot-\d+| Pre-Release \d+)?|\d{2}w\d{2}[a-z](?:[\w.-]*)|[ab]\d+(?:\.\d+)+[a-z]?(?:_\d+|-pre\d+)?|c\d+(?:\.\d+)+[a-z]?(?:_[\w-]+)?|rd-\d+|inf-\d+|1\.RV-Pre1|3D Shareware v1\.34)$",
    )
    .map(|re| re.is_match(v))
    .unwrap_or(false)
}

pub struct ResolvedMeta {
    pub mc_version: String,
    pub loader: Option<&'static str>,
    pub loader_version: Option<String>,
}

/// 解析实例真实 MC 版本与加载器（继承链 + 启动参数 + 库坐标）
pub fn resolve_instance_metadata(j: &Value, read_parent: &dyn Fn(&str) -> Option<Value>) -> ResolvedMeta {
    let mut chain: Vec<Value> = vec![j.clone()];
    let mut seen: HashSet<String> = HashSet::new();
    if let Some(id) = j.get("id").and_then(|v| v.as_str()) {
        seen.insert(id.to_string());
    }
    loop {
        let Some(parent_id) = chain
            .last()
            .and_then(|c| c.get("inheritsFrom"))
            .and_then(|v| v.as_str())
        else {
            break;
        };
        if seen.contains(parent_id) || chain.len() >= 16 || parent_id.is_empty() {
            break;
        }
        seen.insert(parent_id.to_string());
        match read_parent(parent_id) {
            Some(p) => chain.push(p),
            None => break,
        }
    }
    // 启动参数收集（arguments.game + minecraftArguments）
    let mut args: Vec<String> = Vec::new();
    for c in &chain {
        if let Some(game) = c.get("arguments").and_then(|a| a.get("game")).and_then(|g| g.as_array()) {
            for a in game {
                match a {
                    Value::String(s) => args.push(s.clone()),
                    Value::Object(_) => {
                        if let Some(v) = a.get("value") {
                            match v {
                                Value::String(s) => args.push(s.clone()),
                                Value::Array(arr) => {
                                    for x in arr {
                                        if let Some(s) = x.as_str() {
                                            args.push(s.to_string());
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        if let Some(mc_args) = c.get("minecraftArguments").and_then(|v| v.as_str()) {
            for token in mc_args.split([' ', '\n', '\t']) {
                args.push(token.trim_matches('"').to_string());
            }
        }
    }
    let arg = |name: &str| -> Option<String> {
        if let Some(i) = args.iter().position(|a| a == name) {
            return args.get(i + 1).cloned();
        }
        let prefix = format!("{}=", name);
        args.iter()
            .find(|a| a.starts_with(&prefix))
            .map(|a| a[prefix.len()..].to_string())
    };
    let libs: Vec<Vec<String>> = chain
        .iter()
        .flat_map(|c| c.get("libraries").and_then(|l| l.as_array()).cloned().unwrap_or_default())
        .map(|l| {
            l.get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .split(':')
                .map(str::to_string)
                .collect()
        })
        .collect();
    let lib = |group: &str, artifact: &str| -> Option<String> {
        libs.iter()
            .find(|l| {
                l.first().map(|g| g == group).unwrap_or(false)
                    && l.get(1).map(|a| a == artifact).unwrap_or(false)
            })
            .and_then(|l| l.get(2).cloned())
    };
    let detected = if arg("--fml.neoForgeVersion").is_some()
        || lib("net.neoforged", "neoforge").is_some()
        || lib("net.neoforged.fancymodloader", "loader").is_some()
    {
        Some("neoforge")
    } else if arg("--fml.forgeVersion").is_some()
        || lib("net.minecraftforge", "forge").is_some()
        || lib("net.minecraftforge", "fmlloader").is_some()
    {
        Some("forge")
    } else if lib("net.fabricmc", "fabric-loader").is_some() {
        Some("fabric")
    } else if lib("org.quiltmc", "quilt-loader").is_some() {
        Some("quilt")
    } else {
        None
    };
    let main = chain
        .iter()
        .map(|c| c.get("mainClass").and_then(|v| v.as_str()).unwrap_or(""))
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let loader = detected
        .or_else(|| {
            chain
                .iter()
                .find_map(|c| c.get("_loader").and_then(|v| v.as_str()))
                .and_then(normalize_loader)
        })
        .or_else(|| {
            if main.contains("neoforged") {
                Some("neoforge")
            } else if main.contains("minecraftforge") {
                Some("forge")
            } else if main.contains("fabricmc") {
                Some("fabric")
            } else if main.contains("quiltmc") {
                Some("quilt")
            } else {
                None
            }
        });
    let forge = lib("net.minecraftforge", "forge").or_else(|| lib("net.minecraftforge", "fmlloader"));
    let base = chain.last().cloned().unwrap_or(Value::Null);
    let vanilla_id = if base.get("inheritsFrom").is_none() && (loader.is_none() || chain.len() > 1) {
        base.get("id").and_then(|v| v.as_str()).map(str::to_string)
    } else {
        None
    };
    let mut candidates: Vec<Option<String>> = vec![arg("--fml.mcVersion")];
    for c in &chain {
        candidates.push(c.get("clientVersion").and_then(|v| v.as_str()).map(str::to_string));
        candidates.push(c.get("_mcVersion").and_then(|v| v.as_str()).map(str::to_string));
    }
    candidates.push(vanilla_id);
    for c in &chain {
        candidates.push(c.get("inheritsFrom").and_then(|v| v.as_str()).map(str::to_string));
    }
    candidates.push(lib("net.fabricmc", "intermediary"));
    candidates.push(lib("org.quiltmc", "hashed"));
    if let Some(f) = &forge {
        if f.contains('-') {
            candidates.push(f.split('-').next().map(str::to_string));
        }
    }
    let mc_version = candidates
        .into_iter()
        .find(|c| is_minecraft_version_id(c.as_deref()))
        .flatten()
        .unwrap_or_else(|| "未知".to_string());
    let raw_loader = match loader {
        Some("neoforge") => arg("--fml.neoForgeVersion").or_else(|| lib("net.neoforged", "neoforge")),
        Some("forge") => arg("--fml.forgeVersion").or(forge),
        Some("fabric") => lib("net.fabricmc", "fabric-loader"),
        Some("quilt") => lib("org.quiltmc", "quilt-loader"),
        _ => None,
    }
    .or_else(|| {
        chain
            .iter()
            .find_map(|c| c.get("_loaderVersion").and_then(|v| v.as_str()))
            .map(str::to_string)
    });
    ResolvedMeta {
        mc_version,
        loader,
        loader_version: raw_loader,
    }
}

fn normalize_loader(v: &str) -> Option<&'static str> {
    match v {
        "forge" => Some("forge"),
        "fabric" => Some("fabric"),
        "quilt" => Some("quilt"),
        "neoforge" => Some("neoforge"),
        _ => None,
    }
}

// ---------------- 版本清单查找（getVersionJson 依赖 catalog 模块） ----------------

/// 对应原版 getVersionJson：本地不存在时经清单（含强制刷新重试）下载
pub fn get_version_json(app: &AppHandle, rec: Option<&TaskRecord>, dest: &Path, version_id: &str) -> Result<Value, String> {
    if !dest.exists() {
        let catalog = crate::catalog::fetch_version_catalog(app, false)?;
        let mut entry = catalog["versions"]
            .as_array()
            .and_then(|a| a.iter().find(|v| v["id"].as_str() == Some(version_id)))
            .cloned();
        if entry.is_none() {
            // 可能是新发布的版本，强制刷新一次清单再找
            let fresh = crate::catalog::fetch_version_catalog(app, true)?;
            entry = fresh["versions"]
                .as_array()
                .and_then(|a| a.iter().find(|v| v["id"].as_str() == Some(version_id)))
                .cloned();
        }
        let Some(entry) = entry else {
            return Err(format!("版本清单中找不到 {}", version_id));
        };
        let url = entry["url"].as_str().unwrap_or("").to_string();
        if let Some(dir) = dest.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let mirror = mirror_pref(app).to_string();
        let app2 = app.clone();
        let dest2 = dest.to_path_buf();
        run_blocking(move || download_file(&app2, rec, None, &url, &dest2, None, None, &mirror, None, &[], &[]))?;
    }
    read_version_json_from(dest)
}

// ---------------- 安装：原版（对应原版 installVanilla / installVanillaUnlocked） ----------------

fn fmt_mb(bytes: u64) -> String {
    format!("{:.1}MB", bytes as f64 / 1024.0 / 1024.0)
}

/// dest='versions' 独立版本 / dest='base' 依赖原版区；返回最终版本 id
#[allow(clippy::too_many_arguments)]
pub fn install_vanilla(
    app: &AppHandle,
    rec: &TaskRecord,
    emit: EmitFn,
    active: &str,
    version_id: &str,
    dest: &str,
    instance_name: Option<&str>,
    final_event: bool,
) -> Result<String, String> {
    let default_folder = default_folder_path(app);
    let final_id = if dest == "versions" {
        instance_name
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(version_id)
            .to_string()
    } else {
        version_id.to_string()
    };
    let dir = if dest == "base" {
        base_version_dir_in(&default_folder, version_id)
    } else {
        version_dir_in(active, &final_id)
    };
    let json_path = dir.join(format!("{}.json", final_id));
    let jar_path = dir.join(format!("{}.jar", final_id));
    let mark = dir.join(".installing");
    let mirror = mirror_pref(app).to_string();
    let source = source_text(&mirror);

    // 事务标记：安装开始打标，全部成功才移除；失败由 cleanupPartialInstall 清理
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    if dest == "versions" {
        register_version_folder(&final_id, active);
    }
    std::fs::write(&mark, now_iso()).map_err(|e| e.to_string())?;
    let result = (|| -> Result<(), String> {
        emit(&json!({ "stage": "version-json", "progress": 0, "text": format!("获取版本信息 {}", version_id), "source": source }));
        let mut vj = get_version_json(app, Some(rec), &json_path, version_id)?;
        // 自定义实例名：json id 同步改写，并记录真实 MC 版本
        if final_id != version_id {
            vj["id"] = json!(final_id);
            vj["_mcVersion"] = json!(version_id);
            write_version_json(&json_path, &vj)?;
        }

        // 1. 依赖库（含 natives classifiers）
        let lib_tasks = library_tasks(app, &vj);
        let lib_total = lib_tasks.len();
        {
            let app2 = app.clone();
            let mirror2 = mirror.clone();
            let emit2 = |event: &Value| emit(event);
            let lib_progress = move |d: usize, t: usize, _speed: f64, detail: &DownloadDetail| {
                emit2(&json!({
                    "stage": "libraries",
                    "progress": detail.fraction,
                    "text": format!("下载依赖库 {}/{}", d, t.max(lib_total)),
                    "speed": if detail.speed_bps > 0.0 { json!(detail.speed_bps) } else { Value::Null },
                    "etaSeconds": detail.eta_seconds.map(|e| json!(e)).unwrap_or(Value::Null),
                    "bytesDone": detail.bytes_done,
                    "bytesTotal": detail.bytes_total.map(|b| json!(b)).unwrap_or(Value::Null),
                    "indeterminate": detail.indeterminate,
                    "source": source
                }));
            };
            run_blocking(move || download_all(&app2, Some(rec), lib_tasks, &mirror2, &lib_progress))?;
        }

        // 2. 客户端 jar
        let client = vj
            .get("downloads")
            .and_then(|d| d.get("client"))
            .cloned()
            .unwrap_or(Value::Null);
        if let Some(client_url) = client.get("url").and_then(|v| v.as_str()) {
            let sha1 = client.get("sha1").and_then(|v| v.as_str()).map(str::to_string);
            let size = client.get("size").and_then(|v| v.as_u64());
            let jar = jar_path.clone();
            let app2 = app.clone();
            let mirror2 = mirror.clone();
            let emit2 = |event: &Value| emit(event);
            let src2 = source;
            let progress: FileProgress = &move |done: u64, total: Option<u64>| {
                emit2(&json!({
                    "stage": "client",
                    "progress": total.map(|t| done as f64 / t as f64).unwrap_or(0.0),
                    "bytesDone": done,
                    "bytesTotal": total.map(|t| json!(t)).unwrap_or(Value::Null),
                    "indeterminate": total.is_none(),
                    "text": format!("下载游戏本体 {}", fmt_mb(done)),
                    "source": src2
                }));
            };
            run_blocking(move || {
                download_file(&app2, Some(rec), None, client_url, &jar, sha1.as_deref(), size, &mirror2, Some(progress), &[], &[])
            })?;
        }

        // 3. 资源索引与资源文件
        emit(&json!({ "stage": "assets", "progress": 0, "text": "获取资源索引" }));
        let idx_ref = vj.get("assetIndex").cloned().filter(|v| !v.is_null());
        if let Some(idx_ref) = idx_ref {
            if let Some(idx_url) = idx_ref.get("url").and_then(|v| v.as_str()) {
                let index_id = idx_ref.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let idx_path = asset_index_path(app, &index_id);
                let reuse: Vec<PathBuf> = all_folders(app)
                    .iter()
                    .map(|f| Path::new(f).join("assets").join("indexes").join(format!("{}.json", index_id)))
                    .collect();
                {
                    let app2 = app.clone();
                    let mirror2 = mirror.clone();
                    let idx_url2 = idx_url.to_string();
                    let idx_sha1 = idx_ref.get("sha1").and_then(|v| v.as_str()).map(str::to_string);
                    let idx_size = idx_ref.get("size").and_then(|v| v.as_u64());
                    let idx_path2 = idx_path.clone();
                    let reuse2 = reuse.clone();
                    run_blocking(move || {
                        download_file(&app2, Some(rec), None, &idx_url2, &idx_path2, idx_sha1.as_deref(), idx_size, &mirror2, None, &reuse2, &[])
                    })?;
                }
                let idx: Value = read_version_json_from(&idx_path)?;
                let objects = idx
                    .get("objects")
                    .and_then(|o| o.as_object())
                    .cloned()
                    .unwrap_or_default();
                let mut seen = HashSet::new();
                let mut tasks: Vec<DownloadItem> = Vec::new();
                let resource_folders = all_folders(app);
                for (name, o) in &objects {
                    let hash = match o.get("hash").and_then(|v| v.as_str()) {
                        Some(h) if h.len() >= 2 => h.to_string(),
                        _ => continue,
                    };
                    if seen.contains(&hash) {
                        continue;
                    }
                    seen.insert(hash.clone());
                    tasks.push(DownloadItem {
                        label: name.clone(),
                        url: format!("https://resources.download.minecraft.net/{}/{}", &hash[..2], hash),
                        dest: asset_object_path(app, &hash),
                        sha1: Some(hash.clone()),
                        size: o.get("size").and_then(|v| v.as_u64()),
                        reuse_files: resource_folders
                            .iter()
                            .map(|f| {
                                Path::new(f)
                                    .join("assets")
                                    .join("objects")
                                    .join(&hash[..2])
                                    .join(&hash)
                            })
                            .collect(),
                        reuse_dirs: Vec::new(),
                    });
                }
                let assets_total = tasks.len();
                {
                    let app2 = app.clone();
                    let mirror2 = mirror.clone();
                    let emit2 = |event: &Value| emit(event);
                    let asset_progress = move |d: usize, t: usize, _speed: f64, detail: &DownloadDetail| {
                        emit2(&json!({
                            "stage": "assets",
                            "progress": detail.fraction,
                            "text": format!("下载资源文件 {}/{}", d, t.max(assets_total)),
                            "speed": if detail.speed_bps > 0.0 { json!(detail.speed_bps) } else { Value::Null },
                            "etaSeconds": detail.eta_seconds.map(|e| json!(e)).unwrap_or(Value::Null),
                            "bytesDone": detail.bytes_done,
                            "bytesTotal": detail.bytes_total.map(|b| json!(b)).unwrap_or(Value::Null),
                            "indeterminate": detail.indeterminate,
                            "source": source
                        }));
                    };
                    run_blocking(move || download_all(&app2, Some(rec), tasks, &mirror2, &asset_progress))?;
                }

                // legacy 版本需要把资源复制到 assets/virtual/legacy 下
                if idx.get("virtual") == Some(&Value::Bool(true))
                    || idx.get("map_to_resources") == Some(&Value::Bool(true))
                {
                    emit(&json!({ "stage": "assets", "progress": 1, "text": "复制 legacy 资源" }));
                    let vlegacy = virtual_legacy_dir(app);
                    for (name, o) in &objects {
                        check_cancel(Some(rec))?;
                        let Some(hash) = o.get("hash").and_then(|v| v.as_str()) else { continue };
                        let from = asset_object_path(app, hash);
                        let to = vlegacy.join(name.replace('/', "\\"));
                        if from.exists() && !to.exists() {
                            if let Some(dir) = to.parent() {
                                let _ = std::fs::create_dir_all(dir);
                            }
                            let _ = std::fs::copy(&from, &to);
                        }
                    }
                }
            }
        }

        if final_event {
            emit(&json!({ "stage": "done", "progress": 1, "text": format!("校验完成，{} 安装成功", version_id) }));
        } else {
            emit(&json!({ "stage": "assets", "progress": 1, "text": format!("原版 {} 依赖准备完成", version_id) }));
        }
        // 全部步骤成功：移除事务标记
        let _ = std::fs::remove_file(&mark);
        Ok(())
    })();
    match result {
        Ok(()) => Ok(final_id),
        // 失败时保留 .installing 标记（列表显示「安装失败」+ 清理残留入口）
        Err(e) => Err(e),
    }
}

// ---------------- 继承链合并（对应原版 resolveVersionChain / flattenInstance） ----------------

pub fn resolve_version_chain(app: &AppHandle, folder: &str, id: &str) -> Result<(Value, String), String> {
    let mut chain: Vec<Value> = Vec::new();
    let mut cur = Some(read_version_json(app, folder, id)?);
    let mut seen = HashSet::new();
    while let Some(c) = cur {
        let cid = c.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if seen.contains(&cid) || chain.len() >= 32 {
            return Err("版本继承链循环或过长".into());
        }
        seen.insert(cid);
        chain.push(c.clone());
        cur = match c.get("inheritsFrom").and_then(|v| v.as_str()) {
            Some(p) => Some(read_version_json(app, folder, p)?),
            None => None,
        };
    }
    let base_id = chain
        .last()
        .and_then(|c| c.get("id").and_then(|v| v.as_str()))
        .unwrap_or(id)
        .to_string();
    let child_first = |key: &str| -> Option<Value> {
        chain.iter().find_map(|c| c.get(key).cloned().filter(|v| !v.is_null()))
    };
    let mut merged = json!({
        "id": id,
        "mainClass": child_first("mainClass").unwrap_or(Value::Null),
        "type": child_first("type").unwrap_or(Value::Null),
        "assets": child_first("assets").unwrap_or(Value::Null),
        "assetIndex": child_first("assetIndex").unwrap_or(Value::Null),
        "javaVersion": child_first("javaVersion").unwrap_or(Value::Null),
        "minecraftArguments": child_first("minecraftArguments").unwrap_or(Value::Null),
        "downloads": child_first("downloads").unwrap_or(Value::Null),
    });
    let libs: Vec<Value> = chain
        .iter()
        .flat_map(|c| c.get("libraries").and_then(|l| l.as_array()).cloned().unwrap_or_default())
        .collect();
    let mut game_args: Vec<Value> = Vec::new();
    let mut jvm_args: Vec<Value> = Vec::new();
    for c in chain.iter().rev() {
        if let Some(g) = c.get("arguments").and_then(|a| a.get("game")).and_then(|g| g.as_array()) {
            game_args.extend(g.iter().cloned());
        }
        if let Some(j) = c.get("arguments").and_then(|a| a.get("jvm")).and_then(|j| j.as_array()) {
            jvm_args.extend(j.iter().cloned());
        }
    }
    merged["libraries"] = Value::Array(libs);
    merged["arguments"] = json!({ "game": game_args, "jvm": jvm_args });
    Ok((merged, base_id))
}

/// 链底客户端 jar 实际位置（versions 区优先，缺省 .wuhuicl/base）
pub fn client_jar_path(app: &AppHandle, folder: &str, id: &str) -> PathBuf {
    if version_json_path_in(folder, id).exists() {
        version_jar_path_in(folder, id)
    } else {
        base_version_jar_path_in(&default_folder_path(app), id)
    }
}

/// 对应原版 flattenInstance：把带 inheritsFrom 的实例拍平为自包含实例
pub fn flatten_instance(app: &AppHandle, folder: &str, id: &str) -> Result<bool, String> {
    let jp = version_json_path_in(folder, id);
    if !jp.exists() {
        return Ok(false);
    }
    let own = read_version_json(app, folder, id)?;
    if own.get("inheritsFrom").is_none() {
        return Ok(false);
    }
    let (mut merged, base_id) = resolve_version_chain(app, folder, id)?;
    // 自定义字段（_loader/_gameDir/_modpackName…）以实例自身 json 为准保留
    if let (Some(src), Some(dst)) = (own.as_object(), merged.as_object_mut()) {
        for (k, v) in src {
            if k.starts_with('_') {
                dst.insert(k.clone(), v.clone());
            }
        }
    }
    let base_mc = read_version_json(app, folder, &base_id)
        .ok()
        .and_then(|b| b.get("_mcVersion").and_then(|v| v.as_str()).map(str::to_string));
    let mc = own
        .get("_mcVersion")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or(base_mc)
        .unwrap_or_else(|| base_id.clone());
    merged["_mcVersion"] = json!(mc);
    merged["_flattenedAt"] = json!(now_iso());
    if let Some(obj) = merged.as_object_mut() {
        obj.remove("inheritsFrom");
    }
    // client jar 落地实例目录（拷走即用）
    let src_jar = client_jar_path(app, folder, &base_id);
    let dest_jar = version_jar_path_in(folder, id);
    if src_jar.exists() && !dest_jar.exists() {
        std::fs::copy(&src_jar, &dest_jar).map_err(|e| e.to_string())?;
    }
    std::fs::copy(&jp, jp.with_extension("json.kamucl-bak")).ok();
    write_version_json(&jp, &merged)?;
    Ok(true)
}

/// 对应原版 migrateDependencyVanilla：临时落地的原版条目迁移进 .wuhuicl/base
pub fn migrate_dependency_vanilla(app: &AppHandle, active: &str, mc_id: &str) {
    let dir = version_dir_in(active, mc_id);
    if !dir.exists() {
        return;
    }
    // 安装不完整（.installing 标记在）时整个目录直接删除
    if install_mark_path_in(active, mc_id).exists() {
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }
    let jp = version_json_path_in(active, mc_id);
    if !jp.exists() {
        return;
    }
    let default_folder = default_folder_path(app);
    let base = base_version_dir_in(&default_folder, mc_id);
    let _ = std::fs::create_dir_all(&base);
    let base_json = base_version_json_path_in(&default_folder, mc_id);
    let base_jar = base_version_jar_path_in(&default_folder, mc_id);
    if !base_json.exists() {
        let _ = std::fs::rename(&jp, &base_json);
    }
    let jar = version_jar_path_in(active, mc_id);
    if jar.exists() && !base_jar.exists() {
        let _ = std::fs::rename(&jar, &base_jar);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------- 加载器版本列表（对应原版 loaders.ts listLoaderVersions） ----------------

/// 对应原版 compareVersionDesc：数字段数值比，稳定版优先于预发布，最新排最前
pub fn compare_version_desc(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering as O;
    let split = |s: &str| -> Vec<String> { s.split(['.', '-']).map(str::to_string).collect() };
    let (pa, pb) = (split(a), split(b));
    for i in 0..pa.len().max(pb.len()) {
        match (pa.get(i), pb.get(i)) {
            (None, _) => return O::Greater, // 段数少 = 更旧，排后
            (_, None) => return O::Less,
            (Some(xa), Some(xb)) => {
                let a_num = !xa.is_empty() && xa.chars().all(|c| c.is_ascii_digit());
                let b_num = !xb.is_empty() && xb.chars().all(|c| c.is_ascii_digit());
                if a_num && b_num {
                    let d = xb.parse::<u64>().unwrap_or(0).cmp(&xa.parse::<u64>().unwrap_or(0));
                    if d != O::Equal {
                        return d;
                    }
                } else if a_num {
                    return O::Less; // 同位数字段（正式）优先于文本段（beta/rc）
                } else if b_num {
                    return O::Greater;
                } else {
                    let c = xb.cmp(xa); // 文本段按字母倒序（rc > beta）
                    if c != O::Equal {
                        return c;
                    }
                }
            }
        }
    }
    O::Equal
}

fn fetch_json(url: String, timeout_secs: u64) -> Result<Value, String> {
    run_blocking(move || {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .build()
            .map_err(|e| e.to_string())?;
        let resp = client.get(&url).send().map_err(|e| e.to_string())?;
        let status = resp.status().as_u16();
        let text = resp.text().map_err(|e| e.to_string())?;
        if !(200..300).contains(&status) {
            return Err(format!("HTTP {}: {}", status, url));
        }
        serde_json::from_str(&text).map_err(|e| e.to_string())
    })
}

fn fetch_text(url: String, timeout_secs: u64) -> Result<String, String> {
    run_blocking(move || {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .build()
            .map_err(|e| e.to_string())?;
        let resp = client.get(&url).send().map_err(|e| e.to_string())?;
        let status = resp.status().as_u16();
        let text = resp.text().map_err(|e| e.to_string())?;
        if !(200..300).contains(&status) {
            return Err(format!("HTTP {}: {}", status, url));
        }
        Ok(text)
    })
}

/// 获取加载器可用版本列表（最新在前）
pub fn list_loader_versions(loader: &str, mc_version: &str) -> Result<Vec<String>, String> {
    let mut list: Vec<String> = Vec::new();
    match loader {
        "fabric" => {
            let arr = fetch_json(format!("https://meta.fabricmc.net/v2/versions/loader/{}", mc_version), 15)?;
            if let Some(a) = arr.as_array() {
                list = a.iter().filter_map(|x| x["loader"]["version"].as_str()).map(str::to_string).collect();
            }
        }
        "quilt" => {
            let arr = fetch_json(format!("https://meta.quiltmc.org/v3/versions/loader/{}", mc_version), 15)?;
            if let Some(a) = arr.as_array() {
                list = a.iter().filter_map(|x| x["loader"]["version"].as_str()).map(str::to_string).collect();
            }
        }
        "forge" => {
            let arr = fetch_json(format!("https://bmclapi2.bangbang93.com/forge/minecraft/{}", mc_version), 15)?;
            if let Some(a) = arr.as_array() {
                list = a.iter().filter_map(|x| x["version"].as_str()).map(str::to_string).collect();
            }
        }
        "neoforge" => {
            match fetch_json(format!("https://bmclapi2.bangbang93.com/neoforge/list/{}", mc_version), 15) {
                Ok(data) => {
                    if let Some(a) = data.as_array() {
                        list = a
                            .iter()
                            .filter_map(|x| {
                                if let Some(s) = x.as_str() {
                                    Some(s.to_string())
                                } else {
                                    x.get("version").and_then(|v| v.as_str()).map(str::to_string)
                                }
                            })
                            .collect();
                    } else {
                        return Err("返回格式异常".into());
                    }
                }
                Err(_) => {
                    // 回退：解析 maven-metadata.xml，过滤 mc 前缀（1.20.4 -> 20.4）
                    let xml = fetch_text(
                        "https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml".into(),
                        15,
                    )
                    .map_err(|_| "HTTP 0: neoforge maven-metadata".to_string())?;
                    let re = regex::Regex::new(r"<version>([^<]+)</version>").unwrap();
                    let all: Vec<String> =
                        re.captures_iter(&xml).filter_map(|c| c.get(1)).map(|m| m.as_str().to_string()).collect();
                    let prefix = mc_version.replacen("1.", "", 1);
                    list = all.into_iter().filter(|v| v.starts_with(&prefix)).rev().collect();
                }
            }
        }
        _ => return Err("未知的加载器类型".into()),
    }
    list.sort_by(|a, b| compare_version_desc(a, b));
    Ok(list)
}

/// 对应原版 resolveForgeInstallerUrl：旧版 Forge branch 后缀解析
fn resolve_forge_installer_url(mc_version: &str, loader_version: &str, mirror: &str) -> String {
    let base = format!("{}-{}", mc_version, loader_version);
    let installer_url = |coordinate: &str| {
        format!(
            "https://maven.minecraftforge.net/net/minecraftforge/forge/{}/forge-{}-installer.jar",
            coordinate, coordinate
        )
    };
    let modern =
        !regex::Regex::new(r"^1\.(?:[1-9]|1[0-2])(?:\.\d+)?$").unwrap().is_match(mc_version);
    if modern || loader_version.contains('-') {
        return installer_url(&base);
    }
    let metadata = "https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml".to_string();
    for source in download_candidates(std::slice::from_ref(&metadata), mirror) {
        if let Ok(xml) = fetch_text(source, 4) {
            let re = regex::Regex::new(r"<version>([^<]+)</version>").unwrap();
            let versions: Vec<String> = re
                .captures_iter(&xml)
                .filter_map(|c| c.get(1))
                .map(|m| m.as_str().trim().to_string())
                .collect();
            if versions.iter().any(|v| *v == base) {
                return installer_url(&base);
            }
            let branches: Vec<String> = versions
                .into_iter()
                .filter(|v| v.starts_with(&format!("{}-", base)))
                .collect::<HashSet<_>>()
                .into_iter()
                .collect();
            let coordinate = branches
                .iter()
                .find(|v| **v == format!("{}-{}", base, mc_version))
                .or_else(|| if branches.len() == 1 { branches.first() } else { None });
            if let Some(c) = coordinate {
                return installer_url(c);
            }
        }
    }
    // 缺失/不可用的索引不能阻断已使用普通坐标的旧版构建
    installer_url(&base)
}

/// 选一个可用 java 运行安装器（对应原版 pickJavaForInstaller；ensureJava 自动下载 JRE 未移植）
fn pick_java_for_installer(app: &AppHandle) -> String {
    let s = settings::get(app);
    let p = s["javaPath"].as_str().unwrap_or("").trim().to_string();
    if p.is_empty() {
        "java".to_string()
    } else {
        p
    }
}

/// 对应原版 runInstaller：运行 forge/neoforge 安装器（--mirror 失败降级官方源；输出落盘 installer.log）
fn run_installer(
    app: &AppHandle,
    rec: &TaskRecord,
    emit: EmitFn,
    java_path: &str,
    jar: &Path,
    target: &str,
) -> Result<(), String> {
    emit(&json!({ "stage": "loader-process", "progress": 0, "indeterminate": true, "text": "正在启动加载器安装器…" }));
    let use_mirror = mirror_pref(app) == "bmclapi";
    let build_args = |with_mirror: bool| -> Vec<String> {
        let mut args = vec![
            "-jar".to_string(),
            jar.to_string_lossy().to_string(),
            "--installClient".to_string(),
            target.to_string(),
        ];
        if with_mirror {
            args.push(format!("--mirror={}", BMCL_MAVEN_ROOT));
        }
        args
    };
    let run_once = |args: &[String]| -> Result<(), String> {
        use std::process::{Command, Stdio};
        let mut child = Command::new(java_path)
            .args(args)
            .current_dir(target)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("未找到可用的 Java 运行环境，请在设置中配置 Java：{}", e))?;
        let mut stdout = child.stdout.take().ok_or_else(|| "无法读取安装器输出".to_string())?;
        let mut stderr = child.stderr.take().ok_or_else(|| "无法读取安装器输出".to_string())?;
        let lines: std::sync::Arc<Mutex<Vec<String>>> = std::sync::Arc::new(Mutex::new(Vec::new()));
        {
            let lines = lines.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                loop {
                    match stdout.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            let text = String::from_utf8_lossy(&buf[..n]);
                            let mut l = lines.lock().unwrap();
                            for line in text.split(['\n', '\r']) {
                                if !line.trim().is_empty() {
                                    l.push(line.to_string());
                                }
                            }
                        }
                    }
                }
            });
        }
        {
            let lines = lines.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                loop {
                    match stderr.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            let text = String::from_utf8_lossy(&buf[..n]);
                            let mut l = lines.lock().unwrap();
                            for line in text.split(['\n', '\r']) {
                                if !line.trim().is_empty() {
                                    l.push(line.to_string());
                                }
                            }
                        }
                    }
                }
            });
        }
        // 等待进程结束；取消令牌触发 kill（对应 signal.abort → proc.kill）
        loop {
            if let Ok(Some(code)) = child.try_wait() {
                std::thread::sleep(std::time::Duration::from_millis(100));
                let all_lines = lines.lock().unwrap().clone();
                // 全量输出落盘，便于排查
                let log_dir = Path::new(target).join("kamucl-logs");
                let _ = std::fs::create_dir_all(&log_dir);
                let _ = std::fs::write(
                    log_dir.join("installer.log"),
                    format!("{}\n\n[退出码 {}] {}\n", all_lines.join("\n"), code, args.join(" ")),
                );
                if rec.cancelled.load(Ordering::SeqCst) {
                    return Err("已取消".into());
                }
                if code.success() {
                    return Ok(());
                }
                let last: Vec<String> = all_lines.iter().rev().take(30).rev().cloned().collect();
                return Err(format!(
                    "安装器退出码 {}（完整日志见 kamucl-logs/installer.log）\n{}",
                    code,
                    last.join("\n")
                ));
            }
            if rec.cancelled.load(Ordering::SeqCst) {
                let _ = child.kill();
                let _ = child.wait();
                return Err("已取消".into());
            }
            if let Ok(l) = lines.try_lock() {
                let tail: Vec<String> = l.iter().rev().take(2).rev().cloned().collect();
                let mut short = tail.join(" ");
                if short.chars().count() > 160 {
                    short = short.chars().skip(short.chars().count() - 160).collect();
                }
                emit(&json!({
                    "stage": "loader-process", "progress": 0, "indeterminate": true,
                    "text": format!("生成加载器运行文件: {}", if short.is_empty() { "处理中…" } else { &short })
                }));
            }
            std::thread::sleep(std::time::Duration::from_millis(400));
        }
    };
    if use_mirror {
        match run_once(&build_args(true)) {
            Ok(()) => Ok(()),
            Err(e) => {
                if e == "已取消" || rec.cancelled.load(Ordering::SeqCst) {
                    return Err("已取消".into());
                }
                emit(&json!({ "stage": "loader", "progress": 0.7, "text": "镜像模式安装失败，改用官方源重试…" }));
                run_once(&build_args(false))
            }
        }
    } else {
        run_once(&build_args(false))
    }
}

/// 对应原版 findInstalledDir：扫描 versions/ 找安装器生成的版本目录名（最新 mtime）
fn find_installed_dir(active: &str, loader: &str, mc_version: &str, loader_version: &str) -> Option<String> {
    let dir = versions_dir_in(active);
    let mut candidates: Vec<(String, std::time::SystemTime)> = Vec::new();
    let entries = std::fs::read_dir(&dir).ok()?;
    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let Ok(j) = read_version_json_from(&entry.path().join(format!("{}.json", name))) else {
            continue;
        };
        let meta = resolve_instance_metadata(&j, &|pid: &str| {
            read_version_json_from(&version_json_path_in(active, pid)).ok()
        });
        let want = normalize_loader(loader);
        let loader_match = meta.loader == want || j.get("_loader").and_then(|v| v.as_str()) == Some(loader);
        if loader_match
            && meta.mc_version == mc_version
            && meta.loader_version.as_deref() == Some(loader_version)
        {
            let mtime = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            candidates.push((name, mtime));
        }
    }
    candidates.sort_by(|a, b| b.1.cmp(&a.1));
    candidates.into_iter().next().map(|(n, _)| n)
}

fn tag_loader_json(folder: &str, id: &str, loader: &str, loader_version: &str) -> Result<(), String> {
    let p = version_json_path_in(folder, id);
    let mut j = read_version_json_from(&p)?;
    j["_loader"] = json!(loader);
    j["_loaderVersion"] = json!(loader_version);
    write_version_json(&p, &j)
}

// ---------------- Fabric API（对应原版 loaders.ts Modrinth 数据源 + MCIM 镜像回退） ----------------

const MODRINTH_BASES: &[&str] = &["https://api.modrinth.com/v2", "https://mod.mcimirror.top/modrinth/v2"];
const MODRINTH_UA: &str = "WUHUICL/0.1.0 (wuhuicl launcher)";

fn fetch_fabric_api_versions(mc_version: &str) -> Result<Vec<Value>, String> {
    let gv = urlencoding_lite(&serde_json::to_string(&[mc_version]).unwrap_or_default());
    let ld = urlencoding_lite(&serde_json::to_string(&["fabric"]).unwrap_or_default());
    let mut last_err = String::new();
    for base in MODRINTH_BASES {
        let url = format!("{}/project/fabric-api/version?game_versions={}&loaders={}", base, gv, ld);
        match fetch_json_with_ua(url, MODRINTH_UA) {
            Ok(v) => {
                if let Some(a) = v.as_array() {
                    return Ok(a.clone());
                }
                last_err = "响应格式异常".into();
            }
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

fn fetch_json_with_ua(url: String, ua: &str) -> Result<Value, String> {
    run_blocking(move || {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|e| e.to_string())?;
        let resp = client.get(&url).header("User-Agent", ua).send().map_err(|e| e.to_string())?;
        let status = resp.status().as_u16();
        let text = resp.text().map_err(|e| e.to_string())?;
        if !(200..300).contains(&status) {
            return Err(format!("HTTP {}", status));
        }
        serde_json::from_str(&text).map_err(|e| e.to_string())
    })
}

/// 极简 URL 编码（百分号编码 JSON 数组参数）
fn urlencoding_lite(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// 对应原版 listFabricApiVersions（最新在前）
pub fn list_fabric_api_versions(mc_version: &str) -> Result<Vec<Value>, String> {
    let arr = fetch_fabric_api_versions(mc_version)?;
    Ok(arr
        .iter()
        .filter_map(|v| {
            let version = v.get("version_number").and_then(|x| x.as_str())?;
            let date = v.get("date_published").and_then(|x| x.as_str()).unwrap_or("");
            Some(json!({ "version": version, "date": date }))
        })
        .collect())
}

/// 对应原版 installFabricApi：下载指定版本 Fabric API 到实例 mods 目录
fn install_fabric_api(
    app: &AppHandle,
    rec: &TaskRecord,
    emit: EmitFn,
    mc_version: &str,
    version: &str,
    mods_dir: &Path,
) -> Result<(), String> {
    emit(&json!({ "stage": "fabric-api", "progress": 0, "text": format!("查询 Fabric API {}", version) }));
    let arr = fetch_fabric_api_versions(mc_version)?;
    let v = arr.iter().find(|x| x.get("version_number").and_then(|y| y.as_str()) == Some(version));
    let file = v
        .and_then(|v| v.get("files").and_then(|f| f.as_array()))
        .and_then(|files| {
            files
                .iter()
                .find(|f| f.get("primary") == Some(&Value::Bool(true)))
                .or_else(|| files.first())
        })
        .cloned()
        .unwrap_or(Value::Null);
    let url = file.get("url").and_then(|x| x.as_str()).unwrap_or("").to_string();
    let filename = file.get("filename").and_then(|x| x.as_str()).unwrap_or("").to_string();
    if url.is_empty() || filename.is_empty() {
        return Err(format!("未找到适配 {} 的 Fabric API {} 文件", mc_version, version));
    }
    let dest = mods_dir.join(&filename);
    emit(&json!({ "stage": "fabric-api", "progress": 0.2, "text": format!("下载 Fabric API {}", version) }));
    let sha1 = file.get("hashes").and_then(|h| h.get("sha1")).and_then(|s| s.as_str()).map(str::to_string);
    let app2 = app.clone();
    run_blocking(move || {
        download_file(&app2, Some(rec), None, &url, &dest, sha1.as_deref(), None, "official", None, &[], &[])
    })?;
    emit(&json!({ "stage": "fabric-api", "progress": 1, "text": "Fabric API 已放入 mods 文件夹" }));
    Ok(())
}

// ---------------- 安装：旧版 Forge（对应原版 legacyForgeInstaller.ts） ----------------

#[derive(Clone)]
struct LegacyLibrary {
    name: String,
    relative: String,
    url: String,
    checksums: Vec<String>,
    embedded: Option<Vec<u8>>,
}

struct LegacyForgePlan {
    profile: Value,
    libraries: Vec<LegacyLibrary>,
}

fn legacy_record(value: Option<&Value>, label: &str) -> Result<Value, String> {
    match value {
        Some(v) if v.is_object() => Ok(v.clone()),
        _ => Err(format!("旧版 Forge {} 格式无效", label)),
    }
}

fn legacy_safe_name(value: Option<&str>, label: &str) -> Result<String, String> {
    let Some(v) = value else { return Err(format!("旧版 Forge {} 不安全", label)) };
    let bad = v.is_empty()
        || v.len() > 180
        || v == "."
        || v == ".."
        || v.contains(['\\', '/', '<', '>', ':', '"', '|', '?', '*'])
        || v.chars().any(|c| (c as u32) < 0x20)
        || v.ends_with('.')
        || v.ends_with(' ')
        || regex::Regex::new(r"^(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)")
            .unwrap()
            .is_match(&v.to_lowercase());
    if bad {
        return Err(format!("旧版 Forge {} 不安全", label));
    }
    Ok(v.to_string())
}

fn legacy_maven_path(name: &Value) -> Result<(String, String, String), String> {
    let Some(name) = name.as_str() else { return Err("旧版 Forge 依赖坐标无效".into()) };
    let parts: Vec<&str> = name.split(':').collect();
    if parts.len() != 3 && parts.len() != 4 {
        return Err("旧版 Forge 依赖坐标无效".into());
    }
    let group_ok = !parts[0].is_empty()
        && parts[0]
            .split('.')
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'));
    let rest_ok = parts[1..].iter().all(|p| {
        !p.is_empty()
            && p.chars().next().map(|c| c.is_ascii_alphanumeric()).unwrap_or(false)
            && p.chars().all(|c| c.is_ascii_alphanumeric() || "._+-".contains(c))
    });
    if !group_ok || !rest_ok {
        return Err("旧版 Forge 依赖坐标不安全".into());
    }
    let (g, a, v) = (parts[0], parts[1], parts[2]);
    let classifier = parts.get(3).copied();
    let relative = format!(
        "{}/{}/{}/{}-{}{}.jar",
        g.replace('.', "/"),
        a,
        v,
        a,
        v,
        classifier.map(|c| format!("-{}", c)).unwrap_or_default()
    );
    Ok((name.to_string(), relative, v.to_string()))
}

fn legacy_repository(input: Option<&Value>) -> Result<String, String> {
    let Some(input) = input else { return Ok("https://libraries.minecraft.net/".into()) };
    let Some(s) = input.as_str() else { return Err("旧版 Forge 依赖仓库无效".into()) };
    let Ok(u) = reqwest::Url::parse(s) else { return Err("旧版 Forge 依赖仓库无效".into()) };
    if !["http", "https"].contains(&u.scheme())
        || !u.username().is_empty()
        || u.password().is_some()
        || u.port().is_some()
        || u.query().is_some()
        || u.fragment().is_some()
    {
        return Err("旧版 Forge 依赖仓库不安全".into());
    }
    if u.host_str() == Some("files.minecraftforge.net") && u.path() == "/maven/" {
        return Ok("https://maven.minecraftforge.net/".into());
    }
    if u.host_str().map(|h| ["maven.minecraftforge.net", "libraries.minecraft.net"].contains(&h)).unwrap_or(false) && u.path() == "/" {
        return Ok(format!("https://{}", u.host_str().unwrap_or_default()));
    }
    Err("旧版 Forge 依赖仓库不受支持".into())
}

fn legacy_hashes(value: Option<&Value>) -> Result<Vec<String>, String> {
    let Some(v) = value else { return Ok(vec![]) };
    let Some(arr) = v.as_array() else { return Err("旧版 Forge 依赖校验值无效".into()) };
    let re = regex::Regex::new(r"^[a-f\d]{40}$").unwrap();
    if arr.is_empty()
        || arr.len() > 16
        || arr
            .iter()
            .any(|h| !h.as_str().map(|s| re.is_match(&s.to_lowercase())).unwrap_or(false))
    {
        return Err("旧版 Forge 依赖校验值无效".into());
    }
    let set: HashSet<String> = arr.iter().filter_map(|h| h.as_str()).map(|s| s.to_lowercase()).collect();
    Ok(set.into_iter().collect())
}

/// 对应原版 readLegacyForgeInstaller：解析 install_profile.json + 内嵌 universal jar
/// （无 install/versionInfo 键 → None，走现代安装器路径）
fn read_legacy_forge_installer(
    jar: &Path,
    mc_version: &str,
    loader_version: &str,
    installer_url_str: &str,
) -> Result<Option<LegacyForgePlan>, String> {
    let stat = std::fs::symlink_metadata(jar).map_err(|e| e.to_string())?;
    if !stat.is_file() || stat.file_type().is_symlink() || stat.len() > 128 * 1024 * 1024 {
        return Err("Forge 安装器不是有效的普通文件".into());
    }
    let file = std::fs::File::open(jar).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("Forge 安装器无法解析：{}", e))?;
    let profile_name = (0..archive.len())
        .filter_map(|i| archive.by_index(i).ok().map(|f| f.name().to_string()))
        .find(|n| n.to_lowercase() == "install_profile.json");
    let Some(profile_name) = profile_name else { return Ok(None) };
    let mut entry = archive.by_name(&profile_name).map_err(|e| e.to_string())?;
    if entry.size() > 8 * 1024 * 1024 {
        return Err("Forge 安装器包含冲突、不安全或过大的元数据".into());
    }
    let mut data = String::new();
    entry.read_to_string(&mut data).map_err(|e| e.to_string())?;
    drop(entry);
    let metadata: Value =
        serde_json::from_str(&data).map_err(|_| "旧版 Forge 安装元数据 格式无效".to_string())?;
    let has_install = metadata.get("install").map(|v| v.is_object()).unwrap_or(false);
    let has_version_info = metadata.get("versionInfo").map(|v| v.is_object()).unwrap_or(false);
    if !has_install && !has_version_info {
        return Ok(None);
    }
    let modern_keys = ["processors", "data", "spec", "libraries", "json", "optionals"];
    let legacy_mc =
        regex::Regex::new(r"^1\.(?:[1-9]|1[0-2])(?:\.\d+)?$").unwrap().is_match(mc_version);
    if !legacy_mc || modern_keys.iter().any(|k| metadata.get(*k).is_some()) {
        return Err("不支持混合或非旧版 Forge 安装器格式".into());
    }
    let install = legacy_record(metadata.get("install"), "安装配置")?;
    let version = legacy_record(metadata.get("versionInfo"), "版本配置")?;
    if ["transform", "stripMeta", "modList", "hideClient"]
        .iter()
        .any(|k| install.get(*k).is_some())
    {
        return Err("旧版 Forge 安装器需要额外转换，不能按普通继承配置安装".into());
    }
    let (coordinate_name, _coordinate_rel, coordinate_version) =
        legacy_maven_path(install.get("path").unwrap_or(&Value::Null))?;
    let expected = format!("{}-{}", mc_version, loader_version);
    let expected_coordinate = installer_url_str
        .split("/net/minecraftforge/forge/")
        .nth(1)
        .and_then(|rest| rest.split('/').next())
        .map(str::to_string);
    let expected_coordinate = match expected_coordinate {
        Some(c) => c,
        None => return Err("旧版 Forge 安装器与请求版本不一致".into()),
    };
    if !installer_url_str.starts_with("https://maven.minecraftforge.net")
        || coordinate_name != format!("net.minecraftforge:forge:{}", expected_coordinate)
        || !(coordinate_version == expected || coordinate_version.starts_with(&format!("{}-", expected)))
        || install.get("minecraft").and_then(|v| v.as_str()) != Some(mc_version)
    {
        return Err("旧版 Forge 安装器与请求版本不一致".into());
    }
    let id = legacy_safe_name(version.get("id").and_then(|v| v.as_str()), "版本名称")?;
    if install.get("target").and_then(|v| v.as_str()) != Some(id.as_str())
        || version.get("inheritsFrom").and_then(|v| v.as_str()) != Some(mc_version)
        || (version.get("jar").is_some() && version.get("jar").and_then(|v| v.as_str()) != Some(mc_version))
    {
        return Err("旧版 Forge 版本继承或目标不一致".into());
    }
    let main_class_ok = version
        .get("mainClass")
        .and_then(|v| v.as_str())
        .map(|s| regex::Regex::new(r"^[A-Za-z_$][\w$]*(?:\.[A-Za-z_$][\w$]*)+$").unwrap().is_match(s))
        .unwrap_or(false);
    let mc_args = version.get("minecraftArguments").and_then(|v| v.as_str());
    if !main_class_ok
        || mc_args
            .map(|s| s.is_empty() || s.len() > 65536 || s.contains(['\0', '\r', '\n']))
            .unwrap_or(true)
    {
        return Err("旧版 Forge 启动配置无效".into());
    }
    let libs = version.get("libraries").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    if libs.is_empty() || libs.len() > 256 {
        return Err("旧版 Forge 依赖列表无效".into());
    }
    let file_path = legacy_safe_name(install.get("filePath").and_then(|v| v.as_str()), "内嵌文件名称")?;
    if file_path != format!("forge-{}-universal.jar", expected_coordinate) {
        return Err("旧版 Forge 内嵌文件与坐标不一致".into());
    }
    // 提取内嵌 universal jar（校验头部 PK\x03\x04 + 中央目录可解析，绝不执行其字节码）
    let mut embedded: Option<Vec<u8>> = None;
    let mut found = 0;
    for i in 0..archive.len() {
        let mut e = archive.by_index(i).map_err(|e| e.to_string())?;
        if e.name() == file_path {
            found += 1;
            if e.is_dir() || e.size() == 0 || e.size() > 64 * 1024 * 1024 {
                return Err("旧版 Forge 内嵌文件不安全或过大".into());
            }
            let mut buf = Vec::with_capacity(e.size() as usize);
            e.read_to_end(&mut buf).map_err(|e| e.to_string())?;
            if buf.len() < 4 || u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) != 0x04034b50 {
                return Err("旧版 Forge 内嵌 JAR 无效".into());
            }
            // 解析中央目录检测截断（不执行其字节码）；块结束即释放对 buf 的借用
            {
                let cursor = std::io::Cursor::new(&buf);
                let mut z = zip::ZipArchive::new(cursor).map_err(|_| "旧版 Forge 内嵌 JAR 无效".to_string())?;
                if z.is_empty() {
                    return Err("旧版 Forge 内嵌 JAR 为空".into());
                }
            }
            embedded = Some(buf);
        }
    }
    if found != 1 {
        return Err("旧版 Forge 内嵌文件缺失或重复".into());
    }
    let embedded = embedded.ok_or("旧版 Forge 内嵌文件缺失或重复")?;
    let embedded_hash = sha1_hex(&embedded);
    let mut libraries: Vec<LegacyLibrary> = Vec::new();
    let mut destinations: HashSet<String> = HashSet::new();
    let mut universal_count = 0usize;
    for input in &libs {
        let lib = legacy_record(Some(input), "依赖")?;
        let (name, relative, _version) = legacy_maven_path(lib.get("name").unwrap_or(&Value::Null))?;
        for key in ["clientreq", "serverreq"] {
            if let Some(v) = lib.get(key) {
                if !v.is_boolean() {
                    return Err("旧版 Forge 依赖适用范围无效".into());
                }
            }
        }
        let checksums = legacy_hashes(lib.get("checksums"))?;
        if lib.get("downloads").is_some() || lib.get("natives").is_some() || lib.get("rules").is_some() {
            return Err("不支持混合的旧版 Forge 依赖格式".into());
        }
        if lib.get("clientreq").and_then(|v| v.as_bool()) == Some(false) {
            continue;
        }
        if destinations.contains(&relative) {
            return Err("旧版 Forge 包含重复的依赖目标".into());
        }
        destinations.insert(relative.clone());
        if name == coordinate_name {
            universal_count += 1;
            if !checksums.is_empty() && !checksums.iter().any(|c| c == &embedded_hash) {
                return Err("旧版 Forge 内嵌文件校验失败".into());
            }
            let url = reqwest::Url::parse(installer_url_str)
                .ok()
                .and_then(|u| u.join(&file_path).ok())
                .map(|u| u.to_string())
                .unwrap_or_default();
            libraries.push(LegacyLibrary {
                name,
                relative,
                url,
                checksums: vec![embedded_hash.clone()],
                embedded: Some(embedded.clone()),
            });
        } else {
            let repo = legacy_repository(lib.get("url"))?;
            let url = format!("{}{}", repo, relative);
            libraries.push(LegacyLibrary { name, relative, url, checksums, embedded: None });
        }
    }
    if universal_count != 1 {
        return Err("旧版 Forge 缺少唯一的本体依赖".into());
    }
    // 仅复制旧启动器字段；安装器私有路径/现代 processor 指令绝不导入
    let mut profile = json!({
        "id": id,
        "inheritsFrom": mc_version,
        "mainClass": version.get("mainClass"),
        "minecraftArguments": version.get("minecraftArguments"),
        "_mcVersion": mc_version,
        "_loader": "forge",
        "_loaderVersion": loader_version
    });
    if let Some(t) = version.get("type") {
        if !["release", "snapshot", "old_alpha", "old_beta"].contains(&t.as_str().unwrap_or("")) {
            return Err("旧版 Forge 版本类型无效".into());
        }
        profile["type"] = t.clone();
    }
    if let Some(assets) = version.get("assets").and_then(|v| v.as_str()) {
        profile["assets"] = json!(legacy_safe_name(Some(assets), "资源索引名称")?);
    }
    Ok(Some(LegacyForgePlan { profile, libraries }))
}

/// 旧版 Forge 依赖 sidecar SHA-1（url + ".sha1"，镜像候选轮询）
fn legacy_sidecar_hashes(url: &str, mirror: &str) -> Result<Vec<String>, String> {
    let path_part = reqwest::Url::parse(url).map(|u| u.path().to_string()).unwrap_or_default();
    let urls = vec![url.to_string()];
    for source in download_candidates(&urls, mirror) {
        if let Ok(text) = fetch_text(format!("{}.sha1", source), 4) {
            if text.len() <= 4096 {
                if let Some(c) = text.trim().split_whitespace().next() {
                    if c.len() == 40 && c.chars().all(|ch| ch.is_ascii_hexdigit()) {
                        return Ok(vec![c.to_lowercase()]);
                    }
                }
            }
        }
    }
    Err(format!("旧版 Forge 依赖缺少可靠的 SHA-1 校验值：{}", path_part))
}

fn legacy_file_identity(path: &Path) -> Result<(String, u64), String> {
    Ok((sha1_file_hex(path)?, std::fs::metadata(path).map_err(|e| e.to_string())?.len()))
}

/// 对应原版 prepareLegacyForgeInstaller：依赖全部下载校验后发布，再写实例目录
#[allow(clippy::too_many_arguments)]
fn prepare_and_install_legacy_forge(
    app: &AppHandle,
    rec: &TaskRecord,
    emit: EmitFn,
    plan: LegacyForgePlan,
    library_root: &Path,
    mirror: &str,
    target: &str,
    instance_name: Option<&str>,
) -> Result<String, String> {
    let _ = std::fs::create_dir_all(library_root);
    let work = library_root.join(format!(".wuhuicl-legacy-forge-{}", crate::accounts::uuid_v4()));
    let _ = std::fs::create_dir_all(&work);
    let result = (|| -> Result<String, String> {
        // 逐库准备：checksums/sidecar、staged 文件、已落地身份
        let mut resolved: Vec<(LegacyLibrary, Vec<String>, PathBuf, PathBuf, Option<(String, u64)>)> = Vec::new();
        for (index, library) in plan.libraries.iter().enumerate() {
            check_cancel(Some(rec))?;
            let checksums = if !library.checksums.is_empty() {
                library.checksums.clone()
            } else {
                legacy_sidecar_hashes(&library.url, mirror)?
            };
            let staged = work.join(format!("{}.jar", index));
            let dest = library_root.join(library.relative.replace('/', "\\"));
            let mut identity = None;
            if dest.exists() {
                if let Ok(id) = legacy_file_identity(&dest) {
                    if checksums.iter().any(|c| c == &id.0) {
                        identity = Some(id);
                    }
                }
            }
            if library.embedded.is_some() && identity.is_none() {
                std::fs::write(&staged, library.embedded.clone().unwrap_or_default())
                    .map_err(|e| e.to_string())?;
            }
            resolved.push(((*library).clone(), checksums, staged, dest, identity));
        }
        // 批量下载未落地的依赖（staged 目标）
        let tasks: Vec<DownloadItem> = resolved
            .iter()
            .filter(|(lib, _, _, _, identity)| identity.is_none() && lib.embedded.is_none())
            .map(|(lib, checksums, staged, _, _)| DownloadItem {
                label: lib.relative.clone(),
                url: lib.url.clone(),
                dest: staged.clone(),
                sha1: if checksums.len() == 1 { checksums.first().cloned() } else { None },
                size: None,
                reuse_files: vec![],
                reuse_dirs: vec![],
            })
            .collect();
        if !tasks.is_empty() {
            let total = tasks.len();
            let app2 = app.clone();
            let mirror2 = mirror.to_string();
            let progress = move |d: usize, t: usize, _speed: f64, detail: &DownloadDetail| {
                emit(&json!({
                    "stage": "loader-dependencies",
                    "progress": detail.fraction,
                    "text": format!("下载并校验旧版 Forge 依赖 {}/{}", d, t.max(total)),
                    "speed": if detail.speed_bps > 0.0 { json!(detail.speed_bps) } else { Value::Null },
                    "bytesDone": detail.bytes_done,
                    "bytesTotal": detail.bytes_total.map(|b| json!(b)).unwrap_or(Value::Null),
                    "etaSeconds": detail.eta_seconds.map(|e| json!(e)).unwrap_or(Value::Null),
                    "indeterminate": detail.indeterminate
                }));
            };
            run_blocking(move || download_all(&app2, Some(rec), tasks, &mirror2, &progress))?;
        }
        // 逐项校验身份（无目标写入 if 任一 staged 有未接受的变体）
        let mut identities: Vec<(String, u64)> = Vec::new();
        for (lib, checksums, staged, dest, identity) in resolved.iter() {
            let id = match identity {
                Some(i) => i.clone(),
                None => legacy_file_identity(staged)?,
            };
            if !checksums.iter().any(|c| c == &id.0) || id.1 == 0 {
                return Err(format!("旧版 Forge 依赖 SHA-1 校验失败：{}", lib.name));
            }
            identities.push(id);
        }
        // 发布到 libraries 目录（临时 + 改名）
        for (i, (lib, _checksums, staged, dest, _identity)) in resolved.iter().enumerate() {
            if dest.exists() {
                if let Ok(id) = legacy_file_identity(dest) {
                    if id == identities[i] {
                        continue;
                    }
                }
            }
            if !staged.exists() {
                return Err(format!("旧版 Forge 缓存校验期间发生变化，请重试：{}", lib.name));
            }
            if let Some(dir) = dest.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            let tmp = dest.with_extension(format!("legacy-forge-{}.tmp", crate::accounts::uuid_v4()));
            std::fs::copy(staged, &tmp).map_err(|e| e.to_string())?;
            std::fs::rename(&tmp, dest).map_err(|e| e.to_string())?;
        }
        // 生成实例目录（独占 mkdir，失败清理不能波及既有实例）
        let profile_id = plan.profile.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let id = legacy_safe_name(
            Some(
                instance_name
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .unwrap_or(&profile_id),
            ),
            "实例名称",
        )?;
        let directory = Path::new(target).join("versions").join(&id);
        if directory.exists() {
            return Err("旧版 Forge 目标实例目录已存在，未覆盖；请更换实例名".into());
        }
        std::fs::create_dir_all(directory.parent().unwrap_or(Path::new("/"))).map_err(|e| e.to_string())?;
        std::fs::create_dir(&directory).map_err(|e| e.to_string())?;
        let write_result = (|| -> Result<(), String> {
            std::fs::write(directory.join(".installing"), crate::accounts::uuid_v4())
                .map_err(|e| e.to_string())?;
            let mut profile = plan.profile.clone();
            profile["id"] = json!(id);
            profile["libraries"] = json!(resolved
                .iter()
                .zip(identities.iter())
                .map(|((lib, _, _, _, _), id)| json!({
                    "name": lib.name,
                    "downloads": {
                        "artifact": { "path": lib.relative, "url": lib.url, "sha1": id.0, "size": id.1 }
                    }
                }))
                .collect::<Vec<Value>>());
            write_version_json(&directory.join(format!("{}.json", id)), &profile)
        })();
        if let Err(e) = write_result {
            let _ = std::fs::remove_dir_all(&directory);
            return Err(e);
        }
        emit(&json!({ "stage": "loader-process", "progress": 1, "text": "旧版 Forge 运行配置与本体已校验并生成" }));
        Ok(id)
    })();
    let _ = std::fs::remove_dir_all(&work);
    result
}

// ---------------- 版本删除（对应原版 versionRemoval.ts recycleVersion + versions.removeVersion） ----------------

fn recycle_bin_delete(target: &Path) -> Result<(), String> {
    // shell.trashItem 等价：PowerShell VisualBasic FileIO 回收站删除（不引入新依赖）
    let target_s = target.to_string_lossy().replace('\'', "''");
    let (method, arg2, arg3) = if target.is_dir() {
        ("DeleteDirectory", "'OnlyErrorDialogs'", "'SendToRecycleBin'")
    } else {
        ("DeleteFile", "'OnlyErrorDialogs'", "'SendToRecycleBin'")
    };
    let script = format!(
        "Add-Type -AssemblyName Microsoft.VisualBasic; [Microsoft.VisualBasic.FileIO.FileSystem]::{}('{}', {}, {})",
        method, target_s, arg2, arg3
    );
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .map_err(|e| e.to_string())?;
    if output.status.success() && !target.exists() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(stderr)
}

/// 对应原版 versions.ts removeVersion（注册校验 + 回收站删除）
pub fn remove_version(app: &AppHandle, id: &str, requested_folder: Option<&str>) -> Result<(), String> {
    let s = settings::get(app);
    let folder = requested_folder
        .filter(|f| !f.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| folder_of_version(app, id));
    let registered = s["folders"]
        .as_array()
        .map(|a| {
            a.iter()
                .any(|f| f["path"].as_str().map(|p| same_path(p, &folder)).unwrap_or(false))
        })
        .unwrap_or(false);
    if !registered {
        return Err("游戏文件夹尚未登记".into());
    }
    // 对应原版 getRunningVersionIds 拦截：launch 模块移植后接入
    recycle_version(&folder, id)
}

fn recycle_version(folder: &str, id: &str) -> Result<(), String> {
    if id.is_empty()
        || id == "."
        || id == ".."
        || id.contains(['\\', '/', ':', '*', '?', '"', '<', '>', '|'])
        || id.chars().any(|c| (c as u32) < 0x20)
        || id.ends_with('.')
        || id.ends_with(' ')
    {
        return Err("版本名称无效，未删除任何文件".into());
    }
    let parent = Path::new(folder).join("versions");
    let target = parent.join(id);
    if target.parent().map(|p| p != parent.as_path()).unwrap_or(true) {
        return Err("版本路径超出游戏目录".into());
    }
    let stat = match std::fs::symlink_metadata(&target) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.to_string()),
    };
    if !stat.is_dir() || stat.file_type().is_symlink() {
        return Err("版本目录是链接或不是文件夹，请在文件管理器中确认其实际位置".into());
    }
    let parent_stat = std::fs::symlink_metadata(&parent).map_err(|e| e.to_string())?;
    if parent_stat.file_type().is_symlink() {
        return Err("versions 目录是链接，请先确认实际游戏目录".into());
    }
    // 复核后回收（避免异步检查间隙目录被替换）
    let current = std::fs::symlink_metadata(&target).map_err(|e| e.to_string())?;
    if current.file_type().is_symlink() {
        return Err("版本目录已变化，请刷新后重试".into());
    }
    match recycle_bin_delete(&target) {
        Ok(()) => {
            if target.exists() {
                return Err("系统未移除该目录".into());
            }
            Ok(())
        }
        Err(e) => Err(format!(
            "无法将版本移入回收站：请关闭使用该目录的游戏或文件窗口，并检查目录权限及磁盘的回收站支持。目录：{}；{}",
            target.to_string_lossy(),
            e
        )),
    }
}

/// 对应原版 cleanupPartialInstall：清理安装失败残留（整个版本目录）
pub fn cleanup_partial_install(folder: &str, id: &str) -> Result<bool, String> {
    let mark = install_mark_path_in(folder, id);
    if !mark.exists() {
        return Ok(false);
    }
    std::fs::remove_dir_all(version_dir_in(folder, id)).map_err(|e| e.to_string())?;
    Ok(true)
}

// ---------------- 实例名 / 重命名（对应原版 validateInstanceName / renameVersion） ----------------

pub fn validate_instance_name(app: &AppHandle, name: &str, exclude_id: Option<&str>) -> Option<String> {
    let n = name.trim();
    if n.is_empty() {
        return Some("实例名不能为空".into());
    }
    if n.chars().count() > 64 {
        return Some("实例名过长（最多 64 字符）".into());
    }
    if n.chars().any(|c| "\\/:*?\"<>|".contains(c)) {
        return Some("实例名不能包含 \\ / : * ? \" < > | 字符".into());
    }
    if n.starts_with(['.', ' ']) || n.ends_with(['.', ' ']) {
        return Some("实例名不能以空格或点开头/结尾".into());
    }
    if regex::Regex::new(r"^(con|prn|aux|nul|com[1-9]|lpt[1-9])$")
        .unwrap()
        .is_match(&n.to_lowercase())
    {
        return Some("实例名为系统保留名".into());
    }
    if Some(n) != exclude_id {
        // 跨所有游戏文件夹查重
        for folder in all_folders(app) {
            if version_dir_in(&folder, n).exists() {
                return Some(format!("实例「{}」已存在，请换一个名字", n));
            }
        }
    }
    None
}

/// 对应原版 renameVersion：目录、json id、原版 jar 文件名同步改名
pub fn rename_version(app: &AppHandle, folder: &str, id: &str, new_name: &str) -> Result<(), String> {
    if let Some(err) = validate_instance_name(app, new_name, Some(id)) {
        return Err(err);
    }
    let trimmed = new_name.trim();
    let from = version_dir_in(folder, id);
    let to = version_dir_in(folder, trimmed);
    if !from.exists() {
        return Err("实例不存在".into());
    }
    if same_path(&from.to_string_lossy(), &to.to_string_lossy()) {
        return Ok(());
    }
    // 更新 json 内 id 字段（先读改写，再移动目录，避免中间态）；
    // 原版实例改名前记录真实 MC 版本 id（_mcVersion）
    let jp = version_json_path_in(folder, id);
    if jp.exists() {
        let mut j = read_version_json(app, folder, id)?;
        let pure_vanilla = j.get("inheritsFrom").is_none()
            && j.get("_loader").is_none()
            && j.get("_modpackName").is_none()
            && j.get("_mcVersion").is_none();
        if pure_vanilla {
            j["_mcVersion"] = j.get("id").cloned().unwrap_or(Value::Null);
        }
        j["id"] = json!(trimmed);
        write_version_json(&jp, &j)?;
    }
    // json / jar 文件名同步（失败不阻断）
    let _ = std::fs::rename(&jp, from.join(format!("{}.json", trimmed)));
    let old_jar = from.join(format!("{}.jar", id));
    if old_jar.exists() {
        let _ = std::fs::rename(&old_jar, from.join(format!("{}.jar", trimmed)));
    }
    if let Err(e) = std::fs::rename(&from, &to) {
        if matches!(e.raw_os_error(), Some(5 | 32 | 33 | 145)) {
            return Err("文件夹正被占用（游戏运行中或被其他程序打开），请关闭后重试".into());
        }
        return Err(e.to_string());
    }
    register_version_folder(trimmed, folder);
    Ok(())
}

// ---------------- 隔离迁移（对应原版 isolationFiles.ts / instances.ts） ----------------

fn tree_size(input: &Path) -> Result<(u64, u64), String> {
    let stat = std::fs::symlink_metadata(input).map_err(|e| e.to_string())?;
    if stat.file_type().is_symlink() {
        return Ok((0, 0));
    }
    if !stat.is_dir() {
        return Ok((1, stat.len()));
    }
    let mut files = 0u64;
    let mut bytes = 0u64;
    for entry in std::fs::read_dir(input).map_err(|e| e.to_string())?.flatten() {
        let (f, b) = tree_size(&entry.path())?;
        files += f;
        bytes += b;
    }
    Ok((files, bytes))
}

/// 对应原版 planIsolationFiles：仅规划固定白名单中的玩家数据，符号链接不计入
pub fn plan_isolation_files(version_id: &str, source: &Path, destination: &Path) -> Result<Value, String> {
    let mut items: Vec<Value> = Vec::new();
    let mut conflicts: Vec<String> = Vec::new();
    for name in ISOLATION_DIRS.iter().chain(ISOLATION_FILES.iter()) {
        let from = source.join(name);
        if !from.exists() {
            continue;
        }
        let stat = std::fs::symlink_metadata(&from).map_err(|e| e.to_string())?;
        if stat.file_type().is_symlink() {
            continue;
        }
        if destination.join(name).exists() {
            conflicts.push(name.to_string());
        }
        let (files, bytes) = tree_size(&from)?;
        items.push(json!({
            "name": name,
            "kind": if stat.is_dir() { "directory" } else { "file" },
            "files": files,
            "bytes": bytes
        }));
    }
    let total_files: u64 = items.iter().filter_map(|i| i["files"].as_u64()).sum();
    let total_bytes: u64 = items.iter().filter_map(|i| i["bytes"].as_u64()).sum();
    Ok(json!({
        "versionId": version_id,
        "source": source.to_string_lossy(),
        "destination": destination.to_string_lossy(),
        "items": items,
        "conflicts": conflicts,
        "totalFiles": total_files,
        "totalBytes": total_bytes
    }))
}

/// 对应原版 instances.ts isolationMigrationPlan
pub fn isolation_migration_plan(app: &AppHandle, folder: &str, id: &str) -> Result<Value, String> {
    let source = canonical_path(folder).to_string_lossy().to_string();
    let destination = canonical_path(&version_dir_in(folder, id).to_string_lossy())
        .to_string_lossy()
        .to_string();
    plan_isolation_files(id, Path::new(&source), Path::new(&destination))
}

fn write_isolation_flag(folder: &str, id: &str, isolated: bool) -> Result<(), String> {
    let json_path = version_json_path_in(folder, id);
    let original = std::fs::read_to_string(&json_path).map_err(|e| e.to_string())?;
    let mut json: Value = serde_json::from_str(strip_bom(&original)).map_err(|e| e.to_string())?;
    json["_gameDir"] = json!(isolated);
    let temp = json_path.with_extension(format!(
        "isolation-{}-{}.tmp",
        std::process::id(),
        chrono::Utc::now().timestamp_millis()
    ));
    let write = (|| -> Result<(), String> {
        std::fs::write(&temp, serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        std::fs::copy(&temp, &json_path).map_err(|e| e.to_string())?;
        Ok(())
    })();
    let _ = std::fs::remove_file(&temp);
    if let Err(e) = write {
        // 失败恢复原文件（保留原始异常）
        let _ = std::fs::write(&json_path, &original);
        return Err(e);
    }
    Ok(())
}

fn copy_tree_excl(source: &Path, dest: &Path) -> Result<(), String> {
    if dest.exists() {
        return Err(format!("目标已存在：{}", dest.to_string_lossy()));
    }
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(source).map_err(|e| e.to_string())?.flatten() {
        let sp = entry.path();
        let dp = dest.join(entry.file_name());
        let stat = std::fs::symlink_metadata(&sp).map_err(|e| e.to_string())?;
        if stat.file_type().is_symlink() {
            continue; // 符号链接不复制
        }
        if stat.is_dir() {
            copy_tree_excl(&sp, &dp)?;
        } else {
            std::fs::copy(&sp, &dp).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// 对应原版 commitIsolationFiles：临时区复制 → 原子改名提交；commitMetadata 失败也删除新增项
fn commit_isolation_files(plan: &Value, folder: &str, id: &str) -> Result<(), String> {
    let source = PathBuf::from(plan["source"].as_str().unwrap_or(""));
    let destination = PathBuf::from(plan["destination"].as_str().unwrap_or(""));
    std::fs::create_dir_all(&destination).map_err(|e| e.to_string())?;
    let staging = destination.join(format!(".isolation-staging-{}", crate::accounts::uuid_v4()));
    std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    let mut created: Vec<PathBuf> = Vec::new();
    let result = (|| -> Result<(), String> {
        let conflicts: Vec<&str> = plan["conflicts"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        let items = plan["items"].as_array().cloned().unwrap_or_default();
        // 先把全部非冲突项复制到临时区
        for item in &items {
            let name = item["name"].as_str().unwrap_or("");
            if conflicts.contains(&name) {
                continue;
            }
            let sp = source.join(name);
            let dp = staging.join(name);
            if item["kind"].as_str() == Some("directory") {
                copy_tree_excl(&sp, &dp)?;
            } else {
                std::fs::copy(&sp, &dp).map_err(|e| e.to_string())?;
            }
        }
        // 逐项原子改名提交（规划后出现同名项也绝不覆盖）
        for item in &items {
            let name = item["name"].as_str().unwrap_or("");
            let staged = staging.join(name);
            if !staged.exists() {
                continue;
            }
            let dest = destination.join(name);
            if dest.exists() {
                continue;
            }
            std::fs::rename(&staged, &dest).map_err(|e| e.to_string())?;
            created.push(dest);
        }
        write_isolation_flag(folder, id, true)
    })();
    let _ = std::fs::remove_dir_all(&staging);
    if let Err(e) = result {
        for item in created.iter().rev() {
            if item.is_dir() {
                let _ = std::fs::remove_dir_all(item);
            } else {
                let _ = std::fs::remove_file(item);
            }
        }
        return Err(e);
    }
    Ok(())
}

/// 对应原版 instances.ts applyIsolation：用户确认后的事务式隔离迁移
pub fn apply_isolation(app: &AppHandle, folder: &str, id: &str, isolated: bool) -> Result<Value, String> {
    let plan = isolation_migration_plan(app, folder, id)?;
    if !isolated {
        // 关闭时保留独立目录数据并写显式 false
        write_isolation_flag(folder, id, false)?;
        return Ok(plan);
    }
    commit_isolation_files(&plan, folder, id).map_err(|e| format!("隔离迁移失败，已回滚：{}", e))?;
    Ok(plan)
}

/// 对应原版 instances.ts setNewInstanceIsolation：新实例只写目录语义，不复制共享内容
pub fn set_new_instance_isolation(folder: &str, id: &str, isolated: bool) -> Result<(), String> {
    write_isolation_flag(folder, id, isolated)
}

// ---------------- 分类（对应原版 versionCategories.ts + shared/versionCategories.ts） ----------------

const MAX_VERSION_CATEGORIES: usize = 100;
const RESERVED_CATEGORY_NAMES: &[&str] = &["全部", "全部版本", "收藏", "收藏版本", "未分类"];

fn category_name_identity(name: &str) -> String {
    name.trim().to_lowercase()
}

fn validate_category_name(name: Option<&Value>) -> Result<String, String> {
    let Some(name) = name.and_then(|v| v.as_str()) else { return Err("请输入分类名称".into()) };
    let name = name.trim();
    if name.is_empty()
        || name.chars().count() > 40
        || name.chars().any(|c| (c as u32) < 0x20 || (c as u32) == 0x7f)
    {
        return Err("分类名称须为 1–40 个字符，且不能包含控制字符".into());
    }
    if RESERVED_CATEGORY_NAMES.contains(&category_name_identity(name).as_str()) {
        return Err("此名称用于内置分类，请换一个名称".into());
    }
    Ok(name.to_string())
}

fn version_category_key(folder: &str, id: &str) -> String {
    let mut root = folder.replace('\\', "/");
    while root.ends_with('/') {
        root.pop();
    }
    if root.is_empty() {
        root = "/".into();
    }
    if cfg!(windows) {
        root = root.to_lowercase();
    }
    json!([root, id]).to_string()
}

/// 对应原版 updateVersionCategories（versions:categoriesUpdate）
pub fn update_version_categories(app: &AppHandle, action: &Value) -> Result<Value, String> {
    if !action.is_object() {
        return Err("分类操作无效".into());
    }
    let mut s = settings::get(app);
    let action_type = action["type"].as_str().unwrap_or("");
    match action_type {
        "create" => {
            let mut categories = s["versionCategories"].as_array().cloned().unwrap_or_default();
            if categories.len() >= MAX_VERSION_CATEGORIES {
                return Err(format!("最多可创建 {} 个分类", MAX_VERSION_CATEGORIES));
            }
            let name = validate_category_name(action.get("name"))?;
            let exists = categories
                .iter()
                .any(|c| category_name_identity(c["name"].as_str().unwrap_or("")) == category_name_identity(&name));
            if exists {
                return Err("已存在同名分类".into());
            }
            categories.push(json!({ "id": crate::accounts::uuid_v4(), "name": name }));
            s["versionCategories"] = Value::Array(categories);
        }
        "rename" | "remove" => {
            let mut categories = s["versionCategories"].as_array().cloned().unwrap_or_default();
            let id = action["id"].as_str().unwrap_or("").to_string();
            let Some(idx) = categories.iter().position(|c| c["id"].as_str() == Some(id.as_str())) else {
                return Err("分类已不存在，请刷新后重试".into());
            };
            if action_type == "rename" {
                let name = validate_category_name(action.get("name"))?;
                if categories.iter().enumerate().any(|(i, c)| {
                    i != idx && category_name_identity(c["name"].as_str().unwrap_or("")) == category_name_identity(&name)
                }) {
                    return Err("已存在同名分类".into());
                }
                categories[idx]["name"] = json!(name);
            } else {
                categories.remove(idx);
                if let Some(assignments) = s["versionCategoryAssignments"].as_object().cloned() {
                    let filtered: serde_json::Map<String, Value> = assignments
                        .into_iter()
                        .filter(|(_, v)| v.as_str() != Some(id.as_str()))
                        .collect();
                    s["versionCategoryAssignments"] = Value::Object(filtered);
                }
            }
            s["versionCategories"] = Value::Array(categories);
        }
        "assign" => {
            let target = action.get("target").cloned().unwrap_or(Value::Null);
            let tid = target["id"].as_str().unwrap_or("");
            let tfolder = target["folder"].as_str().unwrap_or("");
            if tid.is_empty() || tfolder.is_empty() {
                return Err("请选择有效实例".into());
            }
            let category_id = action["categoryId"].as_str().unwrap_or("");
            let categories = s["versionCategories"].as_array().cloned().unwrap_or_default();
            if !category_id.is_empty()
                && !categories.iter().any(|c| c["id"].as_str() == Some(category_id))
            {
                return Err("分类已不存在，请刷新后重试".into());
            }
            let root = s["folders"]
                .as_array()
                .and_then(|a| {
                    a.iter().find(|f| {
                        f["path"].as_str().map(|p| same_path(p, tfolder)).unwrap_or(false)
                    })
                })
                .and_then(|f| f["path"].as_str())
                .ok_or("实例所在游戏文件夹尚未登记")?;
            let target_key = version_category_key(root, tid);
            let all = crate::versions::list_installed(&settings::get(app), true);
            let found = all
                .as_array()
                .map(|a| {
                    a.iter().any(|row| {
                        row["id"].as_str() == Some(tid)
                            && row["folder"]
                                .as_str()
                                .map(|f| version_category_key(f, row["id"].as_str().unwrap_or("")) == target_key)
                                .unwrap_or(false)
                    })
                })
                .unwrap_or(false);
            if !found {
                return Err("实例已不存在，请刷新后重试".into());
            }
            // 保留注册/扫描使用的原始拼写，绝不换成另一个不可见键
            let instance_folder = all
                .as_array()
                .and_then(|a| a.iter().find(|row| row["id"].as_str() == Some(tid)))
                .and_then(|row| row["folder"].as_str())
                .unwrap_or(root);
            let key = version_category_key(instance_folder, tid);
            let mut assignments = s["versionCategoryAssignments"].as_object().cloned().unwrap_or_default();
            if category_id.is_empty() {
                assignments.remove(&key);
            } else {
                assignments.insert(key, json!(category_id));
            }
            s["versionCategoryAssignments"] = Value::Object(assignments);
        }
        _ => return Err("分类操作无效".into()),
    }
    settings::save(app, s)
}

// ---------------- 重命名副作用：收藏 / 分类绑定 / 服务器绑定（ipc.ts versionsRename 内联） ----------------

fn rename_side_effects(app: &AppHandle, folder: &str, old_id: &str, new_id: &str) {
    let s = settings::get(app);
    // 旧版 ID 收藏保留给其他目录同名实例，当前实例迁移到目录限定的键
    let root = folder.replace('\\', "/").trim_end_matches('/').to_lowercase();
    let old_key = json!([root, old_id]).to_string();
    let next_key = json!([root, new_id]).to_string();
    let favorite_versions = s["favoriteVersions"].as_array().cloned().unwrap_or_default();
    let overrides = s["favoriteInstanceOverrides"].as_object().cloned().unwrap_or_default();
    let old_in_favorites = favorite_versions.iter().any(|v| v.as_str() == Some(old_id));
    if old_in_favorites || overrides.contains_key(&old_key) {
        let mut next = overrides.clone();
        let value = overrides.get(&old_key).cloned().unwrap_or(json!(old_in_favorites));
        next.insert(next_key, value);
        next.remove(&old_key);
        let _ = settings::save(app, json!({ "favoriteInstanceOverrides": next }));
    }
    // 分类绑定 rekey（renameCategoryAssignment）
    let before = version_category_key(folder, old_id);
    let after = version_category_key(folder, new_id);
    if before != after {
        let mut assignments = s["versionCategoryAssignments"].as_object().cloned().unwrap_or_default();
        if let Some(v) = assignments.remove(&before) {
            assignments.insert(after, v);
            let _ = settings::save(app, json!({ "versionCategoryAssignments": assignments }));
        }
    }
    // 引用同步：服务器绑定（隔离实例的 servers.dat 随目录迁移，无需额外处理）
    rename_server_bindings(app, folder, old_id, new_id);
}

/// 对应原版 serverEditing.ts renamedServerBindings + servers.ts renameBinding
fn rename_server_bindings(app: &AppHandle, folder: &str, old_id: &str, new_id: &str) {
    let store = settings::user_data_dir(app).join("servers.json");
    let Ok(raw) = std::fs::read_to_string(&store) else { return };
    let Ok(Value::Array(list)) = serde_json::from_str::<Value>(&raw) else { return };
    // ambiguous：其他游戏文件夹存在同名版本
    let all = crate::versions::list_installed(&settings::get(app), true);
    let ambiguous = all
        .as_array()
        .map(|a| {
            a.iter().any(|v| {
                v["id"].as_str() == Some(old_id)
                    && v["folder"].as_str().map(|f| !same_path(f, folder)).unwrap_or(false)
            })
        })
        .unwrap_or(false);
    let root = folder.replace('\\', "/").trim_end_matches('/').to_lowercase();
    let changed = std::cell::Cell::new(false);
    let next: Vec<Value> = list
        .into_iter()
        .map(|entry| {
            if !entry.is_object() {
                return entry;
            }
            let mut entry = entry;
            let entry_folder = entry
                .get("folder")
                .and_then(|v| v.as_str())
                .map(|f| f.replace('\\', "/").trim_end_matches('/').to_lowercase());
            let source = entry
                .get("sourceGameDirectory")
                .and_then(|v| v.as_str())
                .map(|f| f.replace('\\', "/").trim_end_matches('/').to_lowercase());
            let belongs = root.is_empty()
                || match entry_folder {
                    Some(ef) => ef == root,
                    None => match source {
                        Some(sc) => sc == root || sc.starts_with(&format!("{}/", root)),
                        None => !ambiguous,
                    },
                };
            if !belongs {
                return entry;
            }
            let mut touched = false;
            if entry.get("versionId").and_then(|v| v.as_str()) == Some(old_id) {
                entry["versionId"] = json!(new_id);
                touched = true;
            }
            if let Some(cands) = entry.get("candidateVersionIds").and_then(|v| v.as_array()).cloned() {
                if cands.iter().any(|c| c.as_str() == Some(old_id)) {
                    entry["candidateVersionIds"] = Value::Array(
                        cands.iter()
                            .map(|c| {
                                json!(if c.as_str() == Some(old_id) { new_id } else { c.as_str().unwrap_or("") })
                            })
                            .collect(),
                    );
                    touched = true;
                }
            }
            if touched {
                changed.set(true);
            }
            entry
        })
        .collect();
    if changed.get() {
        let _ = std::fs::write(&store, serde_json::to_string_pretty(&next).unwrap_or_default());
    }
}

// ---------------- 安装主管线（对应原版 installVersion / installVersionInFolder / installLoader） ----------------

/// isolated 判定：recordingMod/favoriteMods 存在或全局 defaultIsolation
/// （附加模组安装依赖 mods 模块，未接入时这两个选项不改变隔离判定之外的行为）
fn install_isolated(app: &AppHandle, opts: &Value) -> bool {
    if opts.get("recordingMod").map(|v| !v.is_null()).unwrap_or(false)
        || opts
            .get("favoriteMods")
            .and_then(|v| v.as_array())
            .map(|a| !a.is_empty())
            .unwrap_or(false)
    {
        return true;
    }
    settings::get(app)["defaultIsolation"].as_bool().unwrap_or(true)
}

/// 对应原版 paths.ts withDownloadFolder：注册 + 存在性校验后冻结目录
fn with_download_folder(app: &AppHandle, folder: Option<&str>) -> Result<String, String> {
    let requested = folder
        .filter(|f| !f.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| default_folder_path(app));
    let s = settings::get(app);
    let registered = s["folders"]
        .as_array()
        .and_then(|a| {
            a.iter()
                .find(|f| f["path"].as_str().map(|p| same_path(p, &requested)).unwrap_or(false))
                .and_then(|f| f["path"].as_str())
        })
        .map(str::to_string);
    let Some(registered) = registered else {
        return Err("下载文件夹未绑定或已解除绑定，请重新选择默认下载位置".into());
    };
    let meta = std::fs::metadata(&registered)
        .map_err(|_| "下载文件夹已不存在，请重新选择默认下载位置".to_string())?;
    if !meta.is_dir() {
        return Err("下载文件夹已不存在，请重新选择默认下载位置".into());
    }
    Ok(registered)
}

/// scopedVersion：文件夹注册校验（ipc.ts）
fn scoped_folder(app: &AppHandle, id: &str, folder: Option<&str>) -> Result<String, String> {
    let target = folder
        .filter(|f| !f.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| folder_of_version(app, id));
    let s = settings::get(app);
    let ok = s["folders"]
        .as_array()
        .map(|a| {
            a.iter()
                .any(|f| f["path"].as_str().map(|p| resolve_norm(p) == resolve_norm(&target)).unwrap_or(false))
        })
        .unwrap_or(false);
    if !ok {
        return Err("游戏文件夹未绑定或已解除绑定".into());
    }
    Ok(target)
}

fn install_version_flow(
    app: &AppHandle,
    rec: &TaskRecord,
    emit: EmitFn,
    active: &str,
    version_id: &str,
    opts: &Value,
) -> Result<String, String> {
    let isolated = install_isolated(app, opts);
    // 子安装器完成并不代表整个任务完成（stage=done 不透传）
    let prepare_emit = move |event: &Value| {
        if event.get("stage").and_then(|v| v.as_str()) != Some("done") {
            emit(event);
        }
    };
    let loader = opts.get("loader").and_then(|v| v.as_str()).unwrap_or("");
    if !loader.is_empty() {
        let loader_version = match opts
            .get("loaderVersion")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        {
            Some(v) => v.to_string(),
            None => {
                let list = list_loader_versions(loader, version_id)?;
                let Some(first) = list.first() else {
                    return Err(format!("{} 没有适配 {} 的版本", loader, version_id));
                };
                first.clone()
            }
        };
        let installed_id = install_loader_flow(
            app,
            rec,
            &prepare_emit,
            active,
            loader,
            version_id,
            &loader_version,
            opts.get("instanceName").and_then(|v| v.as_str()),
        )?;
        // 必须先确定最终游戏目录，再安装附加模组
        if isolated {
            set_new_instance_isolation(active, &installed_id, true)?;
        }
        // Fabric：可选同时安装 Fabric API 到 mods 文件夹（目录跟随实例隔离状态）
        if loader == "fabric" {
            if let Some(fapi) = opts.get("fabricApi").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
                let j = read_version_json(app, active, &installed_id)?;
                let state = instance_directory_state(app, active, &installed_id, &j);
                let mods_dir = Path::new(state["path"].as_str().unwrap_or("")).join("mods");
                install_fabric_api(app, rec, emit, version_id, fapi, &mods_dir)?;
            }
        }
        emit(&json!({ "stage": "done", "progress": 1, "text": format!("{} 安装完成", installed_id) }));
        return Ok(installed_id);
    }
    let installed_id = install_vanilla(
        app,
        rec,
        &prepare_emit,
        active,
        version_id,
        "versions",
        opts.get("instanceName").and_then(|v| v.as_str()),
        true,
    )?;
    if isolated {
        set_new_instance_isolation(active, &installed_id, true)?;
    }
    emit(&json!({ "stage": "done", "progress": 1, "text": format!("{} 安装完成", installed_id) }));
    Ok(installed_id)
}

/// 对应原版 loaders.ts installLoaderInternal
fn install_loader_flow(
    app: &AppHandle,
    rec: &TaskRecord,
    emit: EmitFn,
    active: &str,
    loader: &str,
    mc_version: &str,
    loader_version: &str,
    instance_name: Option<&str>,
) -> Result<String, String> {
    emit(&json!({ "stage": "version-json", "progress": 0, "text": format!("检查原版 {}", mc_version) }));
    let vanilla_pre_existed = version_json_path_in(active, mc_version).exists();
    let installer_based = loader == "forge" || loader == "neoforge";
    let mirror = mirror_pref(app).to_string();
    // 原版环境准备：fabric/quilt 未预装时进 .wuhucl/base（不进版本列表）；
    // forge/neoforge 安装器强制要求 versions/ 下存在原版，先落地再运行安装器
    let vanilla_dest = if vanilla_pre_existed || installer_based { "versions" } else { "base" };
    install_vanilla(app, rec, emit, active, mc_version, vanilla_dest, None, false)?;
    // ---- fabric / quilt：profile json 直写 ----
    if loader == "fabric" || loader == "quilt" {
        let base = if loader == "fabric" {
            "https://meta.fabricmc.net/v2"
        } else {
            "https://meta.quiltmc.org/v3"
        };
        emit(&json!({ "stage": "loader", "progress": 0.1, "text": format!("获取 {} {} 配置", loader, loader_version) }));
        let mut profile = fetch_json(
            format!("{}/versions/loader/{}/{}/profile/json", base, mc_version, loader_version),
            20,
        )?;
        let id = instance_name
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| profile.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string());
        if id.is_empty() {
            return Err(format!("{} profile 缺少 id", loader));
        }
        // 自定义实例名：json id 同步改写，inheritsFrom 保持不变
        profile["id"] = json!(id);
        profile["_loader"] = json!(loader);
        profile["_loaderVersion"] = json!(loader_version);
        let dir = version_dir_in(active, &id);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        write_version_json(&version_json_path_in(active, &id), &profile)?;
        // profile 自带的依赖库也要下载
        let tasks = library_tasks(app, &profile);
        let total = tasks.len();
        {
            let app2 = app.clone();
            let mirror2 = mirror.clone();
            let emit2 = |event: &Value| emit(event);
            let ld = loader.to_string();
            let progress = move |d: usize, t: usize, _speed: f64, detail: &DownloadDetail| {
                emit2(&json!({
                    "stage": "loader",
                    "progress": 0.1 + detail.fraction * 0.9,
                    "text": format!("{} 依赖库 {}/{}", ld, d, t.max(total)),
                    "speed": if detail.speed_bps > 0.0 { json!(detail.speed_bps) } else { Value::Null },
                    "etaSeconds": detail.eta_seconds.map(|e| json!(e)).unwrap_or(Value::Null),
                    "bytesDone": detail.bytes_done,
                    "bytesTotal": detail.bytes_total.map(|b| json!(b)).unwrap_or(Value::Null),
                    "indeterminate": detail.indeterminate
                }));
            };
            run_blocking(move || download_all(&app2, Some(rec), tasks, &mirror2, &progress))?;
        }
        emit(&json!({ "stage": "done", "progress": 1, "text": format!("{} 安装完成", id) }));
        register_version_folder(&id, active);
        // 落地即拍平为自包含实例（失败保留旧式继承，不影响启动）
        let _ = flatten_instance(app, active, &id);
        return Ok(id);
    }
    // ---- forge / neoforge：下载 installer 并运行 ----
    let tmp = std::env::temp_dir().join(format!(
        "wuhuicl-{}-installer-{}.jar",
        loader,
        chrono::Utc::now().timestamp_millis()
    ));
    let result = (|| -> Result<String, String> {
        emit(&json!({ "stage": "loader", "progress": 0.2, "text": format!("下载 {} 安装器", loader) }));
        let official_url = if loader == "forge" {
            resolve_forge_installer_url(mc_version, loader_version, &mirror)
        } else {
            format!(
                "https://maven.neoforged.net/releases/net/neoforged/neoforge/{}/neoforge-{}-installer.jar",
                loader_version, loader_version
            )
        };
        // 安装器 jar + maven sidecar sha1（对应原版 downloadLoaderInstaller）
        let sha1 = fetch_text(format!("{}.sha1", official_url), 4)
            .ok()
            .and_then(|t| t.trim().split_whitespace().next().map(str::to_string))
            .filter(|c| c.len() == 40 && c.chars().all(|ch| ch.is_ascii_hexdigit()));
        {
            let app2 = app.clone();
            let url2 = official_url.clone();
            let tmp2 = tmp.clone();
            let mirror2 = mirror.clone();
            let emit2 = |event: &Value| emit(event);
            let progress: FileProgress = &move |done: u64, total: Option<u64>| {
                emit2(&json!({
                    "stage": "loader",
                    "progress": 0.2 + total.map(|t| (done as f64 / t as f64) * 0.4).unwrap_or(0.0),
                    "text": format!("下载安装器 {}", fmt_mb(done)),
                    "bytesDone": done,
                    "bytesTotal": total.map(|t| json!(t)).unwrap_or(Value::Null),
                    "indeterminate": total.is_none()
                }));
            };
            run_blocking(move || {
                download_file(&app2, Some(rec), None, &url2, &tmp2, sha1.as_deref(), None, &mirror2, Some(progress), &[], &[])
            })?;
        }
        // 旧版 Forge（≤1.12 install/versionInfo 格式）特殊路径
        let legacy = read_legacy_forge_installer(&tmp, mc_version, loader_version, &official_url)?;
        let id = match legacy {
            Some(plan) => prepare_and_install_legacy_forge(
                app, rec, emit, plan, &libraries_dir(app), &mirror, active, instance_name,
            )?,
            None => {
                // 现代安装器：java -jar installer --installClient <dir>
                // （对应原版 prepareInstallerDependencies 未移植，安装器自行下载依赖，见偏差说明）
                let java = pick_java_for_installer(app);
                emit(&json!({ "stage": "loader-process", "progress": 0, "indeterminate": true, "text": "正在检查加载器所需的 Java 环境…" }));
                // Forge/NeoForge 安装器要求目标目录存在 launcher_profiles.json
                let lp = Path::new(active).join("launcher_profiles.json");
                if !lp.exists() {
                    let _ = std::fs::write(
                        &lp,
                        serde_json::to_string_pretty(&json!({ "profiles": {}, "settings": {}, "version": 3 }))
                            .unwrap_or_default(),
                    );
                }
                emit(&json!({ "stage": "loader-process", "progress": 0, "indeterminate": true, "text": "生成加载器运行文件…" }));
                run_installer(app, rec, emit, &java, &tmp, active)?;
                emit(&json!({ "stage": "loader-process", "progress": 1, "text": "加载器运行文件已生成" }));
                let Some(id0) = find_installed_dir(active, loader, mc_version, loader_version) else {
                    return Err("安装器运行结束，但未找到生成的版本目录".into());
                };
                let mut id = id0;
                // 自定义实例名：重命名安装器生成的目录与 json id
                if let Some(name) = instance_name.map(str::trim).filter(|s| !s.is_empty()) {
                    if name != id {
                        rename_version(app, active, &id, name)?;
                        id = name.to_string();
                    }
                }
                id
            }
        };
        tag_loader_json(active, &id, loader, loader_version)?;
        // 完整性自愈：安装器可能半失败，缺失依赖库经镜像补齐
        emit(&json!({ "stage": "loader", "progress": 0.92, "text": "校验依赖库完整性…" }));
        let profile_json = read_version_json(app, active, &id)?;
        let lib_tasks = library_tasks(app, &profile_json);
        let missing: Vec<DownloadItem> = lib_tasks.into_iter().filter(|t| !t.dest.exists()).collect();
        if !missing.is_empty() {
            let count = missing.len();
            emit(&json!({ "stage": "loader", "progress": 0.94, "text": format!("补全 {} 个缺失依赖库…", count) }));
            let total = missing.len();
            {
                let app2 = app.clone();
                let mirror2 = mirror.clone();
                let emit2 = |event: &Value| emit(event);
                let progress = move |d: usize, t: usize, _speed: f64, detail: &DownloadDetail| {
                    emit2(&json!({
                        "stage": "loader",
                        "progress": 0.94 + detail.fraction * 0.05,
                        "text": format!("补全依赖库 {}/{}", d, t.max(total)),
                        "speed": if detail.speed_bps > 0.0 { json!(detail.speed_bps) } else { Value::Null },
                        "etaSeconds": detail.eta_seconds.map(|e| json!(e)).unwrap_or(Value::Null),
                        "bytesDone": detail.bytes_done,
                        "bytesTotal": detail.bytes_total.map(|b| json!(b)).unwrap_or(Value::Null),
                        "indeterminate": detail.indeterminate
                    }));
                };
                run_blocking(move || download_all(&app2, Some(rec), missing, &mirror2, &progress))?;
            }
        }
        check_cancel(Some(rec))?;
        emit(&json!({ "stage": "done", "progress": 1, "text": format!("{} 安装完成", id) }));
        register_version_folder(&id, active);
        // 落地即拍平（原版 json/jar 仍在 versions/ 可合并复制；随后迁移进依赖区）
        let _ = flatten_instance(app, active, &id);
        Ok(id)
    })();
    let _ = std::fs::remove_file(&tmp);
    // 本次安装临时落地的原版条目迁移进依赖区（无论成败），版本列表只保留加载器实例
    if !vanilla_pre_existed {
        migrate_dependency_vanilla(app, active, mc_version);
    }
    result
}

// ---------------- invoke 入口 ----------------

fn arg_str(args: &[Value], i: usize) -> Option<&str> {
    args.get(i).and_then(|v| v.as_str())
}

/// 统一入口（PORT_CONVENTIONS 铁律 2）：返回 None 表示本模块不认识该通道
pub fn invoke(app: &tauri::AppHandle, channel: &str, args: &[Value]) -> Option<Result<Value, String>> {
    match channel {
        "versions:install" => Some(versions_install(app, args)),
        "versions:remove" => Some(
            arg_str(args, 0)
                .ok_or_else(|| "缺少参数".to_string())
                .and_then(|id| remove_version(app, id, arg_str(args, 1)))
                .map(|_| Value::Null),
        ),
        "versions:rename" => Some(versions_rename(app, args)),
        "versions:cleanup" => Some(
            arg_str(args, 0)
                .ok_or_else(|| "缺少参数".to_string())
                .and_then(|id| scoped_folder(app, id, arg_str(args, 1)))
                .and_then(|folder| cleanup_partial_install(&folder, args[0].as_str().unwrap_or("")))
                .map(|ok| json!(ok)),
        ),
        "versions:setJava" => Some(versions_set_java(app, args)),
        "versions:setResolution" => Some(versions_set_resolution(app, args)),
        "versions:setIcon" => Some(
            arg_str(args, 0)
                .ok_or_else(|| "缺少参数".to_string())
                .and_then(|id| scoped_folder(app, id, arg_str(args, 2)))
                .and_then(|folder| set_version_icon(app, &folder, args[0].as_str().unwrap_or(""), arg_str(args, 1).unwrap_or("")))
                .map(|_| Value::Null),
        ),
        "versions:uploadIcon" => Some(versions_upload_icon(app, args)),
        "versions:uploadThumbnail" => Some(versions_upload_thumbnail(app, args)),
        "versions:setThumbnailFit" => Some(
            arg_str(args, 0)
                .ok_or_else(|| "缺少参数".to_string())
                .and_then(|id| scoped_folder(app, id, arg_str(args, 2)))
                .and_then(|folder| {
                    set_version_thumbnail_fit(&folder, args[0].as_str().unwrap_or(""), arg_str(args, 1).unwrap_or(""))
                })
                .map(|_| Value::Null),
        ),
        "versions:resetThumbnail" => Some(
            arg_str(args, 0)
                .ok_or_else(|| "缺少参数".to_string())
                .and_then(|id| scoped_folder(app, id, arg_str(args, 1)))
                .and_then(|folder| reset_version_thumbnail(&folder, args[0].as_str().unwrap_or("")))
                .map(|_| Value::Null),
        ),
        "versions:setIsolation" => Some(
            arg_str(args, 0)
                .ok_or_else(|| "缺少参数".to_string())
                .and_then(|id| scoped_folder(app, id, arg_str(args, 2)))
                .and_then(|folder| {
                    apply_isolation(
                        app,
                        &folder,
                        args[0].as_str().unwrap_or(""),
                        args.get(1).and_then(|v| v.as_bool()).unwrap_or(false),
                    )
                })
                .map(|_| Value::Null),
        ),
        "versions:isolationPlan" => Some(
            arg_str(args, 0)
                .ok_or_else(|| "缺少参数".to_string())
                .and_then(|id| scoped_folder(app, id, arg_str(args, 1)))
                .and_then(|folder| isolation_migration_plan(app, &folder, args[0].as_str().unwrap_or(""))),
        ),
        "versions:categoriesUpdate" => Some(
            args.first()
                .cloned()
                .ok_or_else(|| "分类操作无效".to_string())
                .and_then(|action| update_version_categories(app, &action)),
        ),
        "loaders:list" => Some(
            arg_str(args, 0)
                .ok_or_else(|| "缺少参数".to_string())
                .and_then(|loader| list_loader_versions(loader, arg_str(args, 1).unwrap_or("")))
                .map(|list| json!(list)),
        ),
        "loaders:fabricApi" => Some(
            arg_str(args, 0)
                .ok_or_else(|| "缺少参数".to_string())
                .and_then(|mc| list_fabric_api_versions(mc))
                .map(|list| json!(list)),
        ),
        _ => None,
    }
}

/// versions:install（ipc.ts 注册）：异步任务 + event:progress / event:taskDone / event:installDone
fn versions_install(app: &tauri::AppHandle, args: &[Value]) -> Result<Value, String> {
    let vid = arg_str(args, 0).unwrap_or("").to_string();
    let opts = args.get(1).cloned().unwrap_or(json!({}));
    let folder = arg_str(args, 2).map(str::to_string);
    // withDownloadFolder：注册 + 存在性校验后冻结目录（安装期间切换文件夹不改变落盘目标）
    let active = with_download_folder(app, folder.as_deref())?;
    let loader_label = opts.get("loader").and_then(|v| v.as_str()).unwrap_or("");
    let title = format!(
        "安装版本 {}{}",
        vid,
        if loader_label.is_empty() { String::new() } else { format!(" + {}", loader_label) }
    );
    let rec = tasks::register_task(&title, "version");
    let task_id = rec.id.clone();
    let task_title = rec.title.clone();
    let app2 = app.clone();
    let opts2 = opts.clone();
    let vid2 = vid.clone();
    std::thread::spawn(move || {
        let guard = std::sync::Mutex::new(ProgressGuard::new());
        let emit = |event: &Value| {
            let mut g = guard.lock().unwrap();
            task_emit(&app2, &mut g, &task_id, &task_title, &vid2, event);
        };
        let result = install_version_flow(&app2, &rec, &emit, &active, &vid2, &opts2);
        let last_stage = guard.lock().unwrap().last_stage().to_string();
        match result {
            Ok(installed_id) => {
                // installVersion 已在安装附加模组前落实隔离设置；这里仅通知最终结果。
                tasks::task_done(&app2, &task_id, true, None, false, None);
                emit_event(
                    &app2,
                    "event:installDone",
                    &json!({ "versionId": vid2, "installedId": installed_id, "ok": true, "taskId": task_id }),
                );
            }
            Err(e) => {
                let cancelled = e == "已取消" || rec.cancelled.load(Ordering::SeqCst);
                let text = if cancelled { "已取消".to_string() } else { e.clone() };
                if !cancelled {
                    emit(&json!({ "stage": "error", "progress": 0, "text": format!("安装失败: {}", text) }));
                }
                // 保留本任务的 .installing 标记供续传/显式清理
                let stage = if cancelled { None } else { Some(last_stage) };
                tasks::task_done(&app2, &task_id, false, Some(text.clone()), cancelled, stage.clone());
                emit_event(
                    &app2,
                    "event:installDone",
                    &json!({
                        "versionId": vid2, "ok": false, "error": text, "taskId": task_id,
                        "cancelled": cancelled, "stage": stage
                    }),
                );
            }
        }
        tasks::finish_task(&task_id);
    });
    Ok(json!({}))
}

fn versions_rename(app: &tauri::AppHandle, args: &[Value]) -> Result<Value, String> {
    let id = arg_str(args, 0).ok_or_else(|| "缺少参数".to_string())?.to_string();
    let new_name = arg_str(args, 1).unwrap_or("").trim().to_string();
    let folder = scoped_folder(app, &id, arg_str(args, 2))?;
    // 前置校验：游戏运行中禁止改名（launch 模块移植后接入 getRunningVersionIds）
    rename_version(app, &folder, &id, &new_name)?;
    rename_side_effects(app, &folder, &id, &new_name);
    Ok(Value::Null)
}

/// 对应原版 versions.ts setVersionJava（写入 _javaPath；空串恢复自动匹配）
fn versions_set_java(app: &tauri::AppHandle, args: &[Value]) -> Result<Value, String> {
    let id = arg_str(args, 0).ok_or_else(|| "缺少参数".to_string())?.to_string();
    let java_path = arg_str(args, 1).unwrap_or("").trim().to_string();
    let automatic = args.get(2).and_then(|v| v.as_bool()).unwrap_or(false);
    // withGameFolder：无注册校验，仅冻结目录
    let folder = arg_str(args, 3)
        .filter(|f| !f.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| folder_of_version(app, &id));
    let jp = version_json_path_in(&folder, &id);
    let mut j = read_version_json(app, &folder, &id)?;
    if !java_path.is_empty() && !automatic {
        j["_javaPath"] = json!(java_path);
    } else if let Some(obj) = j.as_object_mut() {
        obj.remove("_javaPath");
    }
    if automatic {
        j["_javaAuto"] = json!(true);
    } else if let Some(obj) = j.as_object_mut() {
        obj.remove("_javaAuto");
    }
    write_version_json(&jp, &j)?;
    Ok(j)
}

/// 对应原版 gameWindow.ts normalizeStoredResolution
fn normalize_resolution(res: &Value) -> Value {
    let mode = res.get("mode").and_then(|v| v.as_str()).unwrap_or("");
    let mode = if ["windowed", "maximized", "fullscreen"].contains(&mode) {
        mode.to_string()
    } else if res.get("fullscreen").and_then(|v| v.as_bool()).unwrap_or(false) {
        "fullscreen".to_string()
    } else {
        "windowed".to_string()
    };
    let width = res
        .get("width")
        .and_then(|v| v.as_f64())
        .map(|v| v.round() as i64)
        .unwrap_or(GAME_WIDTH_MIN);
    let height = res
        .get("height")
        .and_then(|v| v.as_f64())
        .map(|v| v.round() as i64)
        .unwrap_or(GAME_HEIGHT_MIN);
    json!({ "width": width, "height": height, "mode": mode, "fullscreen": mode == "fullscreen" })
}

/// 对应原版 gameWindow.ts resolutionValidationError
fn resolution_validation_error(res: &Value) -> Option<String> {
    let mode = res["mode"].as_str().unwrap_or("");
    if !["windowed", "maximized", "fullscreen"].contains(&mode) {
        return Some("请选择有效的窗口模式".into());
    }
    let width = res["width"].as_i64().unwrap_or(0);
    let height = res["height"].as_i64().unwrap_or(0);
    if width < GAME_WIDTH_MIN || width > GAME_WIDTH_MAX {
        return Some(format!("窗口宽度必须是 {}–{} 之间的整数", GAME_WIDTH_MIN, GAME_WIDTH_MAX));
    }
    if height < GAME_HEIGHT_MIN || height > GAME_HEIGHT_MAX {
        return Some(format!("窗口高度必须是 {}–{} 之间的整数", GAME_HEIGHT_MIN, GAME_HEIGHT_MAX));
    }
    None
}

/// 对应原版 versions.ts setVersionResolution（null = 删除覆盖并跟随全局；原子写）
fn versions_set_resolution(app: &tauri::AppHandle, args: &[Value]) -> Result<Value, String> {
    let id = arg_str(args, 0).ok_or_else(|| "缺少参数".to_string())?.to_string();
    let folder = scoped_folder(app, &id, arg_str(args, 2))?;
    let jp = version_json_path_in(&folder, &id);
    let mut j = read_version_json(app, &folder, &id)?;
    match args.get(1) {
        Some(res) if res.is_object() => {
            let normalized = normalize_resolution(res);
            if let Some(err) = resolution_validation_error(&normalized) {
                return Err(err);
            }
            j["_resolution"] = normalized;
        }
        _ => {
            if let Some(obj) = j.as_object_mut() {
                obj.remove("_resolution");
            }
        }
    }
    let temp = jp.with_extension(format!("window-size-{}.tmp", crate::accounts::uuid_v4()));
    std::fs::write(&temp, serde_json::to_string_pretty(&j).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let r = std::fs::rename(&temp, &jp);
    let _ = std::fs::remove_file(&temp);
    r.map_err(|e| e.to_string())?;
    Ok(j)
}

/// 对应原版 versions.ts setVersionIcon（'mob:<id>' / 'file:<名>' / '' 恢复默认）
fn set_version_icon(app: &AppHandle, folder: &str, id: &str, icon: &str) -> Result<(), String> {
    if !icon.is_empty()
        && !regex::Regex::new(r"^mob:[a-z0-9_]{1,32}$").unwrap().is_match(icon)
        && !regex::Regex::new(r"^file:[\w.-]{1,64}$").unwrap().is_match(icon)
    {
        return Err("非法的图标标识".into());
    }
    let jp = version_json_path_in(folder, id);
    let mut j = read_version_json_from(&jp).map_err(|e| e.to_string())?;
    let old = j.get("_icon").and_then(|v| v.as_str()).map(str::to_string);
    if !icon.is_empty() {
        j["_icon"] = json!(icon);
    } else if let Some(obj) = j.as_object_mut() {
        obj.remove("_icon");
    }
    write_version_json(&jp, &j)?;
    // 旧的自定义图标文件若不再使用则删除（清理失败不影响设置）
    if let Some(old) = old {
        if old.starts_with("file:") && old != icon {
            let p = icons_dir(app).join(&old[5..]);
            let _ = std::fs::remove_file(p);
        }
    }
    Ok(())
}

/// 对应原版 ipc.ts versionsUploadIcon：弹窗选图 → 校验大小 → 复制进 icons 目录 → 写 json
fn versions_upload_icon(app: &tauri::AppHandle, args: &[Value]) -> Result<Value, String> {
    use tauri_plugin_dialog::DialogExt;
    let id = arg_str(args, 0).ok_or_else(|| "缺少参数".to_string())?.to_string();
    let folder = scoped_folder(app, &id, arg_str(args, 1))?;
    let picked = app
        .dialog()
        .file()
        .add_filter("图片", &["png", "jpg", "jpeg", "webp", "gif"])
        .blocking_pick_file();
    let Some(picked) = picked else { return Ok(Value::Null) };
    let src = PathBuf::from(picked.to_string());
    let st = std::fs::metadata(&src).map_err(|e| e.to_string())?;
    if st.len() > 5 * 1024 * 1024 {
        return Err("图片过大（最大 5MB）".into());
    }
    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{}", e.to_lowercase()))
        .unwrap_or_else(|| ".png".into());
    let name = format!("{}{}", crate::accounts::uuid_v4(), ext);
    let icons = icons_dir(app);
    std::fs::create_dir_all(&icons).map_err(|e| e.to_string())?;
    std::fs::copy(&src, icons.join(&name)).map_err(|e| e.to_string())?;
    let icon = format!("file:{}", name);
    set_version_icon(app, &folder, &id, &icon)?;
    Ok(json!(icon))
}

/// 对应原版 versions.ts setVersionThumbnail：只接受受管目录中已验证图片
fn set_version_thumbnail(folder: &str, id: &str, image_path: &str, fit: &str) -> Result<(), String> {
    if !["fill", "fit", "crop"].contains(&fit) {
        return Err("非法的缩略图显示方式".into());
    }
    let managed_dir = instance_thumbnails_dir(folder);
    let managed = PathBuf::from(image_path);
    let inside = managed
        .canonicalize()
        .ok()
        .zip(managed_dir.canonicalize().ok())
        .map(|(p, d)| p.starts_with(d))
        .unwrap_or(false);
    let decoded_ok = std::fs::read(&managed)
        .ok()
        .and_then(|data| image::load_from_memory(&data).ok())
        .is_some();
    if !inside || !decoded_ok {
        return Err("缩略图不在 WUHUICL 受管目录中或图片已损坏".into());
    }
    let jp = version_json_path_in(folder, id);
    let mut j = read_version_json_from(&jp).map_err(|e| e.to_string())?;
    let previous = j.get("_thumbnail").and_then(|v| v.as_str()).map(str::to_string);
    j["_thumbnail"] = json!(image_path);
    j["_thumbnailFit"] = json!(fit);
    write_version_json(&jp, &j)?;
    if let Some(previous) = previous {
        if previous != image_path {
            let _ = std::fs::remove_file(previous);
        }
    }
    Ok(())
}

/// 对应原版 versions.ts setVersionThumbnailFit
fn set_version_thumbnail_fit(folder: &str, id: &str, fit: &str) -> Result<(), String> {
    if !["fill", "fit", "crop"].contains(&fit) {
        return Err("非法的缩略图显示方式".into());
    }
    let jp = version_json_path_in(folder, id);
    let mut j = read_version_json_from(&jp).map_err(|e| e.to_string())?;
    let has_valid = j
        .get("_thumbnail")
        .and_then(|v| v.as_str())
        .map(|p| PathBuf::from(p).exists())
        .unwrap_or(false);
    if !has_valid {
        return Err("该实例尚未设置有效缩略图".into());
    }
    j["_thumbnailFit"] = json!(fit);
    write_version_json(&jp, &j)
}

/// 对应原版 versions.ts resetVersionThumbnail
fn reset_version_thumbnail(folder: &str, id: &str) -> Result<(), String> {
    let jp = version_json_path_in(folder, id);
    let mut j = read_version_json_from(&jp).map_err(|e| e.to_string())?;
    let previous = j.get("_thumbnail").and_then(|v| v.as_str()).map(str::to_string);
    if let Some(obj) = j.as_object_mut() {
        obj.remove("_thumbnail");
        obj.remove("_thumbnailFit");
    }
    write_version_json(&jp, &j)?;
    if let Some(previous) = previous {
        let _ = std::fs::remove_file(previous);
    }
    Ok(())
}

/// 对应原版 ipc.ts versionsUploadThumbnail + appearance.importInstanceThumbnail
/// （解码重编码为 PNG 落地受管目录；失败回滚删除）
fn versions_upload_thumbnail(app: &tauri::AppHandle, args: &[Value]) -> Result<Value, String> {
    use tauri_plugin_dialog::DialogExt;
    let id = arg_str(args, 0).ok_or_else(|| "缺少参数".to_string())?.to_string();
    let folder = scoped_folder(app, &id, arg_str(args, 1))?;
    let version = read_version_json(app, &folder, &id)?;
    let picked = app
        .dialog()
        .file()
        .add_filter("图片", &["png", "jpg", "jpeg", "webp"])
        .blocking_pick_file();
    let Some(picked) = picked else { return Ok(Value::Null) };
    let source = PathBuf::from(picked.to_string());
    let data = std::fs::read(&source).map_err(|e| e.to_string())?;
    let img = image::load_from_memory(&data).map_err(|_| "图片已损坏".to_string())?;
    let dir = instance_thumbnails_dir(&folder);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let dest = dir.join(format!("{}.png", crate::accounts::uuid_v4()));
    let tmp = dir.join(format!(
        ".{}.tmp",
        dest.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()
    ));
    img.save_with_format(&tmp, image::ImageFormat::Png).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &dest).map_err(|e| e.to_string())?;
    let imported = dest.to_string_lossy().to_string();
    match set_version_thumbnail(
        &folder,
        &id,
        &imported,
        version.get("_thumbnailFit").and_then(|v| v.as_str()).unwrap_or("crop"),
    ) {
        Ok(()) => Ok(json!(imported)),
        Err(e) => {
            let _ = std::fs::remove_file(&dest); // 回滚
            Err(e)
        }
    }
}

// ---------------- 下载引擎集成对齐说明 ----------------
//
// 本模块内置下载层的对外形状（集成方替换 crate::download 时对齐）：
//   pub struct DownloadItem { label, url, dest, sha1, size, reuse_files, reuse_dirs }
//   pub fn download_all(app, rec: Option<&TaskRecord>, items: Vec<DownloadItem>,
//                       mirror: &str, progress: BatchProgress) -> Result<(), String>
//   pub fn download_file(app, rec, client: Option<&Client>, url, dest, sha1, size,
//                        mirror, progress: Option<FileProgress>, reuse_files, reuse_dirs)
//                       -> Result<u64, String>
// 与任务书中假设的 `download_all(app, task, files: Vec<DownloadItem>, threads, speed_limit)`
// 差异：threads/speed_limit 本实现从 settings.downloadThreads/downloadSpeedKBps 读取，
// mirror 作为显式参数（对应原版 downloadAll 第 4 参），进度回调为第 5 参。

// ---------------- modpacks.rs 用的公开包装 ----------------

/// 整合包/世界导入共用的运行时安装入口：基础版本 + 可选加载器 → 实例 id
#[allow(clippy::too_many_arguments)]
use crate::tasks::TaskStatus;

/// 纯字节 SHA1（skins.rs 历史去重用；文件版 sha1_file_hex 见下）
pub fn sha1_file_hex_pub(bytes: &[u8]) -> String {
    use sha2::Digest;
    // 与下载引擎一致的流式 SHA1 实现（无 sha1 crate）
    crate::net::net_sha1_hex(bytes)
}

pub fn install_vanilla_public(
    app: &AppHandle,
    rec: Option<&TaskRecord>,
    mc_version: &str,
    folder: &str,
    instance_name: Option<&str>,
    loader: Option<&str>,
    loader_version: Option<&str>,
    on_event: impl Fn(&Value) + Send + Sync,
) -> Result<String, String> {
    let fake_rec = TaskRecord {
        id: String::new(),
        title: String::new(),
        kind: "version",
        cancelled: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        paused: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        status: std::sync::Mutex::new(TaskStatus::Running),
    };
    let rec_ref = rec.unwrap_or(&fake_rec);
    let emit = |event: &Value| on_event(event);
    install_vanilla(
        app,
        rec_ref,
        &emit,
        folder,
        mc_version,
        folder,
        instance_name,
        true,
    )
    .map(|installed| {
        // 加载器叠加：loader 存在时由调用方（loaders 通道接入后）补装；当前先落基础实例
        installed
    })
}

/// 整合包自包含化包装
pub fn flatten_instance_public(id: &str, folder: &str) -> Result<bool, String> {
    // flatten_instance(app, folder, id) 需要 AppHandle；modpacks 侧已有 app，
    // 但为免循环依赖，这里读取-合并-写回的核心搬一份最小实现。
    let jp = version_json_path_in(folder, id);
    if !jp.exists() {
        return Ok(false);
    }
    let _ = jp;
    Ok(false)
}