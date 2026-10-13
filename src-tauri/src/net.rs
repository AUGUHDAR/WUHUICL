//! HTTP 客户端工厂 + 文件校验 / 格式化工具（下载引擎的底层）。
//!
//! 对应原版（KAMUCL-1.1.21/src/main/）：
//! - [`client`] / [`download_client`]      ← core/httpClient.ts（共享 Agent：keepAlive 连接池、
//!   connections=32、headersTimeout 30s；下载路径 bodyTimeout 120s。undici 的 allowH2 由
//!   reqwest+rustls 的 ALPN 自动协商等价）
//! - [`open_stream`]                       ← core/downloadFetch.ts `downloadFetch()`（手动重定向
//!   最多 10 跳 + 每跳安全检查 + 跨域剥离凭证 + CurseForge x-api-key 注入）
//! - [`needs_curseforge_key`] / [`cf_key`] ← core/downloadFetch.ts `needsCurseForgeKey` +
//!   core/curseforgeKey.ts `CF_BUILTIN_KEY`（KAMUCL_CF_API_KEY 环境变量优先）
//! - [`Integrity`] / [`verify_file`]       ← core/download.ts `verifyFile()`（大小/sha1/sha512/
//!   sha256 流式校验，错误文案原样）
//! - [`Sha1`] / [`sha1_hex`]               ← 等价实现 crypto.createHash('sha1')（FIPS 180-1；
//!   依赖清单没有 sha1 crate，按「缺能力等价实现」规约自写，已用标准测试向量校验）
//! - [`fmt_mb`]                            ← core/versions.ts `fmtMB()` / core/modpacks.ts `fmtMB()`
//! - [`get_text`]                          ← core/download.ts `fetchSignal()` 的 120s 硬超时语义，
//!   供元数据类 GET（loaders 等）使用
//!
//! 与原版的差异：
//! - Electron net.fetch 的 systemProxy/PAC 通道未移植（Windows 下原版 undici 路径本就不走系统代理；
//!   此处 `.no_proxy()` 对齐 undici 行为，环境变量代理不生效）。
//! - undici 默认 UA（"undici"）原样保留。

use reqwest::blocking::Client;
use reqwest::redirect::Policy;
use reqwest::Url;
use sha2::{Digest, Sha256, Sha512};
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

static CLIENT: OnceLock<Client> = OnceLock::new();
static DOWNLOAD_CLIENT: OnceLock<Client> = OnceLock::new();

/// 共享客户端（对应原版 sharedAgent）：连接池 32、空闲 10s（keepAliveTimeout）、
/// connect 30s（headersTimeout 的等价近似）。undici 的 bodyTimeout（读空闲超时）
/// 在 reqwest blocking 客户端上没有对应 API（read_timeout 仅 async 客户端提供），
/// 由调用方的看门狗线程等价实现。
pub fn client() -> &'static Client {
    CLIENT.get_or_init(|| {
        Client::builder()
            .user_agent("undici")
            .no_proxy()
            .connect_timeout(Duration::from_secs(30))
            .pool_idle_timeout(Duration::from_secs(10))
            .pool_max_idle_per_host(32)
            .build()
            .expect("HTTP 客户端初始化失败")
    })
}

/// 下载专用客户端（对应原版 rangeAgent + bodyTimeoutMs=120_000 派生 Agent）：
/// 手动重定向（由 [`open_stream`] 控制跳转）。
pub fn download_client() -> &'static Client {
    DOWNLOAD_CLIENT.get_or_init(|| {
        Client::builder()
            .user_agent("undici")
            .no_proxy()
            .connect_timeout(Duration::from_secs(30))
            .pool_idle_timeout(Duration::from_secs(10))
            .pool_max_idle_per_host(32)
            .redirect(Policy::none())
            .build()
            .expect("HTTP 下载客户端初始化失败")
    })
}

static FORGECDN_FILES: OnceLock<regex::Regex> = OnceLock::new();
fn forgecdn_files_re() -> &'static regex::Regex {
    FORGECDN_FILES.get_or_init(|| regex::Regex::new(r"^/files/\d+/\d+/").unwrap())
}

/// 对应原版 downloadFetch.ts `needsCurseForgeKey()`。
pub fn needs_curseforge_key(url: &str) -> bool {
    Url::parse(url)
        .map(|u| {
            u.scheme() == "https"
                && u.host_str() == Some("edge.forgecdn.net")
                && forgecdn_files_re().is_match(u.path())
        })
        .unwrap_or(false)
}

/// 对应原版 curseforgeKey.ts：环境变量优先，回落内置公开 key。
pub fn cf_key() -> String {
    std::env::var("KAMUCL_CF_API_KEY")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "$2a$10$m36VLjTaHEqxr/hO3kMDE.XCDEG90rSu3iGKkoPsj0KdCPWXOASXG".to_string())
}

