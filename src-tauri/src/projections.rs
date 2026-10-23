//! 投影子系统（对应原版 core/projectionsIpc.ts + projectionFormats.ts + projectionConversion.ts
//! + projectionValidation.ts + projectionJobs.ts + projectionRegistry.ts，形状见 shared/projections.ts）。
//! 通道：projections:list/operate/import/open/analyze/convert/discardAnalysis/versions（invoke）、
//! projections:drag（send）。差异：projectionJobs.ts 的 Worker 线程 → installer::run_blocking 同步作业
//! （120 秒超时不实现，取消经 tasks 轮询）；webContents.startDrag 无 Tauri 等价 API，drag 校验通过后
//! 推 files:dragError；projectionRegistries.json 以逐字节相同的 gzip 副本 include_bytes! 内置；
//! shell.trashItem → PowerShell，shell.showItemInFolder → explorer /select。

use crate::accounts::uuid_v4;
use crate::download::with_file_job;
use crate::installer::{self, read_version_json, same_path};
use crate::mods::assert_instance_idle;
use crate::settings;
use crate::tasks::{self, TaskRecord};
use crate::typednbt::{self, NbtPayload, NbtTag};
use base64::Engine;
use flate2::read::GzDecoder;
use regex::Regex;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};

type Pairs = Vec<(String, NbtTag)>;
type Vec3 = [i64; 3];
type Vec3F = [f64; 3];

const SCAN_DIR_LIMIT: usize = 20000;
const SCAN_ENTRY_LIMIT: usize = 10000;
const DRAG_LIMIT: usize = 1000;
const ANALYSIS_LIMIT: usize = 16;
const ANALYSIS_TTL_MS: i64 = 1_800_000;
const CHOICES_LIMIT: usize = 1_000_000;
const WORKER_MAX_BYTES: u64 = 64 * 1024 * 1024;
const NBT_LIMIT: usize = 16_777_216;
const COLON: &str = "：";
const KINDS: [&str; 4] = ["litematic", "schem", "schematic", "nbt"];
const AIR_STATES: [&str; 3] = ["minecraft:air", "minecraft:cave_air", "minecraft:void_air"];

static CATALOG: LazyLock<Mutex<HashMap<String, Stored>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static SCAN_GENERATION: AtomicU64 = AtomicU64::new(0);
static METADATA_CACHE: LazyLock<Mutex<HashMap<String, (u64, f64, Value)>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static ANALYSES: LazyLock<Mutex<HashMap<String, StoredAnalysis>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static REGISTRIES: LazyLock<Value> = LazyLock::new(load_registries);
static STATE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^([a-z0-9_.-]+:[a-z0-9_./-]+)(?:\[([^\]]*)\])?$").unwrap());
static VERSION_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(1\.20(?:\.\d+)?|1\.21(?:\.\d+)?|26\.\d+(?:\.\d+)?)$").unwrap());
static LEGACY_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^legacy:block_(\d+)\[data=(\d+)\]$").unwrap());

#[derive(Clone)]
struct Source {
    folder: Option<PathBuf>,
    root: PathBuf,
    label: String,
    /// 命中该目录的实例 id（实例目录才有；共享目录为 None）
    instance_id: Option<String>,
}

#[derive(Clone)]
struct Stored {
    entry: Value,
    source: Source,
    rel: String,
}

struct StoredAnalysis {
    view: Value,
    source_id: String,
    file: PathBuf,
    time: i64,
}

struct ScanErr {
    not_found: bool,
    message: String,
}

impl ScanErr {
    fn msg(message: impl Into<String>) -> Self {
        Self { not_found: false, message: message.into() }
    }
}

impl From<std::io::Error> for ScanErr {
    fn from(e: std::io::Error) -> Self {
        Self { not_found: e.kind() == std::io::ErrorKind::NotFound, message: e.to_string() }
    }
}

// ---------------- 快照数据（对应原版 core/projectionRegistry.ts minecraftData()） ----------------

fn load_registries() -> Value {
    let raw = include_bytes!("projection_registries.json.gz");
    let mut text = Vec::new();
    if GzDecoder::new(&raw[..]).read_to_end(&mut text).is_err() {
        return Value::Null;
    }
    serde_json::from_slice::<Value>(&text).unwrap_or(Value::Null)
}

fn snapshot() -> &'static Value {
    &REGISTRIES
}

fn block_schema(version: &str) -> Option<&'static Value> {
    let hash = snapshot()["registry"][version].as_str()?;
    let schema = &snapshot()["schemas"][hash];
    if schema.is_object() {
        Some(schema)
    } else {
        None
    }
}

fn has_blocks_array(version: &str) -> bool {
    block_schema(version).is_some()
}

fn legacy_blocks() -> &'static Value {
    &snapshot()["legacy"]
}

fn legacy_name(key: &str) -> Option<String> {
    legacy_blocks()[key].as_str().map(str::to_string)
}

fn legacy_values() -> HashSet<String> {
    legacy_blocks()
        .as_object()
        .map(|map| map.values().filter_map(|v| v.as_str()).map(str::to_string).collect())
        .unwrap_or_default()
}

fn legacy_reverse_map() -> HashMap<String, String> {
    let mut reverse: HashMap<String, String> = HashMap::new();
    if let Some(map) = legacy_blocks().as_object() {
        for (key, value) in map {
            if let Some(text) = value.as_str() {
                reverse.entry(text.to_string()).or_insert_with(|| key.clone());
            }
        }
    }
    reverse
}

struct RegistryVersion {
    version: String,
    data_version: i64,
    supported: bool,
}

// 对应原版 core/projectionConversion.ts projectionVersions()
fn projection_versions() -> Vec<RegistryVersion> {
    snapshot()["supported"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter(|entry| {
            entry["version"].as_str().map(|v| VERSION_RE.is_match(v)).unwrap_or(false)
        })
        .map(|entry| {
            let version = entry["version"].as_str().unwrap_or("").to_string();
            let data_version = entry["dataVersion"].as_i64().unwrap_or(0);
            RegistryVersion { version, data_version, supported: has_blocks_array(&entry["version"].as_str().unwrap_or("")) }
        })
        .collect()
}

fn projection_versions_json() -> Value {
    Value::Array(
        projection_versions()
            .into_iter()
            .map(|v| json!({ "version": v.version, "dataVersion": v.data_version, "supported": v.supported }))
            .collect(),
    )
}

fn version_name(data_version: Option<i64>) -> Option<String> {
    let wanted = data_version?;
    projection_versions().into_iter().find(|v| v.data_version == wanted).map(|v| v.version)
}

// ---------------- 路径工具（与 backupStore.ts safePath 同语义，复用 recordings.rs 的写法） ----------------

fn resolve_abs(input: &Path) -> PathBuf {
    let absolute = input.is_absolute();
    let mut out = if absolute { PathBuf::new() } else { std::env::current_dir().unwrap_or_default() };
    let mut stack: Vec<std::ffi::OsString> = Vec::new();
    for comp in input.components() {
        match comp {
            Component::Prefix(p) => out = PathBuf::from(p.as_os_str()),
            Component::RootDir => out.push(std::path::MAIN_SEPARATOR_STR),
            Component::CurDir => {}
            Component::ParentDir => {
                stack.pop();
            }
            Component::Normal(name) => stack.push(name.to_os_string()),
        }
    }
    for name in stack {
        out.push(name);
    }
    out
}

fn key_of(input: &Path) -> String {
    let value = resolve_abs(input).to_string_lossy().into_owned();
    if cfg!(windows) {
        value.to_lowercase()
    } else {
        value
    }
}

fn key_of_str(input: &str) -> String {
    key_of(Path::new(input))
}

fn safe_relative(value: &str) -> Result<(), String> {
    let bad = value.is_empty()
        || value.contains('\\')
        || value.contains('\0')
        || value.starts_with('/')
        || value
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == ".." || p.chars().any(|c| ":*?\"<>|".contains(c)));
    if bad {
        return Err(format!("无效的相对路径：{value}"));
    }
    Ok(())
}

fn safe_path(root: &Path, rel: &str, missing: bool) -> Result<PathBuf, ScanErr> {
    safe_relative(rel).map_err(ScanErr::msg)?;
    let base = resolve_abs(root);
    let base_stat = std::fs::symlink_metadata(&base).map_err(|e| ScanErr {
        not_found: e.kind() == std::io::ErrorKind::NotFound,
        message: "目录不是普通目录".into(),
    })?;
    if !base_stat.is_dir() || base_stat.is_symlink() {
        return Err(ScanErr::msg("目录不是普通目录"));
    }
    let mut current = base;
    for part in rel.split('/') {
        current = current.join(part);
        match std::fs::symlink_metadata(&current) {
            Ok(meta) if meta.is_symlink() => return Err(ScanErr::msg(format!("不跟随符号链接：{rel}"))),
            Ok(_) => {}
            Err(e) => {
                let not_found = e.kind() == std::io::ErrorKind::NotFound;
                if missing && not_found {
                    continue;
                }
                return Err(ScanErr { not_found, message: e.to_string() });
            }
        }
    }
    Ok(current)
}

fn real_path(input: &Path) -> Result<PathBuf, String> {
    let canonical = std::fs::canonicalize(input).map_err(|e| e.to_string())?;
    let text = canonical.to_string_lossy().into_owned();
    let mut out = text.clone();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        out = format!("\\{rest}");
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        out = rest.to_string();
    }
    Ok(PathBuf::from(out))
}

fn mtime_ms(meta: &std::fs::Metadata) -> f64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}

#[cfg(unix)]
fn file_id(meta: &std::fs::Metadata) -> u64 {
    std::os::unix::fs::MetadataExt::ino(meta)
}

#[cfg(windows)]
fn file_id(meta: &std::fs::Metadata) -> u64 {
    std::os::windows::fs::MetadataExt::creation_time(meta)
}

fn stat_of(path: &Path) -> Result<std::fs::Metadata, String> {
    std::fs::symlink_metadata(path).map_err(|e| e.to_string())
}

fn text_of(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

// ---------------- 标签访问（对应原版 core/projectionFormats.ts compound()/value()/list()/vec()） ----------------

fn pget<'a>(c: &'a Pairs, k: &str) -> Option<&'a NbtTag> {
    c.iter().find(|(name, _)| name == k).map(|(_, tag)| tag)
}

fn as_compound(t: Option<&NbtTag>) -> Result<&Pairs, String> {
    let Some(tag) = t else { return Err("缺少 Compound 标签".into()) };
    match &tag.value {
        NbtPayload::Compound(v) => Ok(v),
        _ => Err("缺少 Compound 标签".into()),
    }
}

fn pcompound<'a>(c: &'a Pairs, k: &str) -> Result<&'a Pairs, String> {
    as_compound(pget(c, k))
}

fn typed<'a>(c: &'a Pairs, k: &str, type_: u8) -> Result<Option<&'a NbtTag>, String> {
    match pget(c, k) {
        None => Ok(None),
        Some(tag) if tag.tag_type == type_ => Ok(Some(tag)),
        Some(_) => Err(format!("缺少或无效标签：{k}")),
    }
}

fn need<'a>(c: &'a Pairs, k: &str, type_: u8) -> Result<&'a NbtTag, String> {
    typed(c, k, type_)?.ok_or_else(|| format!("缺少或无效标签：{k}"))
}

fn list_of<'a>(c: &'a Pairs, k: &str) -> Result<&'a [NbtTag], String> {
    match typed(c, k, 9)? {
        Some(tag) => tag.items().ok_or_else(|| format!("缺少或无效标签：{k}")),
        None => Ok(&[]),
    }
}

fn t_int(t: &NbtTag) -> i64 {
    match &t.value {
        NbtPayload::Int(v) => *v as i64,
        _ => 0,
    }
}

fn t_short(t: &NbtTag) -> i64 {
    match &t.value {
        NbtPayload::Short(v) => *v as i64,
        _ => 0,
    }
}

fn t_str<'a>(t: &'a NbtTag) -> &'a str {
    match &t.value {
        NbtPayload::String(v) => v,
        _ => "",
    }
}

fn t_bytes<'a>(t: &'a NbtTag) -> &'a [u8] {
    match &t.value {
        NbtPayload::ByteArray(v) => v,
        _ => &[],
    }
}

fn t_longs<'a>(t: &'a NbtTag) -> &'a [i64] {
    match &t.value {
        NbtPayload::LongArray(v) => v,
        _ => &[],
    }
}

fn numeric(t: &NbtTag) -> Option<f64> {
    match &t.value {
        NbtPayload::Byte(v) => Some(*v as f64),
        NbtPayload::Short(v) => Some(*v as f64),
        NbtPayload::Int(v) => Some(*v as f64),
        NbtPayload::Long(v) => Some(*v as f64),
        NbtPayload::Float(v) => Some(*v as f64),
        NbtPayload::Double(v) => Some(*v),
        _ => None,
    }
}

fn to_i64(value: f64) -> i64 {
    if value.is_finite() {
        value.trunc() as i64
    } else {
        0
    }
}

fn round3(value: Vec3F) -> Vec3 {
    [to_i64(value[0]), to_i64(value[1]), to_i64(value[2])]
}

fn js_number(value: f64) -> String {
    if value.is_finite() && value.fract() == 0.0 && value.abs() < 1e21 {
        format!("{}", value as i64)
    } else {
        value.to_string()
    }
}

fn js_value_text(t: &NbtTag) -> String {
    match &t.value {
        NbtPayload::String(v) => v.clone(),
        NbtPayload::Byte(v) => v.to_string(),
        NbtPayload::Short(v) => v.to_string(),
        NbtPayload::Int(v) => v.to_string(),
        NbtPayload::Long(v) => v.to_string(),
        NbtPayload::Float(v) => js_number(*v as f64),
        NbtPayload::Double(v) => js_number(*v),
        NbtPayload::ByteArray(v) => v.iter().map(|b| b.to_string()).collect::<Vec<_>>().join(","),
        NbtPayload::IntArray(v) => v.iter().map(|b| b.to_string()).collect::<Vec<_>>().join(","),
        NbtPayload::LongArray(v) => v.iter().map(|b| b.to_string()).collect::<Vec<_>>().join(","),
        NbtPayload::List(v) => v.iter().map(js_value_text).collect::<Vec<_>>().join(","),
        NbtPayload::Compound(_) => "[object Object]".to_string(),
    }
}

fn vec3_of(c: &Pairs) -> Result<Vec3, String> {
    let mut out: Vec3 = [0, 0, 0];
    for (i, k) in ["x", "y", "z"].iter().enumerate() {
        let tag = need(c, k, 3)?;
        out[i] = t_int(tag);
    }
    Ok(out)
}

fn xyz3(t: Option<&NbtTag>) -> Result<Vec3F, String> {
    let Some(tag) = t else { return Ok([0.0, 0.0, 0.0]) };
    match &tag.value {
        NbtPayload::IntArray(v) if v.len() == 3 => Ok([v[0] as f64, v[1] as f64, v[2] as f64]),
        NbtPayload::List(v) if v.len() == 3 => {
            let mut out = [0.0f64; 3];
            for (i, item) in v.iter().enumerate() {
                out[i] = numeric(item).ok_or("投影坐标无效".to_string())?;
            }
            Ok(out)
        }
        _ => Err("投影坐标无效".into()),
    }
}

