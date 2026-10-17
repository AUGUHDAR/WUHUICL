//! 对应原版 core/instanceVersionEvidence.ts 的 readClientVersionEvidence() 与 cachedClientVersionEvidence()。

use flate2::read::DeflateDecoder;
use regex::Regex;
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const MAX_METADATA: usize = 64 * 1024;

type Cache = (
    HashMap<PathBuf, (String, Option<String>)>,
    VecDeque<PathBuf>,
);

enum Parsed {
    /// 解析走到目录尾部：结果（可为无版本证据）进入缓存
    Cached(Option<String>),
    /// 原版直接 return undefined：不写缓存
    Skip,
}

fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new((HashMap::new(), VecDeque::new())))
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xffff_ffff;
    for byte in data {
        crc ^= *byte as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ if crc & 1 == 1 { 0xedb8_8320 } else { 0 };
        }
    }
    !crc
}

fn identity(md: &std::fs::Metadata) -> String {
    let ms = |t: std::io::Result<std::time::SystemTime>| {
        t.ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis())
            .unwrap_or(0)
    };
    format!("{}:{}:{}", md.len(), ms(md.modified()), ms(md.created()))
}

struct Reader {
    file: File,
    size: u64,
}

impl Reader {
    fn read(&mut self, offset: i64, size: usize) -> Result<Vec<u8>, ()> {
        if offset < 0 {
            return Err(());
        }
        let start = offset as u64;
        if start + size as u64 > self.size {
            return Err(());
        }
        self.file.seek(SeekFrom::Start(start)).map_err(|_| ())?;
        let mut buf = vec![0u8; size];
        self.file.read_exact(&mut buf).map_err(|_| ())?;
        Ok(buf)
    }
}

fn parse_evidence(path: &Path, size: u64) -> Result<Parsed, ()> {
    let mut reader = Reader {
        file: File::open(path).map_err(|_| ())?,
        size,
    };
    let tail_len = size.min(65557) as usize;
    let tail = reader.read(size.saturating_sub(65557) as i64, tail_len)?;
    let mut end = 0usize;
    let mut found = false;
    if tail_len >= 22 {
        let mut i = tail_len - 22;
        loop {
            if u32_at(&tail, i) == Some(0x0605_4b50)
                && i + 22 + u16_at(&tail, i + 20).unwrap_or(0) as usize == tail_len
            {
                end = i;
                found = true;
                break;
            }
            if i == 0 {
                break;
            }
            i -= 1;
        }
    }
    if !found
        || u16_at(&tail, end + 4).unwrap_or(1) != 0
        || u16_at(&tail, end + 6).unwrap_or(1) != 0
    {
        return Ok(Parsed::Skip);
    }
    let count = u16_at(&tail, end + 10).unwrap_or(0) as usize;
    let length = u32_at(&tail, end + 12).unwrap_or(0xffff_ffff);
    let mut cursor = u32_at(&tail, end + 16).unwrap_or(0xffff_ffff) as u64;
    if count == 0xffff
        || length == 0xffff_ffff
        || cursor == 0xffff_ffff
        || cursor + length as u64 > size
    {
        return Ok(Parsed::Skip);
    }
    let limit = cursor + length as u64;
    let mut version: Option<String> = None;
    for _ in 0..count {
        let central = reader.read(cursor as i64, 46)?;
        if u32_at(&central, 0) != Some(0x0201_4b50) {
            return Err(());
        }
        let name_size = u16_at(&central, 28).unwrap_or(0) as usize;
        let next = cursor
            + 46
            + name_size as u64
            + u16_at(&central, 30).unwrap_or(0) as u64
            + u16_at(&central, 32).unwrap_or(0) as u64;
        if next > limit {
            return Err(());
        }
        let name =
            String::from_utf8_lossy(&reader.read(cursor as i64 + 46, name_size)?).to_string();
        cursor = next;
        if name != "version.json" {
            continue;
        }
        let compressed = u32_at(&central, 20).unwrap_or(u32::MAX) as usize;
        let expanded = u32_at(&central, 24).unwrap_or(u32::MAX) as usize;
        let method = u16_at(&central, 10).unwrap_or(9);
        if u16_at(&central, 8).unwrap_or(1) & 1 == 1
            || compressed > MAX_METADATA
            || expanded > MAX_METADATA
            || (method != 0 && method != 8)
        {
            break;
        }
        let offset = u32_at(&central, 42).unwrap_or(0) as i64;
        let local = reader.read(offset, 30)?;
        if u32_at(&local, 0) != Some(0x0403_4b50) || u16_at(&local, 8) != Some(method) {
            break;
        }
        let local_name_size = u16_at(&local, 26).unwrap_or(0) as usize;
        let local_extra = u16_at(&local, 28).unwrap_or(0) as usize;
        let local_name =
            String::from_utf8_lossy(&reader.read(offset + 30, local_name_size)?).to_string();
        if local_name != name {
            break;
        }
        let body = reader.read(
            offset + 30 + local_name_size as i64 + local_extra as i64,
            compressed,
        )?;
        let data = if method == 8 {
            let mut inflated = Vec::new();
            DeflateDecoder::new(&body[..])
                .take((MAX_METADATA + 1) as u64)
                .read_to_end(&mut inflated)
                .map_err(|_| ())?;
            inflated
        } else {
            body
        };
        if data.len() != expanded || crc32(&data) != u32_at(&central, 16).unwrap_or(u32::MAX) {
            break;
        }
        let text = String::from_utf8_lossy(&data).to_string();
        let parsed =
            serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')).map_err(|_| ())?;
        let id = parsed.get("id").and_then(Value::as_str).map(str::to_string);
        if super::instance_metadata::is_minecraft_version_id(id.as_deref()) {
            version = id;
        }
        break;
    }
    Ok(Parsed::Cached(version))
}