/// 手动重定向的下载流（对应原版 downloadFetch.ts `downloadFetch()`）：
/// - 每跳校验协议 http/https、禁止内嵌凭证（『下载地址不安全』）
/// - 301/302/303/307/308 手动跟随；缺 location → 『下载跳转缺少地址』；
///   跳转后降级 https→http 或带凭证 → 『下载跳转地址不安全』
/// - 跨域剥离 authorization/proxy-authorization/cookie/x-api-key
/// - 超过 10 跳 → 『下载跳转次数过多』
/// cancel 在每跳边界检查。
pub fn open_stream(
    url: &str,
    headers: &[(String, String)],
    cancel: Option<&AtomicBool>,
) -> Result<reqwest::blocking::Response, String> {
    let mut current = url.to_string();
    let mut supplied: Vec<(String, String)> = headers.to_vec();
    for _hop in 0..10 {
        let target = Url::parse(&current).map_err(|_| "下载地址不安全".to_string())?;
        if !matches!(target.scheme(), "https" | "http")
            || !target.username().is_empty()
            || target.password().is_some()
        {
            return Err("下载地址不安全".to_string());
        }
        if let Some(flag) = cancel {
            if flag.load(Ordering::SeqCst) {
                return Err("已取消".to_string());
            }
        }
        let mut req = download_client().get(target.clone());
        if needs_curseforge_key(&current) {
            req = req.header("x-api-key", cf_key());
        }
        for (k, v) in &supplied {
            req = req.header(k.as_str(), v.as_str());
        }
        let resp = req.send().map_err(|e| e.to_string())?;
        let status = resp.status().as_u16();
        if !matches!(status, 301 | 302 | 303 | 307 | 308) {
            return Ok(resp);
        }
        let location = resp
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        drop(resp); // 关闭 3xx 响应体（对应原版 response.body?.cancel()）
        let location = location.ok_or_else(|| "下载跳转缺少地址".to_string())?;
        let next = target.join(&location).map_err(|_| "下载跳转缺少地址".to_string())?;
        if !matches!(next.scheme(), "https" | "http")
            || !next.username().is_empty()
            || next.password().is_some()
            || (target.scheme() == "https" && next.scheme() != "https")
        {
            return Err("下载跳转地址不安全".to_string());
        }
        if next.origin().ascii_serialization() != target.origin().ascii_serialization() {
            // 凭证永不跨源转发（对应原版 downloadFetch 的 header 过滤）
            supplied.retain(|(k, _)| {
                !matches!(
                    k.to_ascii_lowercase().as_str(),
                    "authorization" | "proxy-authorization" | "cookie" | "x-api-key"
                )
            });
        }
        current = next.to_string();
    }
    Err("下载跳转次数过多".to_string())
}

/// 校验期望（对应原版 download.ts 的 Integrity 形参）。
#[derive(Debug, Clone, Default)]
pub struct Integrity {
    pub sha1: Option<String>,
    pub sha512: Option<String>,
    pub sha256: Option<String>,
    pub size: Option<u64>,
}

impl Integrity {
    pub fn has_hash(&self) -> bool {
        self.sha1.is_some() || self.sha512.is_some() || self.sha256.is_some()
    }
}

/// 文件校验（对应原版 download.ts `verifyFile()`）。Ok(None) = 校验通过；
/// Ok(Some(reason)) = 校验失败（文案与原版一致）；Err = 读文件 IO 错误 / 已取消。
pub fn verify_file(
    file: &Path,
    expected: &Integrity,
    cancel: Option<&AtomicBool>,
) -> Result<Option<String>, String> {
    let stat = match std::fs::metadata(file) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Some("文件缺失".to_string())),
        Err(e) => return Err(e.to_string()),
    };
    if !stat.is_file() {
        return Ok(Some("路径不是文件".to_string()));
    }
    if let Some(size) = expected.size {
        if stat.len() != size {
            return Ok(Some("文件大小不符".to_string()));
        }
    }
    if stat.len() == 0 && expected.size != Some(0) {
        return Ok(Some("空文件".to_string()));
    }
    // 哈希顺序与原版一致：sha1 → sha512 → sha256，首个不符即返回
    let mut h1 = expected.sha1.as_ref().map(|_| Sha1::new());
    let mut h512 = expected.sha512.as_ref().map(|_| Sha512::new());
    let mut h256 = expected.sha256.as_ref().map(|_| Sha256::new());
    if h1.is_some() || h512.is_some() || h256.is_some() {
        let mut f = std::fs::File::open(file).map_err(|e| e.to_string())?;
        let mut buf = vec![0u8; 256 * 1024];
        loop {
            if let Some(flag) = cancel {
                if flag.load(Ordering::SeqCst) {
                    return Err("已取消".to_string());
                }
            }
            let n = f.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            if let Some(h) = h1.as_mut() {
                h.update(&buf[..n]);
            }
            if let Some(h) = h512.as_mut() {
                h.update(&buf[..n]);
            }
            if let Some(h) = h256.as_mut() {
                h.update(&buf[..n]);
            }
        }
        if let (Some(h), Some(expect)) = (h1, expected.sha1.as_deref()) {
            if hex::encode(h.finish()).to_lowercase() != expect.to_lowercase() {
                return Ok(Some("sha1 校验失败".to_string()));
            }
        }
        if let (Some(h), Some(expect)) = (h512, expected.sha512.as_deref()) {
            if hex::encode(h.finalize()).to_lowercase() != expect.to_lowercase() {
                return Ok(Some("sha512 校验失败".to_string()));
            }
        }
        if let (Some(h), Some(expect)) = (h256, expected.sha256.as_deref()) {
            if hex::encode(h.finalize()).to_lowercase() != expect.to_lowercase() {
                return Ok(Some("sha256 校验失败".to_string()));
            }
        }
    }
    Ok(None)
}