fn short_masked(c: &Pairs, k: &str) -> Result<i64, String> {
    Ok(t_short(need(c, k, 2)?) & 65535)
}

fn set_pair(c: &mut Pairs, k: &str, value: NbtTag) {
    match c.iter_mut().find(|(name, _)| name == k) {
        Some(slot) => slot.1 = value,
        None => c.push((k.to_string(), value)),
    }
}

fn extras(c: &Pairs, known: &[&str]) -> Pairs {
    c.iter().filter(|(name, _)| !known.contains(&name.as_str())).cloned().collect()
}

fn volume_f(size: &Vec3F) -> Result<usize, String> {
    const BAD: &str = "投影区域尺寸无效或超过 1600 万方块";
    let mut product: f64 = 1.0;
    for value in size {
        if !value.is_finite() || value.fract() != 0.0 || *value <= 0.0 || *value > 65535.0 || value.abs() > 9_007_199_254_740_991.0 {
            return Err(BAD.into());
        }
        product *= value;
    }
    if product > NBT_LIMIT as f64 {
        return Err(BAD.into());
    }
    Ok(product as usize)
}

fn volume_i(size: &Vec3) -> Result<usize, String> {
    volume_f(&[size[0] as f64, size[1] as f64, size[2] as f64])
}

// ---------------- 方块状态 / 数组编码（projectionFormats.ts stateText/stateTag/unpackLongs/…） ----------------

fn state_text(t: &NbtTag) -> Result<String, String> {
    let c = as_compound(Some(t))?;
    let name = t_str(need(c, "Name", 8)?).to_string();
    let mut pairs: Vec<String> = Vec::new();
    if let Some(properties) = pget(c, "Properties") {
        for (key, value) in as_compound(Some(properties))? {
            pairs.push(format!("{key}={}", js_value_text(value)));
        }
    }
    pairs.sort();
    let mut out = name;
    if !pairs.is_empty() {
        out.push('[');
        out.push_str(&pairs.join(","));
        out.push(']');
    }
    Ok(out)
}

fn state_tag(text: &str) -> Result<NbtTag, String> {
    let caps = STATE_RE
        .captures(text)
        .ok_or_else(|| format!("无效方块状态：{text}"))?;
    let mut pairs: Pairs = vec![("Name".to_string(), typednbt::string(caps.get(1).map(|m| m.as_str()).unwrap_or("")))];
    let raw = caps.get(2).map(|m| m.as_str()).unwrap_or("");
    if !raw.is_empty() {
        let mut properties: Pairs = Vec::new();
        for entry in raw.split(',') {
            let mut parts = entry.split('=');
            let key = parts.next().unwrap_or("");
            let value = parts.next().unwrap_or("");
            if key.is_empty() || value.is_empty() || parts.next().is_some() || properties.iter().any(|(k, _)| k == key) {
                return Err("无效方块属性".into());
            }
            properties.push((key.to_string(), typednbt::string(value)));
        }
        pairs.push(("Properties".to_string(), typednbt::compound(properties)));
    }
    Ok(typednbt::compound(pairs))
}

fn palette_bits(palette: usize) -> usize {
    let bits = (palette.max(1) as f64).log2().ceil() as i64;
    bits.max(2) as usize
}

fn unpack_longs(longs: &[i64], n: usize, palette: usize) -> Result<Vec<u32>, String> {
    let bits = palette_bits(palette);
    let mask = (1u64 << bits) - 1;
    let expected = (n * bits).div_ceil(64);
    if longs.len() != expected {
        return Err("Litematic 方块数组长度不匹配".into());
    }
    let mut result = vec![0u32; n];
    for i in 0..n {
        let bit = i * bits;
        let word = bit / 64;
        let shift = bit % 64;
        let mut raw = (longs[word] as u64) >> shift;
        if shift + bits > 64 {
            raw |= (longs[word + 1] as u64).wrapping_shl(64 - shift as u32);
        }
        let value = (raw & mask) as usize;
        if value >= palette {
            return Err("投影调色板索引越界".into());
        }
        result[i] = value as u32;
    }
    Ok(result)
}

fn pack_longs(blocks: &[u32], palette: usize) -> Vec<i64> {
    let bits = palette_bits(palette);
    let mut result = vec![0u64; (blocks.len() * bits).div_ceil(64)];
    for (i, value) in blocks.iter().enumerate() {
        let bit = i * bits;
        let word = bit / 64;
        let shift = bit % 64;
        let raw = *value as u64;
        result[word] |= raw.wrapping_shl(shift as u32);
        if shift + bits > 64 {
            result[word + 1] |= raw >> (64 - shift) as u32;
        }
    }
    result.into_iter().map(|v| v as i64).collect()
}

fn decode_varints(bytes: &[u8], n: usize, palette: usize) -> Result<Vec<u32>, String> {
    let mut out = vec![0u32; n];
    let mut p = 0usize;
    for i in 0..n {
        let mut value: f64 = 0.0;
        let mut shift = 0i32;
        loop {
            if p >= bytes.len() || shift > 28 {
                return Err("Sponge VarInt 损坏".into());
            }
            let byte = bytes[p];
            p += 1;
            value += (byte & 127) as f64 * (2i64.pow(shift as u32)) as f64;
            if byte & 128 == 0 {
                break;
            }
            shift += 7;
        }
        if value.fract() != 0.0 || value >= palette as f64 {
            return Err("投影调色板索引越界".into());
        }
        out[i] = value as u32;
    }
    if p != bytes.len() {
        return Err("Sponge 方块数组有尾数据".into());
    }
    Ok(out)
}

fn encode_varints(blocks: &[u32]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    for initial in blocks {
        let mut value = *initial;
        while value >= 128 {
            out.push(((value & 127) as u8) | 128);
            value >>= 7;
        }
        out.push(value as u8);
    }
    out
}

// ---------------- 实体位移（projectionFormats.ts shiftEntity/shiftBlockEntity） ----------------

fn shift_entity(t: &NbtTag, delta: &Vec3) -> Result<NbtTag, String> {
    let mut out = as_compound(Some(t))?.clone();
    let pos = xyz3(pget(&out, "Pos"))?;
    let values: Vec<NbtTag> = pos
        .iter()
        .enumerate()
        .map(|(i, v)| typednbt::double(v + delta[i] as f64))
        .collect();
    set_pair(&mut out, "Pos", typednbt::list(values, 6));
    Ok(typednbt::compound(out))
}

fn shift_block_entity(t: &NbtTag, delta: &Vec3) -> Result<NbtTag, String> {
    let mut out = as_compound(Some(t))?.clone();
    for (i, k) in ["x", "y", "z"].iter().enumerate() {
        let value = need(&out, k, 3)?;
        let current = t_int(value);
        set_pair(&mut out, k, typednbt::int((current + delta[i]) as i32));
    }
    Ok(typednbt::compound(out))
}

fn sponge_entity(t: &NbtTag, block: bool) -> Result<NbtTag, String> {
    let c = as_compound(Some(t))?;
    let id = t_str(need(c, "id", 8)?).to_string();
    let data = extras(c, if block { &["id", "x", "y", "z"] } else { &["id", "Pos"] });
    let pos = if block {
        typednbt::int_array(vec3_of(c)?.map(|v| v as i32).to_vec())
    } else {
        let list = xyz3(pget(c, "Pos"))?;
        typednbt::list(list.iter().map(|v| typednbt::double(*v)).collect(), 6)
    };
    Ok(typednbt::compound(vec![
        ("Id".to_string(), typednbt::string(id)),
        ("Pos".to_string(), pos),
        ("Data".to_string(), typednbt::compound(data)),
    ]))
}

fn entity_from_sponge(t: &NbtTag) -> Result<NbtTag, String> {
    let c = as_compound(Some(t))?;
    let id = t_str(need(c, "Id", 8)?).to_string();
    let mut data: Pairs = match pget(c, "Data") {
        Some(inner) => as_compound(Some(inner))?.clone(),
        None => extras(c, &["Pos", "Id"]),
    };
    set_pair(&mut data, "id", typednbt::string(id));
    if let Some(pos) = pget(c, "Pos") {
        set_pair(&mut data, "Pos", pos.clone());
    }
    Ok(typednbt::compound(data))
}

fn block_entity_from_sponge(t: &NbtTag) -> Result<NbtTag, String> {
    let c = as_compound(Some(t))?;
    let id = t_str(need(c, "Id", 8)?).to_string();
    let mut data: Pairs = match pget(c, "Data") {
        Some(inner) => as_compound(Some(inner))?.clone(),
        None => extras(c, &["Pos", "Id"]),
    };
    set_pair(&mut data, "id", typednbt::string(id));
    let pos = xyz3(pget(c, "Pos"))?;
    for (i, k) in ["x", "y", "z"].iter().enumerate() {
        set_pair(&mut data, k, typednbt::int(pos[i] as i32));
    }
    Ok(typednbt::compound(data))
}

// ---------------- 投影读取（对应原版 core/projectionFormats.ts readProjection()） ----------------

#[derive(Clone)]
struct Region {
    name: String,
    offset: Vec3,
    size: Vec3,
    signed_size: Option<Vec3>,
    anchor: Option<Vec3>,
    palette: Vec<NbtTag>,
    blocks: Vec<u32>,
    entities: Vec<NbtTag>,
    block_entities: Vec<NbtTag>,
    extra: Pairs,
}

#[derive(Clone)]
struct Projection {
    format: String,
    name: String,
    data_version: Option<i64>,
    regions: Vec<Region>,
    metadata: Pairs,
    extra: Pairs,
    raw: Pairs,
}

fn region_non_air(region: &Region) -> usize {
    let flags: Vec<bool> = region
        .palette
        .iter()
        .map(|t| AIR_STATES.contains(&state_text(t).unwrap_or_default().as_str()))
        .collect();
    region.blocks.iter().filter(|index| !flags.get(**index as usize).copied().unwrap_or(false)).count()
}