pub fn read_client_version_evidence(path: &Path) -> Option<String> {
    let md = match std::fs::symlink_metadata(path) {
        Ok(md) => md,
        Err(_) => return None,
    };
    if !md.is_file() || md.is_symlink() {
        return None;
    }
    let current = identity(&md);
    let previous = cache().lock().ok().and_then(|mut guard| {
        guard.0.get(path).and_then(|(cached, version)| {
            if cached == &current {
                Some(version.clone())
            } else {
                None
            }
        })
    });
    if let Some(hit) = previous {
        return hit;
    }
    match parse_evidence(path, md.len()) {
        Ok(Parsed::Cached(version)) => {
            if let Ok(mut entry) = cache().lock() {
                if entry.0.len() > 32 {
                    if let Some(oldest) = entry.1.pop_front() {
                        entry.0.remove(&oldest);
                    }
                }
                entry
                    .0
                    .insert(path.to_path_buf(), (current, version.clone()));
                entry.1.push_back(path.to_path_buf());
            }
            version
        }
        _ => None,
    }
}

pub fn cached_client_version_evidence(chain: &[Value], roots: &[PathBuf]) -> Option<String> {
    static SHA1: OnceLock<Regex> = OnceLock::new();
    let sha1_re = SHA1.get_or_init(|| Regex::new(r"(?i)^[a-f\d]{40}$").unwrap());
    let hash = match chain
        .iter()
        .filter_map(|j| {
            j.get("downloads")
                .and_then(|d| d.get("client"))
                .and_then(|c| c.get("sha1"))
                .and_then(Value::as_str)
        })
        .find(|s| sha1_re.is_match(s))
    {
        Some(value) => value.to_lowercase(),
        None => return None,
    };
    let mut matches: HashSet<String> = HashSet::new();
    let mut visited: HashSet<String> = HashSet::new();
    for root in roots {
        if !visited.insert(root.to_string_lossy().to_lowercase()) {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_dir() || file_type.is_symlink() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let file = entry.path().join(format!("{name}.json"));
            let Ok(md) = std::fs::symlink_metadata(&file) else {
                continue;
            };
            if md.is_symlink() {
                continue;
            }
            let Ok(raw) = std::fs::read(&file) else {
                continue;
            };
            let text = String::from_utf8_lossy(&raw).to_string();
            let Ok(j) = serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')) else {
                continue;
            };
            let falsy = |key: &str| {
                !j.get(key)
                    .and_then(Value::as_str)
                    .map(|s| !s.is_empty())
                    .unwrap_or(false)
            };
            let id = j.get("id").and_then(Value::as_str);
            let sha = j
                .get("downloads")
                .and_then(|d| d.get("client"))
                .and_then(|c| c.get("sha1"))
                .and_then(Value::as_str);
            if falsy("inheritsFrom")
                && falsy("_loader")
                && super::instance_metadata::is_minecraft_version_id(id)
                && sha.map(str::to_lowercase).as_deref() == Some(hash.as_str())
            {
                if let Some(id) = id {
                    matches.insert(id.to_string());
                }
            }
        }
    }
    if matches.len() == 1 {
        matches.into_iter().next()
    } else {
        None
    }
}