/// 对应原版 versions.ts / modpacks.ts `fmtMB()`。
pub fn fmt_mb(bytes: u64) -> String {
    format!("{:.1}MB", bytes as f64 / 1024.0 / 1024.0)
}

/// 元数据类 GET 文本（对应原版 download.ts `fetchSignal()` 的 120s 硬超时；
/// 供 loaders 等网络调用复用）。非 2xx 返回 Err("HTTP {status}: {url}")。
pub fn get_text(url: &str, timeout_ms: u64) -> Result<String, String> {
    let resp = client()
        .get(url)
        .timeout(Duration::from_millis(timeout_ms))
        .send()
        .map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(format!("HTTP {status}: {url}"));
    }
    resp.text().map_err(|e| e.to_string())
}

// ---------------- SHA-1（等价实现 crypto.createHash('sha1')，FIPS 180-1） ----------------

/// 依赖清单没有 sha1 crate；按移植规约「缺能力等价实现」。实现已用
/// "" / "abc" / 边界长度（55/56/63/64/65/119/120…）标准向量校验。
pub struct Sha1 {
    state: [u32; 5],
    len: u64,
    buf: [u8; 64],
    buf_len: usize,
}

impl Sha1 {
    pub fn new() -> Self {
        Self {
            state: [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0],
            len: 0,
            buf: [0; 64],
            buf_len: 0,
        }
    }

    pub fn update(&mut self, mut data: &[u8]) {
        self.len = self.len.wrapping_add(data.len() as u64);
        if self.buf_len > 0 {
            let need = 64 - self.buf_len;
            let take = need.min(data.len());
            self.buf[self.buf_len..self.buf_len + take].copy_from_slice(&data[..take]);
            self.buf_len += take;
            data = &data[take..];
            if self.buf_len == 64 {
                let block = self.buf;
                Self::compress(&mut self.state, &block);
                self.buf_len = 0;
            }
        }
        while data.len() >= 64 {
            let mut block = [0u8; 64];
            block.copy_from_slice(&data[..64]);
            Self::compress(&mut self.state, &block);
            data = &data[64..];
        }
        if !data.is_empty() {
            self.buf[..data.len()].copy_from_slice(data);
            self.buf_len = data.len();
        }
    }

    pub fn finish(mut self) -> [u8; 20] {
        let bit_len = self.len.wrapping_mul(8);
        self.buf[self.buf_len] = 0x80;
        self.buf_len += 1;
        if self.buf_len > 56 {
            for i in self.buf_len..64 {
                self.buf[i] = 0;
            }
            let block = self.buf;
            Self::compress(&mut self.state, &block);
            self.buf_len = 0;
        }
        for i in self.buf_len..56 {
            self.buf[i] = 0;
        }
        self.buf[56..64].copy_from_slice(&bit_len.to_be_bytes());
        let block = self.buf;
        Self::compress(&mut self.state, &block);
        let mut out = [0u8; 20];
        for (i, word) in self.state.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
        out
    }

    fn compress(state: &mut [u32; 5], block: &[u8; 64]) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([block[4 * i], block[4 * i + 1], block[4 * i + 2], block[4 * i + 3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) = (state[0], state[1], state[2], state[3], state[4]);
        for i in 0..80 {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(w[i]);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
    }
}

impl Default for Sha1 {
    fn default() -> Self {
        Self::new()
    }
}

pub fn sha1_hex(data: &[u8]) -> String {
    let mut h = Sha1::new();
    h.update(data);
    hex::encode(h.finish())
}

/// 纯字节 SHA1（skins 历史去重等非流式场景）
pub fn net_sha1_hex(data: &[u8]) -> String {
    let mut h = Sha1::new();
    h.update(data);
    hex::encode(h.finish())
}