fn read_projection(input: &[u8], format: Option<&str>) -> Result<Projection, String> {
    let (root_name, root_tag) = typednbt::read_typed_nbt(input)?;
    let raw = as_compound(Some(&root_tag))?.clone();
    let schematic: Option<Pairs> = match pget(&raw, "Schematic") {
        Some(tag) => Some(as_compound(Some(tag))?.clone()),
        None => None,
    };
    let c: &Pairs = schematic.as_ref().unwrap_or(&raw);
    let blocks_is_type = |want: u8| pget(c, "Blocks").map(|t| t.tag_type == want).unwrap_or(false);
    let detected: Option<&str> = if pget(&raw, "Regions").is_some() {
        Some("litematic")
    } else if pget(c, "Palette").is_some() || blocks_is_type(10) {
        Some("schem")
    } else if blocks_is_type(7) {
        Some("schematic")
    } else if pget(&raw, "palette").is_some() && pget(&raw, "blocks").is_some() {
        Some("nbt")
    } else {
        None
    };
    let actual = match (detected, format) {
        (Some(found), _) => found,
        (None, Some(fallback)) => fallback,
        (None, None) => return Err("无法识别投影格式".into()),
    };
    if let Some(wanted) = format {
        if wanted != actual {
            return Err("扩展名与投影内容不匹配".into());
        }
    }
    let from_c = pget(c, "DataVersion").and_then(numeric).filter(|v| *v != 0.0);
    let data_version = match from_c {
        Some(v) => Some(to_i64(v)),
        None => pget(&raw, "MinecraftDataVersion").and_then(numeric).map(to_i64),
    };
    let metadata: Pairs = match pget(c, "Metadata") {
        Some(tag) => as_compound(Some(tag))?.clone(),
        None => Vec::new(),
    };
    let mut out = Projection {
        format: actual.to_string(),
        name: root_name.clone(),
        data_version,
        regions: Vec::new(),
        metadata,
        extra: Vec::new(),
        raw: raw.clone(),
    };
    if actual == "litematic" {
        let regions = pcompound(&raw, "Regions")?.clone();
        for (name, tag) in &regions {
            let r = as_compound(Some(tag))?.clone();
            let signed = vec3_of(pcompound(&r, "Size")?)?;
            let size: Vec3 = signed.map(|v| v.abs());
            let anchor = vec3_of(pcompound(&r, "Position")?)?;
            let local_min: Vec3 = signed.map(|v| if v < 0 { v + 1 } else { 0 });
            let offset: Vec3 = [anchor[0] + local_min[0], anchor[1] + local_min[1], anchor[2] + local_min[2]];
            let palette = list_of(&r, "BlockStatePalette")?.to_vec();
            if palette.is_empty() {
                return Err("投影调色板为空".into());
            }
            let negative = [-local_min[0], -local_min[1], -local_min[2]];
            let mut block_entities: Vec<NbtTag> = Vec::new();
            for item in list_of(&r, "TileEntities")? {
                let mut inner = as_compound(Some(item))?.clone();
                for (i, k) in ["x", "y", "z"].iter().enumerate() {
                    let value = t_int(need(&inner, k, 3)?);
                    set_pair(&mut inner, k, typednbt::int((value - local_min[i]) as i32));
                }
                block_entities.push(typednbt::compound(inner));
            }
            let mut entities: Vec<NbtTag> = Vec::new();
            for item in list_of(&r, "Entities")? {
                entities.push(shift_entity(item, &negative)?);
            }
            let volume = volume_i(&size)?;
            let blocks = unpack_longs(t_longs(need(&r, "BlockStates", 12)?), volume, palette.len())?;
            let extra = extras(&r, &["Position", "Size", "BlockStatePalette", "BlockStates", "Entities", "TileEntities"]);
            out.regions.push(Region {
                name: name.clone(),
                offset,
                size,
                signed_size: Some(signed),
                anchor: Some(anchor),
                palette,
                blocks,
                entities,
                block_entities,
                extra,
            });
        }
        out.extra = extras(&raw, &["Version", "SubVersion", "MinecraftDataVersion", "Metadata", "Regions"]);
    } else if actual == "schem" {
        let version = t_int(need(c, "Version", 3)?);
        if ![1, 2, 3].contains(&version) {
            return Err("Sponge 文件版本不支持".into());
        }
        let mut size: Vec3 = [0, 0, 0];
        for (i, k) in ["Width", "Height", "Length"].iter().enumerate() {
            size[i] = short_masked(c, k)?;
        }
        let n = volume_i(&size)?;
        let blocks_holder: Option<Pairs> = if version == 3 { Some(pcompound(c, "Blocks")?.clone()) } else { None };
        let b: &Pairs = blocks_holder.as_ref().unwrap_or(c);
        let palette_source = pcompound(b, "Palette")?.clone();
        let mut slots: Vec<Option<NbtTag>> = Vec::new();
        for (name, value) in &palette_source {
            if value.tag_type != 3 {
                return Err("Sponge 调色板无效".into());
            }
            let raw_index = t_int(value);
            if raw_index < 0 || raw_index > 65535 {
                return Err("Sponge 调色板无效".into());
            }
            let index = raw_index as usize;
            if slots.len() <= index {
                slots.resize(index + 1, None);
            }
            if slots[index].is_some() {
                return Err("Sponge 调色板无效".into());
            }
            slots[index] = Some(state_tag(name)?);
        }
        let palette: Vec<NbtTag> = match slots.into_iter().collect::<Option<Vec<NbtTag>>>() {
            Some(list) if !list.is_empty() && list.len() == palette_source.len() => list,
            _ => return Err("Sponge 调色板索引不连续".into()),
        };
        let be_key = if version == 1 { "TileEntities" } else { "BlockEntities" };
        let mut block_entities: Vec<NbtTag> = Vec::new();
        for item in list_of(b, be_key)? {
            block_entities.push(block_entity_from_sponge(item)?);
        }
        let mut entities: Vec<NbtTag> = Vec::new();
        for item in list_of(c, "Entities")? {
            entities.push(entity_from_sponge(item)?);
        }
        let data_key = if version == 3 { "Data" } else { "BlockData" };
        let blocks = decode_varints(t_bytes(need(b, data_key, 7)?), n, palette.len())?;
        let offset = round3(xyz3(pget(c, "Offset"))?);
        let known: &[&str] = if version == 3 {
            &["Palette", "Data", "BlockEntities"]
        } else {
            &["Version", "DataVersion", "Width", "Height", "Length", "Offset", "Palette", "PaletteMax", "BlockData", "TileEntities", "BlockEntities", "Entities", "Metadata"]
        };
        let extra = extras(b, known);
        out.regions.push(Region {
            name: "region".to_string(),
            offset,
            size,
            signed_size: None,
            anchor: None,
            palette,
            blocks,
            entities,
            block_entities,
            extra,
        });
        out.extra = if version == 3 {
            extras(c, &["Version", "DataVersion", "Width", "Height", "Length", "Offset", "Blocks", "Entities", "Metadata"])
        } else {
            Vec::new()
        };
    } else if actual == "schematic" {
        let mut size: Vec3 = [0, 0, 0];
        for (i, k) in ["Width", "Height", "Length"].iter().enumerate() {
            size[i] = short_masked(c, k)?;
        }
        let n = volume_i(&size)?;
        let ids = t_bytes(need(c, "Blocks", 7)?).to_vec();
        let data = t_bytes(need(c, "Data", 7)?).to_vec();
        let add: Vec<u8> = match typed(c, "AddBlocks", 7)? {
            Some(tag) => t_bytes(tag).to_vec(),
            None => Vec::new(),
        };
        if ids.len() != n || data.len() != n || (!add.is_empty() && add.len() != n.div_ceil(2)) {
            return Err("旧投影方块数组长度不匹配".into());
        }
        let mut palette: Vec<NbtTag> = Vec::new();
        let mut map: HashMap<String, usize> = HashMap::new();
        let mut blocks = vec![0u32; n];
        for i in 0..n {
            let high = if add.is_empty() {
                0
            } else {
                let byte = add[i >> 1];
                let nibble = if i % 2 == 1 { byte >> 4 } else { byte & 15 };
                (nibble as usize) << 8
            };
            let id = ids[i] as usize | high;
            let meta = (data[i] & 15) as usize;
            let mut text = legacy_name(&format!("{id}:{meta}"));
            if text.is_none() {
                text = Some(format!("legacy:block_{id}[data={meta}]"));
            }
            let text = text.unwrap();
            let index = match map.get(&text) {
                Some(found) => *found,
                None => {
                    let created = palette.len();
                    palette.push(state_tag(&text)?);
                    map.insert(text, created);
                    created
                }
            };
            blocks[i] = index as u32;
        }
        let offset = [
            pget(c, "WEOffsetX").and_then(numeric).map(to_i64).unwrap_or(0),
            pget(c, "WEOffsetY").and_then(numeric).map(to_i64).unwrap_or(0),
            pget(c, "WEOffsetZ").and_then(numeric).map(to_i64).unwrap_or(0),
        ];
        out.regions.push(Region {
            name: "region".to_string(),
            offset,
            size,
            signed_size: None,
            anchor: None,
            palette,
            blocks,
            entities: list_of(c, "Entities")?.to_vec(),
            block_entities: list_of(c, "TileEntities")?.to_vec(),
            extra: Vec::new(),
        });
        out.data_version = Some(1343);
        out.extra = extras(c, &["Width", "Height", "Length", "Blocks", "Data", "AddBlocks", "Entities", "TileEntities", "Materials", "WEOffsetX", "WEOffsetY", "WEOffsetZ"]);
    } else {
        if pget(&raw, "palettes").is_some() {
            return Err("原版结构包含多个候选调色板，暂不支持无损转换，请先在游戏内选定一个调色板".into());
        }
        let size_f = xyz3(pget(&raw, "size"))?;
        let n = volume_f(&size_f)?;
        let size = round3(size_f);
        let mut palette = list_of(&raw, "palette")?.to_vec();
        if palette.is_empty() {
            return Err("结构调色板为空".into());
        }
        let mut air = palette.iter().position(|t| state_text(t).unwrap_or_default() == "minecraft:air");
        if air.is_none() {
            palette.push(state_tag("minecraft:air")?);
            air = Some(palette.len() - 1);
        }
        let air_index = air.unwrap_or(0) as u32;
        let mut blocks = vec![air_index; n];
        let mut occupied = vec![0u8; n];
        let mut block_entities: Vec<NbtTag> = Vec::new();
        for item in list_of(&raw, "blocks")? {
            let b = as_compound(Some(item))?.clone();
            let p_f = xyz3(pget(&b, "pos"))?;
            if b.iter().any(|(k, _)| !["pos", "state", "nbt"].contains(&k.as_str())) {
                return Err("结构方块含未支持的附加包装标签，无法保证无损转换".into());
            }
            if p_f.iter().enumerate().any(|(i, v)| v.fract() != 0.0 || *v < 0.0 || *v >= size[i] as f64) {
                return Err("结构方块坐标越界".into());
            }
            let p = round3(p_f);
            let at = (p[0] + p[2] * size[0] + p[1] * size[0] * size[2]) as usize;
            let state = t_int(need(&b, "state", 3)?);
            if state < 0 || state as usize >= palette.len() || occupied[at] != 0 {
                return Err("结构方块重复或状态无效".into());
            }
            occupied[at] = 1;
            blocks[at] = state as u32;
            if let Some(nbt_tag) = pget(&b, "nbt") {
                let mut inner = as_compound(Some(nbt_tag))?.clone();
                for (i, k) in ["x", "y", "z"].iter().enumerate() {
                    set_pair(&mut inner, k, typednbt::int(p[i] as i32));
                }
                block_entities.push(typednbt::compound(inner));
            }
        }
        let mut entities: Vec<NbtTag> = Vec::new();
        for item in list_of(&raw, "entities")? {
            let c = as_compound(Some(item))?.clone();
            let mut nbt = as_compound(pget(&c, "nbt"))?.clone();
            let pos = xyz3(pget(&c, "pos"))?;
            if c.iter().any(|(k, _)| !["pos", "blockPos", "nbt"].contains(&k.as_str())) {
                return Err("结构实体包装包含未验证的数据，暂不支持无损转换".into());
            }
            if let Some(block_pos) = pget(&c, "blockPos") {
                let list = xyz3(Some(block_pos))?;
                if list.iter().enumerate().any(|(i, v)| *v != pos[i].floor()) {
                    return Err("结构实体包装包含未验证的数据，暂不支持无损转换".into());
                }
            }
            if let Some(existing) = pget(&nbt, "Pos") {
                let list = xyz3(Some(existing))?;
                if list.iter().enumerate().any(|(i, v)| *v != pos[i]) {
                    return Err("结构实体位置标签不一致，暂不支持无损转换".into());
                }
            }
            let values: Vec<NbtTag> = pos.iter().map(|v| typednbt::double(*v)).collect();
            set_pair(&mut nbt, "Pos", typednbt::list(values, 6));
            entities.push(typednbt::compound(nbt));
        }
        let offset = match pget(&raw, "KAMUCLOffset") {
            Some(tag) => round3(xyz3(Some(tag))?),
            None => [0, 0, 0],
        };
        out.regions.push(Region {
            name: "region".to_string(),
            offset,
            size,
            signed_size: None,
            anchor: None,
            palette,
            blocks,
            entities,
            block_entities,
            extra: Vec::new(),
        });
        out.extra = extras(&raw, &["DataVersion", "size", "palette", "blocks", "entities", "KAMUCLOffset"]);
    }
    let display_name: Option<String> = pget(&out.metadata, "Name").filter(|t| t.tag_type == 8).map(|t| t_str(t).to_string());
    if let Some(name) = display_name {
        out.name = name;
    }
    if out.regions.is_empty() {
        return Err("投影没有区域".into());
    }
    let total: usize = out.regions.iter().map(|r| r.blocks.len()).sum();
    if total > NBT_LIMIT {
        return Err("投影总量超过 1600 万方块".into());
    }
    for region in &out.regions {
        for state in &region.palette {
            state_text(state)?;
        }
    }
    Ok(out)
}

// ---------------- 单区域合并（对应原版 core/projectionFormats.ts flatten()） ----------------

fn flatten(p: &Projection) -> Result<Region, String> {
    let mut min: Vec3 = [i64::MAX, i64::MAX, i64::MAX];
    let mut max: Vec3 = [i64::MIN, i64::MIN, i64::MIN];
    for region in &p.regions {
        for i in 0..3 {
            min[i] = min[i].min(region.offset[i]);
            max[i] = max[i].max(region.offset[i] + region.size[i]);
        }
    }
    let size: Vec3 = [max[0] - min[0], max[1] - min[1], max[2] - min[2]];
    let n = volume_i(&size)?;
    let (w, l) = (size[0] as usize, size[2] as usize);
    let mut blocks = vec![0u32; n];
    let mut palette: Vec<NbtTag> = vec![state_tag("minecraft:air")?];
    let mut map: HashMap<String, usize> = HashMap::new();
    map.insert("minecraft:air".to_string(), 0);
    let mut occupied = vec![0u8; n];
    let mut entities: Vec<NbtTag> = Vec::new();
    let mut block_entities: Vec<NbtTag> = Vec::new();
    for region in &p.regions {
        let delta: Vec3 = [region.offset[0] - min[0], region.offset[1] - min[1], region.offset[2] - min[2]];
        let mut indexes: Vec<usize> = Vec::with_capacity(region.palette.len());
        for state in &region.palette {
            let text = state_text(state)?;
            let index = match map.get(&text) {
                Some(found) => *found,
                None => {
                    let created = palette.len();
                    palette.push(state.clone());
                    map.insert(text, created);
                    created
                }
            };
            indexes.push(index);
        }
        let rw = region.size[0] as usize;
        let rl = region.size[2] as usize;
        for i in 0..region.blocks.len() {
            let x = i % rw + delta[0] as usize;
            let z = (i / rw) % rl + delta[2] as usize;
            let y = i / (rw * rl) + delta[1] as usize;
            let at = x + z * w + y * w * l;
            if at >= n || occupied[at] != 0 {
                return Err("区域重叠，无法可靠合并为单区域格式".into());
            }
            occupied[at] = 1;
            blocks[at] = indexes.get(region.blocks[i] as usize).copied().unwrap_or(0) as u32;
        }
        for item in &region.entities {
            entities.push(shift_entity(item, &delta)?);
        }
        for item in &region.block_entities {
            block_entities.push(shift_block_entity(item, &delta)?);
        }
    }
    Ok(Region {
        name: if p.name.is_empty() { "region".to_string() } else { p.name.clone() },
        offset: min,
        size,
        signed_size: None,
        anchor: None,
        palette,
        blocks,
        entities,
        block_entities,
        extra: Vec::new(),
    })
}

// ---------------- 投影写出（对应原版 core/projectionFormats.ts writeProjection()） ----------------

fn vec_tag(v: &Vec3) -> NbtTag {
    let pairs: Pairs = ["x", "y", "z"]
        .iter()
        .enumerate()
        .map(|(i, k)| (k.to_string(), typednbt::int(v[i] as i32)))
        .collect();
    typednbt::compound(pairs)
}

fn join3(v: &Vec3) -> String {
    format!("{},{},{}", v[0], v[1], v[2])
}

fn as_short(value: i64) -> i16 {
    let mut out = value % 65536;
    if out < 0 {
        out += 65536;
    }
    if out > 32767 {
        out -= 65536;
    }
    out as i16
}

fn single_region(p: &Projection) -> Result<Region, String> {
    if p.regions.len() == 1 {
        Ok(p.regions[0].clone())
    } else {
        flatten(p)
    }
}

fn write_projection(p: &Projection, format: &str) -> Result<Vec<u8>, String> {
    let root: Pairs;
    if format == "litematic" {
        let mut regions: Pairs = Vec::new();
        for r in &p.regions {
            let signed = r.signed_size.unwrap_or(r.size);
            let anchor = r.anchor.unwrap_or(r.offset);
            let delta: Vec3 = signed.map(|v| if v < 0 { v + 1 } else { 0 });
            let mut inner = r.extra.clone();
            set_pair(&mut inner, "Position", vec_tag(&anchor));
            set_pair(&mut inner, "Size", vec_tag(&signed));
            set_pair(&mut inner, "BlockStatePalette", typednbt::list(r.palette.clone(), 10));
            set_pair(&mut inner, "BlockStates", typednbt::long_array(pack_longs(&r.blocks, r.palette.len())));
            let mut entities: Vec<NbtTag> = Vec::new();
            for item in &r.entities {
                entities.push(shift_entity(item, &delta)?);
            }
            set_pair(&mut inner, "Entities", typednbt::list(entities, 10));
            let mut block_entities: Vec<NbtTag> = Vec::new();
            for item in &r.block_entities {
                block_entities.push(shift_block_entity(item, &delta)?);
            }
            set_pair(&mut inner, "TileEntities", typednbt::list(block_entities, 10));
            regions.push((r.name.clone(), typednbt::compound(inner)));
        }
        let total_volume: i64 = p.regions.iter().map(|r| r.blocks.len() as i64).sum();
        let total_blocks: i64 = p.regions.iter().map(region_non_air).sum::<usize>() as i64;
        let mut metadata = p.metadata.clone();
        set_pair(&mut metadata, "Name", typednbt::string(if p.name.is_empty() { "KAMUCL" } else { p.name.as_str() }));
        set_pair(&mut metadata, "RegionCount", typednbt::int(p.regions.len() as i32));
        set_pair(&mut metadata, "TotalVolume", typednbt::int(total_volume as i32));
        set_pair(&mut metadata, "TotalBlocks", typednbt::int(total_blocks as i32));
        let mut built = p.extra.clone();
        set_pair(&mut built, "Version", typednbt::int(6));
        set_pair(&mut built, "SubVersion", typednbt::int(1));
        set_pair(&mut built, "MinecraftDataVersion", typednbt::int(p.data_version.unwrap_or(0) as i32));
        set_pair(&mut built, "Metadata", typednbt::compound(metadata));
        set_pair(&mut built, "Regions", typednbt::compound(regions));
        root = built;
    } else {
        let r = single_region(p)?;
        let (w, h, l) = (r.size[0], r.size[1], r.size[2]);
        if format == "schem" {
            let mut palette_pairs: Pairs = Vec::new();
            for (index, state) in r.palette.iter().enumerate() {
                palette_pairs.push((state_text(state)?, typednbt::int(index as i32)));
            }
            let mut blocks_compound = r.extra.clone();
            set_pair(&mut blocks_compound, "Palette", typednbt::compound(palette_pairs));
            set_pair(&mut blocks_compound, "Data", typednbt::byte_array(encode_varints(&r.blocks)));
            let mut block_entities: Vec<NbtTag> = Vec::new();
            for item in &r.block_entities {
                block_entities.push(sponge_entity(item, true)?);
            }
            set_pair(&mut blocks_compound, "BlockEntities", typednbt::list(block_entities, 10));
            let mut entities: Vec<NbtTag> = Vec::new();
            for item in &r.entities {
                entities.push(sponge_entity(item, false)?);
            }
            let mut schematic = p.extra.clone();
            set_pair(&mut schematic, "Version", typednbt::int(3));
            set_pair(&mut schematic, "DataVersion", typednbt::int(p.data_version.unwrap_or(0) as i32));
            set_pair(&mut schematic, "Metadata", typednbt::compound(p.metadata.clone()));
            set_pair(&mut schematic, "Width", typednbt::short(as_short(w)));
            set_pair(&mut schematic, "Height", typednbt::short(as_short(h)));
            set_pair(&mut schematic, "Length", typednbt::short(as_short(l)));
            set_pair(&mut schematic, "Offset", typednbt::int_array(r.offset.map(|v| v as i32).to_vec()));
            set_pair(&mut schematic, "Blocks", typednbt::compound(blocks_compound));
            set_pair(&mut schematic, "Entities", typednbt::list(entities, 10));
            root = vec![("Schematic".to_string(), typednbt::compound(schematic))];
        } else if format == "nbt" {
            let mut be_map: HashMap<String, NbtTag> = HashMap::new();
            for item in &r.block_entities {
                let inner = as_compound(Some(item))?;
                be_map.insert(join3(&vec3_of(inner)?), item.clone());
            }
            let (rw, rl) = (w as usize, l as usize);
            let mut blocks_list: Vec<NbtTag> = Vec::new();
            for i in 0..r.blocks.len() {
                let pos: Vec3 = [(i % rw) as i64, (i / (rw * rl)) as i64, ((i / rw) % rl) as i64];
                let mut compound_pairs: Pairs = vec![
                    ("pos".to_string(), typednbt::list(pos.iter().map(|v| typednbt::int(*v as i32)).collect(), 3)),
                    ("state".to_string(), typednbt::int(r.blocks[i] as i32)),
                ];
                if let Some(entity) = be_map.get(&join3(&pos)) {
                    let inner = as_compound(Some(entity))?.clone();
                    compound_pairs.push(("nbt".to_string(), typednbt::compound(extras(&inner, &["x", "y", "z"]))));
                }
                blocks_list.push(typednbt::compound(compound_pairs));
            }
            let mut entities: Vec<NbtTag> = Vec::new();
            for item in &r.entities {
                let inner = as_compound(Some(item))?;
                let pos = xyz3(pget(inner, "Pos"))?;
                let pairs: Pairs = vec![
                    ("pos".to_string(), typednbt::list(pos.iter().map(|v| typednbt::double(*v)).collect(), 6)),
                    ("blockPos".to_string(), typednbt::list(pos.iter().map(|v| typednbt::int(v.floor() as i32)).collect(), 3)),
                    ("nbt".to_string(), item.clone()),
                ];
                entities.push(typednbt::compound(pairs));
            }
            let mut built = p.extra.clone();
            set_pair(&mut built, "DataVersion", typednbt::int(p.data_version.unwrap_or(0) as i32));
            set_pair(&mut built, "size", typednbt::list(r.size.iter().map(|v| typednbt::int(*v as i32)).collect(), 3));
            set_pair(&mut built, "palette", typednbt::list(r.palette.clone(), 10));
            set_pair(&mut built, "blocks", typednbt::list(blocks_list, 10));
            set_pair(&mut built, "entities", typednbt::list(entities, 10));
            set_pair(&mut built, "KAMUCLOffset", typednbt::int_array(r.offset.map(|v| v as i32).to_vec()));
            root = built;
        } else {
            let reverse = legacy_reverse_map();
            let mut mappings: Vec<(usize, usize)> = Vec::with_capacity(r.palette.len());
            for state in &r.palette {
                let text = state_text(state)?;
                let key = match reverse.get(&text) {
                    Some(found) => Some(found.clone()),
                    None => LEGACY_RE.captures(&text).map(|caps| format!("{}:{}", &caps[1], &caps[2])),
                };
                let key = key.ok_or_else(|| format!("方块无法表示为旧 schematic：{text}"))?;
                let mut parts = key.split(':');
                let id: usize = parts.next().unwrap_or("0").parse().unwrap_or(0);
                let meta: usize = parts.next().unwrap_or("0").parse().unwrap_or(0);
                mappings.push((id, meta));
            }
            let count = r.blocks.len();
            let mut ids = vec![0u8; count];
            let mut data = vec![0u8; count];
            let mut add = vec![0u8; count.div_ceil(2)];
            for (i, value) in r.blocks.iter().enumerate() {
                let (id, meta) = mappings[*value as usize];
                ids[i] = (id & 255) as u8;
                data[i] = meta as u8;
                add[i >> 1] |= ((id >> 8) as u8) << if i % 2 == 1 { 4 } else { 0 };
            }
            let mut built = p.extra.clone();
            set_pair(&mut built, "Materials", typednbt::string("Alpha"));
            set_pair(&mut built, "Width", typednbt::short(as_short(w)));
            set_pair(&mut built, "Height", typednbt::short(as_short(h)));
            set_pair(&mut built, "Length", typednbt::short(as_short(l)));
            set_pair(&mut built, "Blocks", typednbt::byte_array(ids));
            set_pair(&mut built, "Data", typednbt::byte_array(data));
            set_pair(&mut built, "AddBlocks", typednbt::byte_array(add));
            set_pair(&mut built, "Entities", typednbt::list(r.entities.clone(), 10));
            set_pair(&mut built, "TileEntities", typednbt::list(r.block_entities.clone(), 10));
            set_pair(&mut built, "WEOffsetX", typednbt::int(r.offset[0] as i32));
            set_pair(&mut built, "WEOffsetY", typednbt::int(r.offset[1] as i32));
            set_pair(&mut built, "WEOffsetZ", typednbt::int(r.offset[2] as i32));
            root = built;
        }
    }
    let name = if format == "schematic" { "Schematic" } else { "" };
    typednbt::write_typed_nbt(&typednbt::compound(root), name, true)
}

// ---------------- 生成文件校验（对应原版 core/projectionValidation.ts） ----------------

fn tag_b64(t: &NbtTag) -> Result<String, String> {
    let wrapped = typednbt::compound(vec![("value".to_string(), typednbt::canonical(t))]);
    let bytes = typednbt::write_typed_nbt(&wrapped, "", false)?;
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}

fn tags(items: &[NbtTag]) -> Result<String, String> {
    let mut list: Vec<String> = Vec::with_capacity(items.len());
    for item in items {
        list.push(tag_b64(item)?);
    }
    list.sort();
    Ok(list.join("|"))
}

fn blocks_hash(r: &Region) -> Result<String, String> {
    let names: Vec<String> = r.palette.iter().map(state_text).collect::<Result<_, String>>()?;
    let mut hash = Sha256::new();
    let header = json!({ "size": r.size.to_vec(), "offset": r.offset.to_vec() });
    hash.update(serde_json::to_string(&header).unwrap_or_default().as_bytes());
    let mut seen = vec![0u8; names.len()];
    for index in &r.blocks {
        if let Some(slot) = seen.get_mut(*index as usize) {
            *slot = 1;
        }
    }
    let mut used: Vec<String> = Vec::new();
    for (i, name) in names.iter().enumerate() {
        if seen.get(i).copied().unwrap_or(0) != 0 && !used.contains(name) {
            used.push(name.clone());
        }
    }
    used.sort();
    hash.update(serde_json::to_string(&used).unwrap_or_default().as_bytes());
    let ids: HashMap<&str, u32> = used.iter().enumerate().map(|(i, name)| (name.as_str(), i as u32)).collect();
    let mapped: Vec<u32> = names.iter().map(|name| ids.get(name.as_str()).copied().unwrap_or(0)).collect();
    let mut chunk = vec![0u8; 16384];
    let mut count = 0usize;
    for index in &r.blocks {
        let value = mapped.get(*index as usize).copied().unwrap_or(0);
        let at = count * 4;
        chunk[at..at + 4].copy_from_slice(&value.to_be_bytes());
        count += 1;
        if count == 4096 {
            hash.update(&chunk);
            count = 0;
        }
    }
    if count > 0 {
        hash.update(&chunk[..count * 4]);
    }
    Ok(hex::encode(hash.finalize()))
}

fn assert_projection_equivalent(expected: &Projection, actual: &Projection, format: &str) -> Result<(), String> {
    let collect = |p: &Projection| -> Result<Vec<Region>, String> {
        if format == "litematic" || p.regions.len() == 1 {
            Ok(p.regions.clone())
        } else {
            Ok(vec![flatten(p)?])
        }
    };
    let a = collect(expected)?;
    let b = collect(actual)?;
    if a.len() != b.len() || (format != "schematic" && (expected.data_version.unwrap_or(0)) != (actual.data_version.unwrap_or(0))) {
        return Err("生成文件区域或游戏版本校验失败".into());
    }
    for i in 0..a.len() {
        let (left, right) = (&a[i], &b[i]);
        if blocks_hash(left)? != blocks_hash(right)? {
            return Err("生成文件方块状态或偏移校验失败".into());
        }
        let left_entities = left
            .entities
            .iter()
            .map(|t| shift_entity(t, &left.offset))
            .collect::<Result<Vec<NbtTag>, String>>()?;
        let right_entities = right
            .entities
            .iter()
            .map(|t| shift_entity(t, &right.offset))
            .collect::<Result<Vec<NbtTag>, String>>()?;
        let left_block_entities = left
            .block_entities
            .iter()
            .map(|t| shift_block_entity(t, &left.offset))
            .collect::<Result<Vec<NbtTag>, String>>()?;
        let right_block_entities = right
            .block_entities
            .iter()
            .map(|t| shift_block_entity(t, &right.offset))
            .collect::<Result<Vec<NbtTag>, String>>()?;
        if tags(&left_entities)? != tags(&right_entities)? || tags(&left_block_entities)? != tags(&right_block_entities)? {
            return Err("生成文件实体或方块实体校验失败".into());
        }
        if format == "litematic"
            && (left.name != right.name
                || json!(Vec::from(left.signed_size.unwrap_or(left.size))) != json!(Vec::from(right.signed_size.unwrap_or(right.size))))
        {
            return Err("生成文件区域方向校验失败".into());
        }
        let left_extra = typednbt::compound(left.extra.clone());
        let right_extra = typednbt::compound(right.extra.clone());
        if tags(&[left_extra])? != tags(&[right_extra])? {
            return Err("生成文件区域附加标签校验失败".into());
        }
    }
    if tags(&[typednbt::compound(expected.extra.clone())])? != tags(&[typednbt::compound(actual.extra.clone())])? {
        return Err("生成文件附加标签校验失败".into());
    }
    let generated = ["Name", "RegionCount", "TotalVolume", "TotalBlocks"];
    for (key, value) in &expected.metadata {
        if format == "litematic" && generated.contains(&key.as_str()) {
            continue;
        }
        match pget(&actual.metadata, key) {
            None => return Err(format!("生成文件元数据校验失败：{key}")),
            Some(other) => {
                if tags(std::slice::from_ref(value))? != tags(std::slice::from_ref(other))? {
                    return Err(format!("生成文件元数据校验失败：{key}"));
                }
            }
        }
    }
    Ok(())
}

// ---------------- 差异分析（对应原版 core/projectionConversion.ts） ----------------

struct Difference {
    key: String,
    kind: &'static str,
    description: String,
    count: usize,
    replacement: Option<String>,
    discard_only: bool,
}

struct Analysis {
    source_format: String,
    target_format: String,
    source_version: Option<String>,
    target_version: Option<String>,
    differences: Vec<Difference>,
    blocks: usize,
    unsupported: Option<String>,
}

fn difference_json(d: &Difference) -> Value {
    let mut map = Map::new();
    map.insert("key".into(), json!(d.key));
    map.insert("kind".into(), json!(d.kind));
    map.insert("description".into(), json!(d.description));
    map.insert("count".into(), json!(d.count));
    if let Some(replacement) = &d.replacement {
        map.insert("replacement".into(), json!(replacement));
    }
    if d.discard_only {
        map.insert("discardOnly".into(), json!(true));
    }
    Value::Object(map)
}

impl Analysis {
    fn to_json(&self) -> Value {
        let mut map = Map::new();
        map.insert("sourceFormat".into(), json!(self.source_format));
        map.insert("targetFormat".into(), json!(self.target_format));
        if let Some(value) = &self.source_version {
            map.insert("sourceVersion".into(), json!(value));
        }
        if let Some(value) = &self.target_version {
            map.insert("targetVersion".into(), json!(value));
        }
        map.insert("differences".into(), Value::Array(self.differences.iter().map(difference_json).collect()));
        map.insert("blocks".into(), json!(self.blocks));
        if let Some(value) = &self.unsupported {
            map.insert("unsupported".into(), json!(value));
        }
        Value::Object(map)
    }
}

fn validate_state(text: &str, version: &str) -> Result<Option<String>, String> {
    let tag = state_tag(text)?;
    let c = as_compound(Some(&tag))?;
    let name = t_str(need(c, "Name", 8)?).to_string();
    let Some(schema) = block_schema(version) else { return Ok(Some("目标版本缺少可信方块注册表".into())) };
    if !name.starts_with("minecraft:") {
        return Ok(Some("目标原版注册表不包含此模组方块".into()));
    }
    let block = &schema[&name[10..]];
    if !block.is_object() {
        return Ok(Some("目标版本没有此方块".into()));
    }
    let schema_states = block["states"].as_array().cloned().unwrap_or_default();
    let mut properties: Pairs = Vec::new();
    if let Some(entry) = pget(c, "Properties") {
        properties = as_compound(Some(entry))?.clone();
    }
    for (key, value) in &properties {
        let Some(state) = schema_states.iter().find(|s| s["name"].as_str() == Some(key.as_str())) else {
            return Ok(Some(format!("目标版本没有属性 {key}")));
        };
        let values: Vec<String> = match state.get("values").and_then(|v| v.as_array()) {
            Some(list) => list
                .iter()
                .map(|item| match item.as_str() {
                    Some(text) => text.to_string(),
                    None => item.to_string(),
                })
                .collect(),
            None => {
                if state["type"].as_str() == Some("bool") {
                    vec!["true".to_string(), "false".to_string()]
                } else {
                    let num = state["num_values"].as_i64().unwrap_or(0);
                    (0..num.max(0)).map(|i| i.to_string()).collect()
                }
            }
        };
        let given = js_value_text(value);
        if !values.contains(&given) {
            return Ok(Some(format!("目标版本不接受 {key}={given}")));
        }
    }
    let missing: Vec<&str> = schema_states
        .iter()
        .filter_map(|s| s["name"].as_str())
        .filter(|name| !properties.iter().any(|(key, _)| key == name))
        .collect();
    if !missing.is_empty() {
        let joined: Vec<String> = missing.iter().map(|v| v.to_string()).collect();
        return Ok(Some(format!("需要明确选择目标属性：{}", joined.join("、"))));
    }
    Ok(None)
}

fn raw_int(pairs: &Pairs, k: &str) -> Option<i64> {
    pget(pairs, k).and_then(numeric).map(to_i64)
}

fn analyze_projection(p: &Projection, format: &str, target_version: Option<&str>) -> Result<Analysis, String> {
    if !KINDS.contains(&format) {
        return Err("目标格式无效".into());
    }
    let source_version = version_name(p.data_version);
    let blocks: usize = p.regions.iter().map(|r| r.blocks.len()).sum();
    let mut result = Analysis {
        source_format: p.format.clone(),
        target_format: format.to_string(),
        source_version: source_version.clone(),
        target_version: target_version.map(str::to_string),
        differences: Vec::new(),
        blocks,
        unsupported: None,
    };
    if let Some(version) = target_version {
        let supported = projection_versions().into_iter().any(|v| v.version == version && v.supported);
        if !supported {
            result.unsupported = Some("该版本暂缺可信注册表，未提供版本转换".into());
            return Ok(result);
        }
    }
    let cross = match target_version {
        Some(version) => Some(version) != source_version.as_deref(),
        None => false,
    };
    let litematic_encoding = raw_int(&p.raw, "Version").map(|v| v != 6).unwrap_or(true)
        || raw_int(&p.raw, "SubVersion").map(|v| v != 1).unwrap_or(true);
    if p.format == "litematic" && format == "litematic" && litematic_encoding && !p.raw.is_empty() {
        result.differences.push(Difference {
            key: "litematicEncoding".into(),
            kind: "data",
            description: "将写入已验证的 Litematic v6 / SubVersion 1 编码；原编码版本标签不再保留".into(),
            count: 1,
            replacement: None,
            discard_only: true,
        });
    }
    let sponge_version: Option<i64> = match pget(&p.raw, "Schematic") {
        Some(tag) => raw_int(as_compound(Some(tag))?, "Version"),
        None => raw_int(&p.raw, "Version"),
    };
    let sponge_upgrade = p.format == "schem" && format == "schem" && sponge_version.map(|v| v != 3).unwrap_or(true);
    if sponge_upgrade {
        result.differences.push(Difference {
            key: "spongeVersion".into(),
            kind: "data",
            description: "Sponge 文件版本将升级为 v3；方块、实体与偏移保持，未验证的扩展数据需要逐项舍弃".into(),
            count: 1,
            replacement: None,
            discard_only: true,
        });
    }
    if cross && source_version.is_none() {
        result.unsupported = Some("无法确认原文件的现代游戏版本；旧 schematic 仅支持格式导入，不提供完整跨版本转换".into());
        return Ok(result);
    }
    if format == "schematic" && cross {
        result.unsupported = Some("旧 schematic 无法表示现代版本标签；请选择现代目标格式".into());
        return Ok(result);
    }
    if format != "litematic" {
        match flatten(p) {
            Ok(flat) => {
                if format == "nbt" && flat.blocks.len() > 180_000 {
                    result.unsupported = Some("原版结构标签过多；请拆分为不超过 18 万方块的区域".into());
                }
            }
            Err(e) => result.unsupported = Some(e),
        }
    }
    if p.regions.len() > 1 && format != "litematic" {
        let count = p.regions.len();
        result.differences.push(Difference {
            key: "regions".into(),
            kind: "data",
            description: "目标格式只有一个区域，将合并区域，区域名称和有符号选择方向不再保留".into(),
            count,
            replacement: None,
            discard_only: true,
        });
    } else if format != "litematic" && p.regions.iter().any(|r| r.signed_size.map(|s| s.iter().any(|v| *v < 0)).unwrap_or(false)) {
        result.differences.push(Difference {
            key: "direction".into(),
            kind: "data",
            description: "目标格式保留绝对偏移，但不保留负向选择方向与区域名称".into(),
            count: 1,
            replacement: None,
            discard_only: true,
        });
    }
    if format == "nbt" && p.regions.iter().any(|r| r.offset.iter().any(|v| *v != 0)) {
        result.differences.push(Difference {
            key: "nativeOffset".into(),
            kind: "data",
            description: "原版结构不支持放置偏移；会保留 KAMUCLOffset 标签供启动器读取，原版游戏将忽略该标签".into(),
            count: 1,
            replacement: None,
            discard_only: true,
        });
    }
    if format == "schematic" && p.data_version != Some(1343) {
        result.differences.push(Difference {
            key: "legacyVersion".into(),
            kind: "data",
            description: "旧 schematic 不支持现代游戏版本标签，将保存为 1.12 格式".into(),
            count: 1,
            replacement: None,
            discard_only: true,
        });
    }
    let legacy = legacy_values();
    for (ri, r) in p.regions.iter().enumerate() {
        let mut counts: HashMap<u32, usize> = HashMap::new();
        for index in &r.blocks {
            *counts.entry(*index).or_insert(0) += 1;
        }
        for (pi, state) in r.palette.iter().enumerate() {
            if !counts.contains_key(&(pi as u32)) {
                continue;
            }
            let text = state_text(state)?;
            let reason: Option<String> = if cross {
                validate_state(&text, target_version.unwrap_or(""))?
            } else if format == "schematic" && !legacy.contains(&text) && !text.starts_with("legacy:block_") {
                Some("旧 schematic 无法表示此方块状态".into())
            } else if format != "schematic" && text.starts_with("legacy:block_") {
                Some("无法可靠映射旧数值 ID，请选择替代方块或舍弃".into())
            } else {
                None
            };
            if let Some(reason) = reason {
                result.differences.push(Difference {
                    key: format!("block:{ri}:{pi}"),
                    kind: "block",
                    description: format!("{text} · {reason}"),
                    count: counts.get(&(pi as u32)).copied().unwrap_or(0),
                    replacement: Some("minecraft:air".into()),
                    discard_only: false,
                });
            }
            let unknown: Vec<String> = as_compound(Some(state))?
                .iter()
                .map(|(k, _)| k.clone())
                .filter(|k| k != "Name" && k != "Properties")
                .collect();
            if !unknown.is_empty() && (cross || format == "schem" || format == "schematic") {
                let count = unknown.len();
                result.differences.push(Difference {
                    key: format!("stateExtra:{ri}:{pi}"),
                    kind: "data",
                    description: format!("{text} 的未验证方块状态附加标签：{}", unknown.join("、")),
                    count,
                    replacement: None,
                    discard_only: true,
                });
            }
        }
        for (kind, items) in [("entity", &r.entities), ("blockEntity", &r.block_entities)] {
            for (i, state) in items.iter().enumerate() {
                let c = as_compound(Some(state))?;
                if cross || (format == "schematic" && p.format != "schematic") {
                    let id = pget(c, "id")
                        .map(js_value_text)
                        .filter(|v| !v.is_empty())
                        .unwrap_or_else(|| "未知类型".to_string());
                    result.differences.push(Difference {
                        key: format!("{kind}:{ri}:{i}"),
                        kind: if kind == "entity" { "entity" } else { "blockEntity" },
                        description: format!("{id}{COLON}该版本组合的实体 NBT 迁移规则尚未验证"),
                        count: 1,
                        replacement: None,
                        discard_only: true,
                    });
                } else if kind == "blockEntity" {
                    let pos = vec3_of(c)?;
                    let at = (pos[0] as i128)
                        .wrapping_add((pos[2] as i128).wrapping_mul(r.size[0] as i128))
                        .wrapping_add((pos[1] as i128).wrapping_mul(r.size[0] as i128).wrapping_mul(r.size[2] as i128));
                    let key = if at < 0 {
                        String::new()
                    } else {
                        match r.blocks.get(at as usize) {
                            Some(index) => format!("block:{ri}:{index}"),
                            None => String::new(),
                        }
                    };
                    if result.differences.iter().any(|d| d.key == key) && !key.is_empty() {
                        result.differences.push(Difference {
                            key: format!("blockEntity:{ri}:{i}"),
                            kind: "blockEntity",
                            description: "此方块实体对应方块将被替换，需要舍弃其数据".into(),
                            count: 1,
                            replacement: None,
                            discard_only: true,
                        });
                    }
                }
            }
        }
        if (cross || format != p.format || sponge_upgrade) && !r.extra.is_empty() {
            let names: Vec<String> = r.extra.iter().map(|(k, _)| k.clone()).collect();
            let count = names.len();
            result.differences.push(Difference {
                key: format!("regionExtra:{ri}"),
                kind: "data",
                description: format!("区域附加标签：{}；目标格式或版本不保证语义", names.join("、")),
                count,
                replacement: None,
                discard_only: true,
            });
        }
    }
    if (cross || format != p.format || sponge_upgrade) && !p.extra.is_empty() {
        let names: Vec<String> = p.extra.iter().map(|(k, _)| k.clone()).collect();
        let count = names.len();
        result.differences.push(Difference {
            key: "extra".into(),
            kind: "data",
            description: format!("附加标签：{}；目标格式或版本不保证语义", names.join("、")),
            count,
            replacement: None,
            discard_only: true,
        });
    }
    if format == "nbt" && !p.metadata.is_empty() {
        let count = p.metadata.len();
        result.differences.push(Difference {
            key: "metadata".into(),
            kind: "data",
            description: "原版结构不支持投影作者等元数据".into(),
            count,
            replacement: None,
            discard_only: true,
        });
    }
    Ok(result)
}

// ---------------- 转换（对应原版 core/projectionConversion.ts convertProjection()） ----------------

fn convert_projection(mut p: Projection, format: &str, version: Option<&str>, choices: &Value) -> Result<Vec<u8>, String> {
    let analysis = analyze_projection(&p, format, version)?;
    if let Some(reason) = analysis.unsupported {
        return Err(reason);
    }
    for difference in &analysis.differences {
        let choice = choices.get(&difference.key).and_then(|v| v.as_str()).filter(|v| !v.is_empty());
        let Some(choice) = choice else {
            return Err(format!("请确认全部差异：{}", difference.description));
        };
        if difference.discard_only && choice != "discard" {
            return Err("此数据仅可舍弃".into());
        }
        let parts: Vec<&str> = difference.key.split(':').collect();
        if parts.first().copied() == Some("block") && parts.len() == 3 {
            let replacement = if choice == "discard" { "minecraft:air".to_string() } else { choice.to_string() };
            if let Some(target) = version {
                if let Some(reason) = validate_state(&replacement, target)? {
                    return Err(format!("替代方块不可用：{reason}"));
                }
            }
            if format == "schematic" {
                let legacy = legacy_values();
                if !legacy.contains(&replacement) {
                    return Err("替代方块无法写入旧 schematic".into());
                }
            }
            if let (Ok(ri), Ok(pi)) = (parts[1].parse::<usize>(), parts[2].parse::<usize>()) {
                if let Some(region) = p.regions.get_mut(ri) {
                    if pi < region.palette.len() {
                        region.palette[pi] = state_tag(&replacement)?;
                    }
                }
            }
        }
    }
    for (ri, r) in p.regions.iter_mut().enumerate() {
        let mut kept: Vec<NbtTag> = Vec::new();
        for (i, item) in r.entities.iter().enumerate() {
            if choice_value(choices, &format!("entity:{ri}:{i}")) != "discard" {
                kept.push(item.clone());
            }
        }
        r.entities = kept;
        let mut kept: Vec<NbtTag> = Vec::new();
        for (i, item) in r.block_entities.iter().enumerate() {
            if choice_value(choices, &format!("blockEntity:{ri}:{i}")) != "discard" {
                kept.push(item.clone());
            }
        }
        r.block_entities = kept;
        for pi in 0..r.palette.len() {
            if choice_value(choices, &format!("stateExtra:{ri}:{pi}")) == "discard" {
                let text = state_text(&r.palette[pi])?;
                r.palette[pi] = state_tag(&text)?;
            }
        }
        if choice_value(choices, &format!("regionExtra:{ri}")) == "discard" {
            r.extra.clear();
        }
    }
    if choice_value(choices, "extra") == "discard" {
        p.extra.clear();
    }
    if choice_value(choices, "metadata") == "discard" {
        p.metadata.clear();
    }
    if let Some(target) = version {
        p.data_version = projection_versions().into_iter().find(|v| v.version == target).map(|v| v.data_version);
    }
    let bytes = write_projection(&p, format)?;
    let reread = read_projection(&bytes, Some(format))?;
    assert_projection_equivalent(&p, &reread, format)?;
    Ok(bytes)
}

fn choice_value<'a>(choices: &'a Value, key: &str) -> &'a str {
    choices.get(key).and_then(|v| v.as_str()).unwrap_or("")
}

// ---------------- 后台作业（对应原版 core/projectionJobs.ts runProjectionWorker()） ----------------

struct WorkerJob {
    file: PathBuf,
    source_format: String,
    action: &'static str,
    format: Option<String>,
    version: Option<String>,
    hash: Option<String>,
    choices: Option<Value>,
}

struct WorkerProgress {
    app: tauri::AppHandle,
    task_id: String,
    task_title: String,
    enabled: bool,
}

impl WorkerProgress {
    fn send(&self, fraction: f64, text: &str) {
        if self.enabled {
            tasks::emit_progress(
                &self.app,
                json!({
                    "taskId": self.task_id,
                    "taskTitle": self.task_title,
                    "stage": "world",
                    "progress": fraction,
                    "text": text
                }),
            );
        }
    }
}

enum WorkerOutput {
    Json(Value),
    Bytes(Vec<u8>),
}

fn read_projection_file(job: &WorkerJob, progress: &WorkerProgress) -> Result<(Projection, String), String> {
    let meta = std::fs::metadata(&job.file).map_err(|e| e.to_string())?;
    if meta.len() > WORKER_MAX_BYTES {
        return Err("投影文件超过 64 MB 压缩文件上限".into());
    }
    let mut input: Vec<u8> = Vec::new();
    std::fs::File::open(&job.file)
        .map_err(|e| e.to_string())?
        .read_to_end(&mut input)
        .map_err(|e| e.to_string())?;
    let hash = hex::encode(Sha256::digest(&input));
    if let Some(wanted) = &job.hash {
        if *wanted != hash {
            return Err("原文件已变化，请重新分析".into());
        }
    }
    progress.send(0.15, "读取投影与校验原文件");
    let p = read_projection(&input, Some(job.source_format.as_str()))?;
    progress.send(0.45, "检查区域、方块状态与实体数据");
    Ok((p, hash))
}

fn run_projection_worker(job: WorkerJob, progress: WorkerProgress) -> Result<WorkerOutput, String> {
    installer::run_blocking(move || {
        let (p, hash) = read_projection_file(&job, &progress)?;
        match job.action {
            "metadata" => {
                let mut map = Map::new();
                map.insert("blocks".into(), json!(p.regions.iter().map(region_non_air).sum::<usize>()));
                if let Some(value) = p.data_version {
                    map.insert("dataVersion".into(), json!(value));
                }
                if let Some(value) = version_name(p.data_version) {
                    map.insert("gameVersion".into(), json!(value));
                }
                Ok(WorkerOutput::Json(Value::Object(map)))
            }
            "analyze" => {
                let analysis = analyze_projection(&p, job.format.as_deref().unwrap_or(""), job.version.as_deref())?;
                let mut view = analysis.to_json();
                if let Value::Object(map) = &mut view {
                    map.insert("sourceHash".into(), json!(hash));
                }
                Ok(WorkerOutput::Json(view))
            }
            _ => {
                let empty = json!({});
                let bytes = convert_projection(
                    p.clone(),
                    job.format.as_deref().unwrap_or(""),
                    job.version.as_deref(),
                    job.choices.as_ref().unwrap_or(&empty),
                )?;
                progress.send(0.85, "重新读取生成文件验证");
                let reread = read_projection(&bytes, Some(job.format.as_deref().unwrap_or(""))) ?;
                let before: usize = p.regions.iter().map(|r| r.blocks.len()).sum();
                let after: usize = reread.regions.iter().map(|r| r.blocks.len()).sum();
                if after < before {
                    return Err("生成文件方块数量异常".into());
                }
                let mut current: Vec<u8> = Vec::new();
                std::fs::File::open(&job.file)
                    .map_err(|e| e.to_string())?
                    .read_to_end(&mut current)
                    .map_err(|e| e.to_string())?;
                if hex::encode(Sha256::digest(&current)) != hash {
                    return Err("转换过程中原文件已变化".into());
                }
                Ok(WorkerOutput::Bytes(bytes))
            }
        }
    })
}

// ---------------- 目录与实例（对应原版 core/projectionsIpc.ts activeRoot/library/recordingDirectory） ----------------

fn projection_dir() -> &'static str {
    "schematics"
}

fn active_root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let s = settings::get(app);
    let root = s["activeFolder"]
        .as_str()
        .filter(|v| !v.is_empty())
        .or_else(|| s["gameDir"].as_str().filter(|v| !v.is_empty()))
        .unwrap_or("")
        .to_string();
    let bound = s["folders"]
        .as_array()
        .map(|list| list.iter().any(|f| f["path"].as_str().map(|p| key_of_str(p) == key_of_str(&root)).unwrap_or(false)))
        .unwrap_or(false);
    if root.is_empty() || !bound {
        return Err("请先选择已登记的游戏文件夹".into());
    }
    Ok(resolve_abs(Path::new(&root)))
}

/// 目标实例的 schematics 目录：投影收集/导入的落点跟随所选实例
/// （实例目录由 instance_directory_state 判定：隔离 = versions/<id>，共享 = 游戏文件夹根）。
fn instance_projections_dir(app: &tauri::AppHandle, target: &Value) -> Result<PathBuf, String> {
    let dir = center_target_dir(app, target)?;
    let schematics = dir.join(projection_dir());
    std::fs::create_dir_all(&schematics).map_err(|e| e.to_string())?;
    Ok(schematics)
}

/// 只读变体：仅解析路径、不创建目录（扫描时用于展示"导入与保存位置"）
fn instance_projections_dir_of(app: &tauri::AppHandle, target: &Value) -> Option<PathBuf> {
    let dir = center_target_dir(app, target).ok()?;
    Some(dir.join(projection_dir()))
}

fn validate_instance_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.len() > 80
        || name == "."
        || name == ".."
        || name.chars().any(|c| "\\/:*?\"<>|".contains(c) || (c as u32) < 0x20)
    {
        return Err("实例名称无效".into());
    }
    Ok(())
}

// 对应原版 core/instanceCenter.ts centerTarget()
fn center_target_dir(app: &tauri::AppHandle, target: &Value) -> Result<PathBuf, String> {
    let id = target["id"].as_str().unwrap_or("");
    validate_instance_name(id)?;
    let folder = target["folder"].as_str().unwrap_or("");
    let s = settings::get(app);
    let registered = s["folders"]
        .as_array()
        .map(|list| list.iter().any(|f| f["path"].as_str().map(|p| same_path(p, folder)).unwrap_or(false)))
        .unwrap_or(false);
    if !registered {
        return Err("游戏文件夹尚未登记".into());
    }
    let real_folder = real_path(Path::new(folder)).map_err(|_| "游戏文件夹尚未登记".to_string())?;
    let folder_s = real_folder.to_string_lossy().into_owned();
    let json = read_version_json(app, &folder_s, id)?;
    let state = installer::instance_directory_state(app, &folder_s, id, &json);
    let declared = state["path"].as_str().unwrap_or("").to_string();
    if declared.is_empty() || !Path::new(&declared).exists() {
        return Err("实例目录不存在".into());
    }
    let dir = real_path(Path::new(&declared)).map_err(|_| "实例目录不存在".to_string())?;
    if !same_path(&declared, &dir.to_string_lossy()) {
        return Err("实例目录不能是符号链接".into());
    }
    if !dir.is_dir() {
        return Err("实例目录不存在".into());
    }
    Ok(dir)
}

fn installed_version_ids(root: &Path) -> (Vec<String>, Vec<String>) {
    let dir = root.join("versions");
    if !dir.exists() {
        return (Vec::new(), Vec::new());
    }
    let rd = match std::fs::read_dir(&dir) {
        Ok(rd) => rd,
        Err(e) => return (Vec::new(), vec![format!("无法读取 versions{COLON}{e}")]),
    };
    let mut ids = Vec::new();
    let mut errors = Vec::new();
    for entry in rd.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if !entry.path().join(format!("{name}.json")).exists() {
            errors.push(format!("{name}{COLON}缺少版本描述 {name}.json"));
        }
        ids.push(name);
    }
    (ids, errors)
}

// ---------------- 目录缓存 / 选择项 ----------------

fn selected_items(app: &tauri::AppHandle, ids: &Value) -> Result<Vec<Stored>, String> {
    let arr = ids.as_array().ok_or("请选择投影文件".to_string())?;
    if arr.is_empty() || arr.len() > SCAN_ENTRY_LIMIT {
        return Err("请选择投影文件".into());
    }
    let s = settings::get(app);
    let bound: HashSet<String> = s["folders"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|f| f["path"].as_str())
        .map(key_of_str)
        .collect();
    let catalog = CATALOG.lock().unwrap();
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for raw in arr {
        let id = raw.as_str().unwrap_or("").to_string();
        if !seen.insert(id.clone()) {
            continue;
        }
        let matched = catalog.get(&id).filter(|stored| stored.source.folder.as_ref().map(|f| bound.contains(&key_of(f))).unwrap_or(false));
        match matched {
            Some(stored) => out.push(stored.clone()),
            None => return Err("投影所属文件夹已解除绑定，请刷新列表".into()),
        }
    }
    Ok(out)
}

fn entry_size(item: &Stored) -> u64 {
    item.entry["size"].as_u64().unwrap_or(0)
}

fn entry_modified(item: &Stored) -> f64 {
    item.entry["modified"].as_f64().unwrap_or(0.0)
}

fn entry_kind(item: &Stored) -> String {
    item.entry["kind"].as_str().unwrap_or("litematic").to_string()
}

fn entry_name(item: &Stored) -> String {
    item.entry["name"].as_str().unwrap_or("").to_string()
}

fn current_file(item: &Stored) -> Result<PathBuf, String> {
    let file = safe_path(&item.source.root, &item.rel, false).map_err(|e| e.message)?;
    let meta = stat_of(&file)?;
    if !meta.is_file() || meta.is_symlink() || meta.len() != entry_size(item) || mtime_ms(&meta) != entry_modified(item) {
        return Err("投影已变化，请刷新列表后重试".into());
    }
    Ok(file)
}

fn drag_path(root: &Path, rel: &str) -> Result<PathBuf, String> {
    safe_relative(rel)?;
    let base = resolve_abs(root);
    let base_stat = stat_of(&base)?;
    if !base_stat.is_dir() || base_stat.is_symlink() {
        return Err("目录不是普通目录".into());
    }
    let mut current = base;
    for part in rel.split('/') {
        current = current.join(part);
        if stat_of(&current)?.is_symlink() {
            return Err("不跟随符号链接".into());
        }
    }
    let meta = stat_of(&current)?;
    if !meta.is_file() && !meta.is_dir() {
        return Err("不支持拖出特殊文件".into());
    }
    Ok(current)
}

// ---------------- 回收站（对应原版 core/recycleFile.ts） ----------------

fn recycle_bin_delete(target: &Path) -> Result<(), String> {
    let escaped = target.to_string_lossy().replace('\'', "''");
    let method = if target.is_dir() { "DeleteDirectory" } else { "DeleteFile" };
    let script = format!(
        "Add-Type -AssemblyName Microsoft.VisualBasic; [Microsoft.VisualBasic.FileIO.FileSystem]::{method}('{escaped}', 'OnlyErrorDialogs', 'SendToRecycleBin')"
    );
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .map_err(|e| e.to_string())?;
    if output.status.success() && !target.exists() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        Err("系统未移除该文件".into())
    } else {
        Err(stderr)
    }
}

pub(crate) fn recycle_file(directory: &Path, name: &str) -> Result<(), String> {
    let bad_name = name.is_empty()
        || name == "."
        || name == ".."
        || name.chars().any(|c| "\\/:*?\"<>|\0".contains(c) || (c as u32) < 0x20)
        || name.ends_with('.')
        || name.ends_with(' ');
    if bad_name {
        return Err("文件名称无效，未删除任何文件".into());
    }
    let parent = resolve_abs(directory);
    let target = parent.join(name);
    if target.parent() != Some(parent.as_path()) {
        return Err("文件路径越界".into());
    }
    let parent_stat = stat_of(&parent)?;
    if !parent_stat.is_dir() || parent_stat.is_symlink() {
        return Err("资源目录是链接或不是文件夹，请先确认实际位置".into());
    }
    let stat = stat_of(&target)?;
    if stat.is_symlink() {
        return Err("文件是链接，请在系统文件管理器中确认实际位置".into());
    }
    let real_parent = real_path(&parent)?;
    let real_target = real_path(&target)?;
    if real_target.parent() != Some(real_parent.as_path()) {
        return Err("文件实际路径越界".into());
    }
    let current = stat_of(&target)?;
    #[cfg(unix)]
    let moved = current.is_symlink()
        || std::os::unix::fs::MetadataExt::ino(&current) != std::os::unix::fs::MetadataExt::ino(&stat)
        || std::os::unix::fs::MetadataExt::dev(&current) != std::os::unix::fs::MetadataExt::dev(&stat);
    #[cfg(not(unix))]
    let moved = current.is_symlink() || file_id(&current) != file_id(&stat);
    if moved {
        return Err("文件已变化，请刷新后重试".into());
    }
    let failure = match recycle_bin_delete(&real_target) {
        Ok(()) => match std::fs::symlink_metadata(&real_target) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => e.to_string(),
            Ok(_) => "系统未移除该文件".to_string(),
        },
        Err(e) => e,
    };
    Err(format!("无法移入系统回收站，未执行永久删除。请检查文件占用、权限及磁盘的回收站支持：{name}{COLON}{failure}"))
}

// ---------------- 复制（对应原版 core/recordingFiles.ts copyRecording()，原版投影直接复用） ----------------

fn sha256_file(path: &Path, rec: Option<&TaskRecord>) -> Result<String, String> {
    if let Some(task) = rec {
        tasks::wait_while_paused(task);
        if task.cancelled.load(Ordering::SeqCst) {
            return Err("已取消".into());
        }
    }
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut reader = std::io::BufReader::new(file);
    let mut digest = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer).map_err(|e| e.to_string())?;
        if read == 0 {
            break;
        }
        if let Some(task) = rec {
            tasks::wait_while_paused(task);
            if task.cancelled.load(Ordering::SeqCst) {
                return Err("已取消".into());
            }
        }
        digest.update(&buffer[..read]);
    }
    Ok(hex::encode(digest.finalize()))
}

struct StageGuard(PathBuf);

impl Drop for StageGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn copy_projection(source: &Path, destination: &Path, rec: Option<&TaskRecord>, on_bytes: &mut dyn FnMut(u64)) -> Result<PathBuf, String> {
    const CHANGED: &str = "录像在复制期间发生变化，请停止录制后重试";
    let before = stat_of(source)?;
    if !before.is_file() || before.is_symlink() {
        return Err("录像不是普通文件".into());
    }
    let root = real_path(destination)?;
    if stat_of(destination)?.is_symlink() {
        return Err("目标目录不能是链接".into());
    }
    let stage = root.join(format!(".kamucl-recording-{}.part", uuid_v4()));
    let _guard = StageGuard(stage.clone());
    let mut reader = std::fs::File::open(source).map_err(|e| e.to_string())?;
    let mut writer = std::fs::File::create_new(&stage).map_err(|e| e.to_string())?;
    let mut digest = Sha256::new();
    let mut transferred = 0u64;
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        if let Some(task) = rec {
            tasks::wait_while_paused(task);
            if task.cancelled.load(Ordering::SeqCst) {
                return Err("已取消".into());
            }
        }
        let read = reader.read(&mut buffer).map_err(|e| e.to_string())?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
        writer.write_all(&buffer[..read]).map_err(|e| e.to_string())?;
        transferred += read as u64;
        on_bytes(transferred);
    }
    writer.flush().map_err(|e| e.to_string())?;
    drop(writer);
    let after = stat_of(source)?;
    let sha = hex::encode(digest.finalize());
    if after.is_symlink() || file_id(&after) != file_id(&before) || after.len() != before.len() || mtime_ms(&after) != mtime_ms(&before) {
        return Err(CHANGED.into());
    }
    if sha != sha256_file(source, rec)? || sha != sha256_file(&stage, rec)? {
        return Err(CHANGED.into());
    }
    if let Some(task) = rec {
        if task.cancelled.load(Ordering::SeqCst) {
            return Err("已取消".into());
        }
    }
    let stem = source.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = source.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let source_abs = resolve_abs(source);
    for index in 0..10000usize {
        let name = if index == 0 {
            format!("{stem}{ext}")
        } else {
            format!("{stem} ({index}){ext}")
        };
        let dest = safe_path(&root, &name, true).map_err(|e| e.message)?;
        if resolve_abs(&dest) == source_abs {
            return Ok(source.to_path_buf());
        }
        match std::fs::File::create_new(&dest) {
            Ok(mut out) => {
                let mut from = std::fs::File::open(&stage).map_err(|e| e.to_string())?;
                std::io::copy(&mut from, &mut out).map_err(|e| e.to_string())?;
                out.flush().map_err(|e| e.to_string())?;
                return Ok(dest);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.to_string()),
        }
    }
    Err("同名录像过多，请整理目标目录".into())
}

// ---------------- 扫描（对应原版 core/projectionsIpc.ts scan()） ----------------

fn visit_source(
    rec: &TaskRecord,
    source: &Source,
    kind: &'static str,
    rel: &str,
    depth: usize,
    visited: &mut usize,
    found: &mut HashMap<String, Stored>,
    order: &mut Vec<String>,
) -> Result<(), ScanErr> {
    if rec.cancelled.load(Ordering::SeqCst) {
        return Err(ScanErr::msg("投影任务已取消，原文件保留"));
    }
    *visited += 1;
    if *visited > SCAN_DIR_LIMIT {
        return Err(ScanErr::msg("扫描已达到 20000 个目录/文件上限，请分目录整理"));
    }
    let dir = safe_path(&source.root, rel, true)?;
    let reader = match std::fs::read_dir(&dir) {
        Ok(reader) => reader,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(ScanErr::from(e)),
    };
    let mut listing = Vec::new();
    for entry in reader {
        let entry = entry.map_err(ScanErr::from)?;
        let file_type = entry.file_type().map_err(ScanErr::from)?;
        listing.push((entry.file_name().to_string_lossy().into_owned(), file_type.is_dir(), file_type.is_file(), file_type.is_symlink()));
    }
    let suffix = format!(".{kind}");
    for (name, is_dir, is_file, is_link) in listing {
        if is_link || name.starts_with('.') {
            continue;
        }
        let next = format!("{rel}/{name}");
        if is_dir {
            if depth < 4 {
                let lower = name.to_lowercase();
                if !(lower.ends_with(".cache") || lower.ends_with(".tmp") || lower.ends_with(".del")) {
                    visit_source(rec, source, kind, &next, depth + 1, visited, found, order)?;
                }
            }
            continue;
        }
        if !is_file || !name.to_lowercase().ends_with(&suffix) {
            continue;
        }
        if found.len() >= SCAN_ENTRY_LIMIT {
            return Err(ScanErr::msg("仅显示前 10000 个投影文件"));
        }
        let full = safe_path(&source.root, &next, false)?;
        let meta = std::fs::symlink_metadata(&full).map_err(ScanErr::from)?;
        let id = hex::encode(Sha256::digest(key_of(&full).as_bytes()));
        let mut entry = Map::new();
        if let Some(folder) = &source.folder {
            entry.insert("folder".into(), json!(text_of(folder)));
        }
        if let Some(instance_id) = &source.instance_id {
            entry.insert("instanceId".into(), json!(instance_id));
        }
        entry.insert("id".into(), json!(id.clone()));
        entry.insert("name".into(), json!(name));
        entry.insert("kind".into(), json!(kind));
        entry.insert("size".into(), json!(meta.len()));
        entry.insert("modified".into(), json!(mtime_ms(&meta)));
        entry.insert("source".into(), json!(source.label));
        entry.insert("directory".into(), json!(full.parent().map(text_of).unwrap_or_default()));
        if !found.contains_key(&id) {
            order.push(id.clone());
        }
        found.insert(id, Stored { entry: Value::Object(entry), source: source.clone(), rel: next });
    }
    Ok(())
}

fn worker_json(app: &tauri::AppHandle, job: WorkerJob, task_id: &str, task_title: &str, enabled: bool) -> Result<Value, String> {
    let progress = WorkerProgress { app: app.clone(), task_id: task_id.to_string(), task_title: task_title.to_string(), enabled };
    match run_projection_worker(job, progress)? {
        WorkerOutput::Json(value) => Ok(value),
        WorkerOutput::Bytes(_) => Ok(Value::Null),
    }
}

fn metadata_job(file: &Path, kind: &str) -> WorkerJob {
    WorkerJob { file: file.to_path_buf(), source_format: kind.to_string(), action: "metadata", format: None, version: None, hash: None, choices: None }
}

fn scan_metadata(app: &tauri::AppHandle, item: &Stored, kind: &str, task_id: &str, task_title: &str) -> Result<Value, String> {
    let file = current_file(item)?;
    worker_json(app, metadata_job(&file, kind), task_id, task_title, false)
}

fn scan_inner(
    app: &tauri::AppHandle,
    rec: &TaskRecord,
    task_id: &str,
    task_title: &str,
    scope: &str,
    target: &Value,
) -> Result<Value, String> {
    let target_folder: Option<String> = target["folder"].as_str().map(str::to_string);
    let target_id: Option<String> = target["id"].as_str().map(str::to_string);
    let generation = SCAN_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let root = active_root(app)?;
    // 「导入与保存位置」= 当前选中实例的 schematics（扫不到实例时退回游戏文件夹根）
    let collection = instance_projections_dir_of(app, target)
        .map(|dir| dir.to_string_lossy().to_string())
        .unwrap_or_else(|| root.join(projection_dir()).to_string_lossy().to_string());
    let settings_value = settings::get(app);
    let registered: Vec<Value> = {
        let mut seen: HashSet<String> = HashSet::new();
        settings_value["folders"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|f| seen.insert(key_of_str(f["path"].as_str().unwrap_or(""))))
            .collect()
    };
    let mut sources: Vec<Source> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut instances: Vec<Value> = Vec::new();
    for folder in &registered {
        let root_path = resolve_abs(Path::new(folder["path"].as_str().unwrap_or("")));
        let root_text = text_of(&root_path);
        let label = folder["name"].as_str().filter(|v| !v.is_empty()).map(str::to_string).unwrap_or_else(|| root_text.clone());
        // 实例/文件夹视图只看选定实例所在的文件夹
        if scope == "instance" || scope == "folder" {
            let active = target_folder.as_deref().unwrap_or("");
            if !same_path(&root_text, active) {
                continue;
            }
        }
        // 共享目录模式下的实例其 schematics 就在游戏文件夹根
        sources.push(Source { folder: Some(root_path.clone()), root: root_path.clone(), label: format!("{label} · 共享目录"), instance_id: None });
        let (ids, errors) = installed_version_ids(&root_path);
        for error in errors {
            warnings.push(format!("{root_text}{COLON}{error}"));
        }
        for id in ids {
            if scope == "instance" && Some(id.as_str()) != target_id.as_deref() {
                continue;
            }
            match center_target_dir(app, &json!({ "folder": root_text, "id": id })) {
                Ok(dir) => {
                    sources.push(Source { folder: Some(root_path.clone()), root: dir, label: id.clone(), instance_id: Some(id.clone()) });
                    instances.push(json!({ "folder": root_text, "id": id, "name": format!("{id} · {root_text}") }));
                }
                Err(_) => warnings.push(format!("无法读取实例：{root_text}{COLON}{id}")),
            }
        }
    }
    let mut found: HashMap<String, Stored> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut visited = 0usize;
    for source in &sources {
        if !seen.insert(key_of(&source.root)) {
            continue;
        }
        for kind in KINDS {
            if let Err(e) = visit_source(rec, source, kind, projection_dir(), 0, &mut visited, &mut found, &mut order) {
                if !e.not_found {
                    warnings.push(format!("{}{COLON}{}", source.label, e.message));
                }
            }
            if rec.cancelled.load(Ordering::SeqCst) {
                return Err("已取消".into());
            }
        }
    }
    let total = order.len();
    let mut read = 0usize;
    for id in &order {
        tasks::wait_while_paused(rec);
        if rec.cancelled.load(Ordering::SeqCst) {
            return Err("已取消".into());
        }
        let item = match found.get(id) {
            Some(value) => value.clone(),
            None => continue,
        };
        let size = entry_size(&item);
        let modified = entry_modified(&item);
        let kind = entry_kind(&item);
        let cached = METADATA_CACHE
            .lock()
            .unwrap()
            .get(id)
            .filter(|entry| entry.0 == size && entry.1 == modified)
            .map(|entry| entry.2.clone());
        let metadata = match cached {
            Some(cached) => cached,
            None => {
                let outcome = scan_metadata(app, &item, &kind, task_id, task_title);
                let value = match outcome {
                    Ok(value) => value,
                    Err(e) => {
                        if rec.cancelled.load(Ordering::SeqCst) {
                            return Err("已取消".into());
                        }
                        json!({ "error": e })
                    }
                };
                METADATA_CACHE.lock().unwrap().insert(id.clone(), (size, modified, value.clone()));
                value
            }
        };
        if let Some(stored) = found.get_mut(id) {
            if let Value::Object(target) = &mut stored.entry {
                if let Value::Object(source_map) = &metadata {
                    for (key, value) in source_map {
                        target.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        read += 1;
        tasks::emit_progress(
            app,
            json!({
                "taskId": task_id,
                "taskTitle": task_title,
                "stage": "world",
                "progress": (read as f64 / total.max(1) as f64).min(1.0),
                "text": format!("读取投影 {read}/{total}")
            }),
        );
    }
    {
        let mut cache = METADATA_CACHE.lock().unwrap();
        cache.retain(|id, _| found.contains_key(id));
    }
    if SCAN_GENERATION.load(Ordering::SeqCst) == generation {
        let mut catalog = CATALOG.lock().unwrap();
        catalog.clear();
        for id in &order {
            if let Some(stored) = found.get(id) {
                catalog.insert(id.clone(), stored.clone());
            }
        }
    }
    let mut entries: Vec<Value> = order.iter().filter_map(|id| found.get(id)).map(|stored| stored.entry.clone()).collect();
    entries.sort_by(|a, b| b["modified"].as_f64().unwrap_or(0.0).total_cmp(&a["modified"].as_f64().unwrap_or(0.0)));
    Ok(json!({
        "entries": entries,
        "warnings": warnings,
        "library": collection,
        "instances": instances
    }))
}

fn list(app: &tauri::AppHandle, scope: &str, target: &Value) -> Result<Value, String> {
    let rec = tasks::register_task("投影 · 扫描全部目录", "world");
    let task_id = rec.id.clone();
    let task_title = rec.title.clone();
    let outcome = scan_inner(app, &rec, &task_id, &task_title, scope, target);
    let cancelled = rec.cancelled.load(Ordering::SeqCst);
    let error = outcome.as_ref().err().cloned();
    tasks::task_done(app, &task_id, outcome.is_ok(), error, cancelled, None);
    outcome
}

// ---------------- 操作（collect / export / dispatch / trash） ----------------

fn action_title(action: &str) -> &'static str {
    match action {
        "collect" => "收集",
        "export" => "提取",
        "dispatch" => "复制到实例",
        _ => "移入回收站",
    }
}

fn emit_run_progress(app: &tauri::AppHandle, task_id: &str, task_title: &str, total: u64, base: u64, bytes: u64, processed: usize, count: usize, name: &str) {
    let progress = if total > 0 { ((base + bytes) as f64 / total as f64).min(1.0) } else { 0.0 };
    tasks::emit_progress(
        app,
        json!({
            "taskId": task_id,
            "taskTitle": task_title,
            "stage": "world",
            "progress": progress,
            "text": format!("{processed}/{count} · {name}")
        }),
    );
}

fn run_one(
    app: &tauri::AppHandle,
    rec: &TaskRecord,
    item: &Stored,
    action: &str,
    destination: Option<&Path>,
    task_id: &str,
    task_title: &str,
    total: u64,
    base: u64,
    processed: usize,
    count: usize,
) -> Result<Option<PathBuf>, String> {
    tasks::wait_while_paused(rec);
    if rec.cancelled.load(Ordering::SeqCst) {
        return Err("已取消".into());
    }
    let name = entry_name(item);
    let kind = entry_kind(item);
    emit_run_progress(app, task_id, task_title, total, base, 0, processed, count, &name);
    let file = current_file(item)?;
    if action == "trash" {
        let inner = item.clone();
        let rec_ref: Option<&TaskRecord> = Some(rec);
        with_file_job(&inner.source.root, rec_ref, || -> Result<(), String> {
            // 投影都在某个实例目录下，删除前一律确认该实例未在运行
            assert_instance_idle(app, &inner.source.root)?;
            current_file(&inner)?;
            let full = safe_path(&inner.source.root, &inner.rel, false).map_err(|e| e.message)?;
            let parent = full.parent().map(|p| p.to_path_buf()).unwrap_or_default();
            let leaf = full.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            recycle_file(&parent, &leaf)
        })?;
        return Ok(None);
    }
    worker_json(app, metadata_job(&file, &kind), task_id, task_title, false)?;
    let root = destination.ok_or("未指定投影目标目录".to_string())?.to_path_buf();
    let mut on_bytes = {
        let (name, app_ref, tid, title) = (name.clone(), app, task_id, task_title);
        move |bytes: u64| emit_run_progress(app_ref, tid, title, total, base, bytes, processed, count, &name)
    };
    let mut output: Option<PathBuf> = None;
    {
        let action_owned = action.to_string();
        let file_for_copy = file.clone();
        let root_for_lock = root.clone();
        let rec_ref: Option<&TaskRecord> = Some(rec);
        with_file_job(&root_for_lock, rec_ref, || -> Result<(), String> {
            // 收集/复制到的目标实例正在运行 → 拒绝写入其 schematics。
            // 目标目录是 schematics 本身，实例目录取其父级再做运行中判定。
            if action_owned != "export" {
                if let Some(instance_dir) = root_for_lock.parent() {
                    assert_instance_idle(app, instance_dir)?;
                }
            }
            // export 用用户挑选的目录原样写入；其余 destination 已是目标 schematics 本身
            let dir = root_for_lock.clone();
            let rec_ref: Option<&TaskRecord> = Some(rec);
            let copied = copy_projection(&file_for_copy, &dir, rec_ref, &mut on_bytes)?;
            output = Some(copied);
            Ok(())
        })?;
    }
    Ok(output)
}

fn run(app: &tauri::AppHandle, items: Vec<Stored>, action: &str, destination: Option<&Path>) -> Result<Value, String> {
    let rec = tasks::register_task(&format!("投影文件 · {}", action_title(action)), "world");
    let task_id = rec.id.clone();
    let task_title = rec.title.clone();
    let total: u64 = items.iter().map(entry_size).sum();
    let count = items.len();
    let mut results: Vec<Value> = Vec::new();
    let mut done = 0u64;
    for item in &items {
        let id = item.entry["id"].as_str().unwrap_or("").to_string();
        let name = entry_name(item);
        let size = entry_size(item);
        let processed = results.len();
        let outcome = run_one(app, &rec, item, action, destination, &task_id, &task_title, total, done, processed, count);
        match outcome {
            Ok(path) => {
                let mut row = json!({ "id": id, "name": name, "ok": true });
                if let Some(p) = path {
                    row["path"] = json!(text_of(&p));
                }
                results.push(row);
            }
            Err(e) => {
                let message = if rec.cancelled.load(Ordering::SeqCst) { "任务已取消，原投影保留".to_string() } else { e };
                results.push(json!({ "id": id, "name": name, "ok": false, "error": message }));
            }
        }
        done += size;
    }
    let failed = results.iter().filter(|row| row["ok"].as_bool() != Some(true)).count();
    let cancelled = rec.cancelled.load(Ordering::SeqCst);
    let error = if failed > 0 { Some(format!("{failed} 个投影处理失败，请查看投影页结果")) } else { None };
    tasks::task_done(app, &task_id, failed == 0, error, cancelled, None);
    Ok(Value::Array(results))
}

fn operate(app: &tauri::AppHandle, args: &[Value]) -> Result<Value, String> {
    let request = args.first().cloned().unwrap_or(Value::Null);
    let action = request["action"].as_str().unwrap_or("").to_string();
    let ids = request["ids"].clone();
    let valid_action = matches!(action.as_str(), "collect" | "export" | "dispatch" | "trash");
    let id_len = ids.as_array().map(|a| a.len()).unwrap_or(0);
    if !request.is_object() || !valid_action || !ids.is_array() || id_len < 1 || id_len > SCAN_ENTRY_LIMIT {
        return Err("投影操作参数无效".into());
    }
    let root = active_root(app)?;
    let items = selected_items(app, &ids)?;
    let mut destination: Option<PathBuf> = None;
    match action.as_str() {
        // 收集 = 复制到目标实例的 schematics（默认当前实例，可在弹层里选其它实例）
        "collect" => {
            let target = request
                .get("target")
                .filter(|v| v.is_object())
                .cloned()
                .unwrap_or(Value::Null);
            destination = Some(instance_projections_dir(app, &target)?);
        }
        "dispatch" => {
            let target = request.get("target").filter(|v| v.is_object()).ok_or("请选择目标实例".to_string())?;
            destination = Some(instance_projections_dir(app, target)?);
        }
        "export" => {
            use tauri_plugin_dialog::DialogExt;
            let picked = app.dialog().file().set_title("选择投影提取文件夹").blocking_pick_folder();
            let Some(picked) = picked else { return Ok(Value::Null) };
            destination = Some(picked.into_path().map_err(|e| e.to_string())?);
        }
        _ => {}
    }
    let _ = root;
    run(app, items, &action, destination.as_deref())
}

fn import_at(app: &tauri::AppHandle, paths: &[String], target: Value) -> Result<Value, String> {
    let destination = instance_projections_dir(app, &target)?;
    let mut items: Vec<Stored> = Vec::new();
    for raw in paths {
        let file = PathBuf::from(raw);
        let meta = stat_of(&file)?;
        let rel = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let parent = file.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        let kind = file
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let mut entry = Map::new();
        entry.insert("id".into(), json!(text_of(&file)));
        entry.insert("name".into(), json!(rel.clone()));
        entry.insert("kind".into(), json!(kind));
        entry.insert("size".into(), json!(meta.len()));
        entry.insert("modified".into(), json!(mtime_ms(&meta)));
        entry.insert("source".into(), json!("导入"));
        entry.insert("directory".into(), json!(text_of(&parent)));
        entry.insert("library".into(), json!(false));
        items.push(Stored { entry: Value::Object(entry), source: Source { folder: None, root: parent, label: "导入".into(), instance_id: None }, rel });
    }
    run(app, items, "collect", Some(&destination))
}

/// 对话框选择导入（projections:import 原口径）：挑完复用 import_at
fn import_dialog(app: &tauri::AppHandle, target: Value) -> Result<Value, String> {
    use tauri_plugin_dialog::DialogExt;
    let picked = app
        .dialog()
        .file()
        .set_title("导入投影到当前实例 schematics")
        .add_filter("Minecraft 投影", &["litematic", "schem", "schematic", "nbt"])
        .blocking_pick_files();
    let Some(picked) = picked else { return Ok(Value::Null) };
    let paths: Vec<String> = picked
        .into_iter()
        .filter_map(|raw| raw.into_path().ok().map(|p| p.to_string_lossy().into_owned()))
        .collect();
    if paths.is_empty() {
        return Ok(Value::Null);
    }
    import_at(app, &paths, target)
}

fn open_directory(dir: &Path) -> Result<(), String> {
    let mut command = if cfg!(windows) { std::process::Command::new("explorer.exe") } else { std::process::Command::new("xdg-open") };
    command.arg(dir.as_os_str());
    command.spawn().map_err(|e| e.to_string())?;
    Ok(())
}

fn reveal_item(path: &Path) -> Result<(), String> {
    if cfg!(windows) {
        std::process::Command::new("explorer.exe")
            .arg(format!("/select,{}", path.to_string_lossy()))
            .spawn()
            .map_err(|e| e.to_string())?;
        return Ok(());
    }
    open_directory(path.parent().unwrap_or(path))
}

fn open(app: &tauri::AppHandle, args: &[Value]) -> Result<Value, String> {
    let id = args.first().and_then(|v| v.as_str()).filter(|v| !v.is_empty()).map(str::to_string);
    match id {
        Some(id) => {
            let item = selected_items(app, &json!([id]))?.into_iter().next().ok_or("请选择投影文件".to_string())?;
            reveal_item(&current_file(&item)?)?;
        }
        None => {
            // 无参 = 打开当前（或指定）实例的 schematics
            let target = args.get(1).cloned().unwrap_or(Value::Null);
            let dir = instance_projections_dir(app, &target)?;
            open_directory(&dir)?;
        }
    }
    Ok(Value::Null)
}

// ---------------- 转换通道（对应原版 core/projectionJobs.ts registerProjectionConversion()） ----------------

fn resolve_source(app: &tauri::AppHandle, id: &str) -> Result<(PathBuf, String), String> {
    let item = selected_items(app, &json!([id]))?.into_iter().next().ok_or("请选择投影文件".to_string())?;
    let file = current_file(&item)?;
    Ok((file, entry_kind(&item)))
}

fn worker_progress(app: &tauri::AppHandle, task_id: &str, task_title: &str, enabled: bool) -> WorkerProgress {
    WorkerProgress { app: app.clone(), task_id: task_id.to_string(), task_title: task_title.to_string(), enabled }
}

fn task_wrapper<F>(app: &tauri::AppHandle, title: &str, run: F) -> Result<Value, String>
where
    F: FnOnce(&tauri::AppHandle, &TaskRecord, &str, &str) -> Result<Value, String>,
{
    let rec = tasks::register_task(title, "world");
    let outcome = run(app, &rec, &rec.id, &rec.title);
    let cancelled = rec.cancelled.load(Ordering::SeqCst);
    let error = outcome.as_ref().err().cloned();
    tasks::task_done(app, &rec.id, outcome.is_ok(), error, cancelled, None);
    outcome
}

fn analyze(app: &tauri::AppHandle, args: &[Value]) -> Result<Value, String> {
    let id = args.first().and_then(|v| v.as_str()).unwrap_or("").to_string();
    let format = args.get(1).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let version = args.get(2).and_then(|v| v.as_str()).filter(|v| !v.is_empty()).map(str::to_string);
    let now = now_ms();
    {
        let mut analyses = ANALYSES.lock().unwrap();
        analyses.retain(|_, stored| now - stored.time <= ANALYSIS_TTL_MS);
        if analyses.len() >= ANALYSIS_LIMIT {
            return Err("待确认转换过多，请关闭旧转换窗口".into());
        }
    }
    let (file, kind) = resolve_source(app, &id)?;
    task_wrapper(app, "投影转换 · 分析差异", move |app, _rec, task_id, task_title| {
        let job = WorkerJob {
            file: file.clone(),
            source_format: kind.clone(),
            action: "analyze",
            format: Some(format.clone()),
            version: version.clone(),
            hash: None,
            choices: None,
        };
        let mut view = match run_projection_worker(job, worker_progress(app, task_id, task_title, true))? {
            WorkerOutput::Json(value) => value,
            WorkerOutput::Bytes(_) => Value::Null,
        };
        let new_id = uuid_v4();
        if let Value::Object(map) = &mut view {
            map.insert("id".into(), json!(new_id.clone()));
        }
        ANALYSES.lock().unwrap().insert(
            new_id,
            StoredAnalysis { view: view.clone(), source_id: id.clone(), file: file.clone(), time: now_ms() },
        );
        Ok(view)
    })
}

fn convert(app: &tauri::AppHandle, args: &[Value]) -> Result<Value, String> {
    let id = args.first().and_then(|v| v.as_str()).unwrap_or("").to_string();
    let choices = args.get(1).cloned().unwrap_or(Value::Null);
    let stored = ANALYSES.lock().unwrap().get(&id).map(|v| (v.view.clone(), v.source_id.clone(), v.file.clone(), v.time));
    let Some((view, source_id, analysis_file, time)) = stored else {
        return Err("转换分析已过期，请重新分析".into());
    };
    if now_ms() - time > ANALYSIS_TTL_MS {
        return Err("转换分析已过期，请重新分析".into());
    }
    if choices.is_null() || !(choices.is_object() || choices.is_array()) || serde_json::to_string(&choices).map(|s| s.len()).unwrap_or(usize::MAX) > CHOICES_LIMIT {
        return Err("转换选择无效".into());
    }
    let (current, current_format) = resolve_source(app, &source_id)?;
    let source_format = view["sourceFormat"].as_str().unwrap_or("").to_string();
    if current != analysis_file || source_format != current_format {
        return Err("投影来源已变化，请重新扫描并分析".into());
    }
    let target_format = view["targetFormat"].as_str().unwrap_or("").to_string();
    let target_version = view["targetVersion"].as_str().map(str::to_string);
    let source_hash = view["sourceHash"].as_str().unwrap_or("").to_string();
    task_wrapper(app, "投影转换 · 生成与验证", move |app, rec, task_id, task_title| {
        if rec.cancelled.load(Ordering::SeqCst) {
            return Err("投影任务已取消，原文件保留".into());
        }
        let job = WorkerJob {
            file: analysis_file.clone(),
            source_format: source_format.clone(),
            action: "convert",
            format: Some(target_format.clone()),
            version: target_version.clone(),
            hash: Some(source_hash.clone()),
            choices: Some(choices.clone()),
        };
        let bytes = match run_projection_worker(job, worker_progress(app, task_id, task_title, true))? {
            WorkerOutput::Bytes(value) => value,
            WorkerOutput::Json(_) => Vec::new(),
        };
        let dir = analysis_file.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        let stem = analysis_file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let mut output = PathBuf::new();
        {
            let rec_ref: Option<&TaskRecord> = Some(rec);
            let target = target_format.clone();
            with_file_job(&dir, rec_ref, || -> Result<(), String> {
                if rec.cancelled.load(Ordering::SeqCst) {
                    return Err("投影任务已取消，原文件保留".into());
                }
                let temp = dir.join(format!(".kamucl-convert-{}.part", uuid_v4()));
                let wrote = (|| -> Result<(), String> {
                    let mut handle = std::fs::File::create_new(&temp).map_err(|e| e.to_string())?;
                    handle.write_all(&bytes).map_err(|e| e.to_string())?;
                    handle.flush().map_err(|e| e.to_string())?;
                    drop(handle);
                    if rec.cancelled.load(Ordering::SeqCst) {
                        return Err("投影任务已取消，原文件保留".into());
                    }
                    for index in 1..10000usize {
                        let name = if index == 1 {
                            format!("{stem}.{target}")
                        } else {
                            format!("{stem}-{index}.{target}")
                        };
                        let candidate = dir.join(name);
                        match std::fs::hard_link(&temp, &candidate) {
                            Ok(_) => {
                                output = candidate;
                                break;
                            }
                            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                            Err(e) => return Err(e.to_string()),
                        }
                    }
                    if output.as_os_str().is_empty() {
                        return Err("转换副本重名过多".into());
                    }
                    if rec.cancelled.load(Ordering::SeqCst) {
                        let _ = std::fs::remove_file(&output);
                        return Err("投影任务已取消，原文件保留".into());
                    }
                    Ok(())
                })();
                let _ = std::fs::remove_file(&temp);
                wrote
            })?;
        }
        ANALYSES.lock().unwrap().remove(id.as_str());
        tasks::emit_progress(
            app,
            json!({
                "taskId": task_id,
                "taskTitle": task_title,
                "stage": "world",
                "progress": 1,
                "text": "生成文件已验证，原文件哈希保持不变"
            }),
        );
        Ok(json!({
            "path": text_of(&output),
            "name": output.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
        }))
    })
}

fn discard_analysis(args: &[Value]) -> Result<Value, String> {
    let id = args.first().and_then(|v| v.as_str()).unwrap_or("");
    ANALYSES.lock().unwrap().remove(id);
    Ok(Value::Null)
}

// ---------------- projections:drag（ipcMain.on 等价） ----------------

fn handle_drag(app: &tauri::AppHandle, args: &[Value]) {
    let outcome: Result<(), String> = (|| {
        let items = selected_items(app, args.first().unwrap_or(&Value::Null))?;
        if items.len() > DRAG_LIMIT {
            return Err(format!("每次最多拖出 {DRAG_LIMIT} 个投影，请分批选择"));
        }
        let mut paths: Vec<String> = Vec::with_capacity(items.len());
        for item in &items {
            let file = drag_path(&item.source.root, &item.rel)?;
            let meta = stat_of(&file)?;
            if !meta.is_file() || meta.len() != entry_size(item) || mtime_ms(&meta) != entry_modified(item) {
                return Err("投影已变化，请刷新后重试".into());
            }
            paths.push(file.to_string_lossy().to_string());
        }
        crate::dragout::validate_paths(&paths)?;
        crate::dragout::start_drag_out(&paths).map(|_| ())
    })();
    if let Err(e) = outcome {
        installer::emit_event(app, "files:dragError", &Value::String(e));
    }
}

pub fn invoke(app: &tauri::AppHandle, channel: &str, args: &[Value]) -> Option<Result<Value, String>> {
    match channel {
                "projections:list" => {
            let target = args.get(1).cloned().unwrap_or(json!({}));
            Some(list(
                app,
                args.first().and_then(|v| v.as_str()).unwrap_or("instance"),
                &target,
            ))
        }
        "projections:operate" => Some(operate(app, args)),
        "projections:import" => Some(import_dialog(app, args.first().cloned().unwrap_or(Value::Null))),
        // 拖拽导入（projections:importAt）：带路径 + 目标实例，跳过对话框
        "projections:importAt" => Some(import_at(
            app,
            &args.first()
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect::<Vec<String>>()
                })
                .unwrap_or_default(),
            args.get(1).cloned().unwrap_or(Value::Null),
        )),
        "projections:open" => Some(open(app, args)),
        "projections:analyze" => Some(analyze(app, args)),
        "projections:convert" => Some(convert(app, args)),
        "projections:discardAnalysis" => Some(discard_analysis(args)),
        "projections:versions" => Some(Ok(projection_versions_json())),
        _ => None,
    }
}

pub fn send(app: &tauri::AppHandle, channel: &str, args: &[Value]) -> Option<Result<(), String>> {
    match channel {
        "projections:drag" => {
            handle_drag(app, args);
            Some(Ok(()))
        }
        _ => None,
    }
}
