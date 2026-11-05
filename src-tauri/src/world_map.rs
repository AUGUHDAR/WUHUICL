//! 世界 2D 地图后端：读取存档 region/*.mca（Anvil），逐列取地表方块着色，生成瓦片 PNG。
//!
//! 通道：worldmap:info（世界元信息 + 各维度 region 清单 + 出生点）、
//! worldmap:renderTile（瓦片渲染，返回磁盘缓存 PNG 的绝对路径，前端经 kamucl-asset 协议加载）。
//!
//! 瓦片级别：level 0 = 1px/方块（一个 region 文件 = 512×512 px 瓦片）；
//! level k 每像素覆盖 2^k 方块，瓦片覆盖 2^k × 2^k 个 region。各级瓦片**按级别直接
//! 采样渲染**，不做自底向上建塔——粗级别只解析含采样列的少数区块（level 6 约 1/16、
//! level 8 约 1/256），首次浏览大世界也是秒级出图；k≤4 的渲染会顺带落 level-0 缓存
//! （反正全区块都解析了），之后任何级别都能对它零解析抽样。region 的 level-0 瓦片在
//! 缓存里时，一切更粗级别都直接从图上抽样。
//! 缓存目录 launcher-managed/map-tile-cache/<世界key>/<维度>/，文件名含 region
//! mtime/size 签名，存档一改自动失效；写出走 .part 原子改名（同截图缩略图口径）。
//!
//! 性能：区块解析按 std::thread::scope 分片并行（无 rayon，不加依赖），并用全局信号量
//! 把「同时活着的解析线程数」压在物理核数内，多瓦片并发请求不会过订。

use std::collections::{BTreeMap, HashMap};
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use quartz_nbt::NbtCompound;
use serde::Deserialize;
use serde_json::{json, Value};

const MAX_LEVEL: u32 = 8;
const TILE_PX: usize = 512;
/// 单 region 文件读入上限（理论上限 32×32×1MB，正常远小于此）
const REGION_FILE_LIMIT: u64 = 256 * 1024 * 1024;
const TILE_CACHE_LIMIT: usize = 4000;
/// region 清单进程内缓存 TTL：浏览地图期间世界被改动，最多延迟这么久反映到瓦片签名
const REGION_LISTING_TTL: Duration = Duration::from_millis(1500);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MapDim {
    Overworld,
    Nether,
    End,
}

impl MapDim {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "overworld" => Ok(Self::Overworld),
            "nether" => Ok(Self::Nether),
            "end" => Ok(Self::End),
            _ => Err(format!("未知维度：{value}")),
        }
    }

    fn key(&self) -> &'static str {
        match self {
            Self::Overworld => "overworld",
            Self::Nether => "nether",
            Self::End => "end",
        }
    }

    /// 维度在 dimensions/<命名空间>/<维度>/ 布局下的目录名（服务端/部分整合包存档用）
    fn dimension_key(&self) -> &'static str {
        match self {
            Self::Overworld => "overworld",
            Self::Nether => "the_nether",
            Self::End => "the_end",
        }
    }
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct RegionEntry {
    x: i32,
    z: i32,
    mtime_ms: u64,
    size: u64,
}

struct WorldCtx {
    path: PathBuf,
    key: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InfoQuery {
    instance_id: String,
    world: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TileQuery {
    instance_id: String,
    world: String,
    dimension: String,
    level: u32,
    tx: i32,
    tz: i32,
}

/*
 * WUHUICL IPC 分发（args[0] 为请求对象）。
 */
pub fn invoke(
    app: &tauri::AppHandle,
    channel: &str,
    args: &[Value],
) -> Option<Result<Value, String>> {
    let request = || args.first().cloned().unwrap_or(Value::Null);
    match channel {
        "worldmap:info" => {
            let parsed: InfoQuery = match serde_json::from_value(request()) {
                Ok(v) => v,
                Err(e) => return Some(Err(e.to_string())),
            };
            Some(world_info(app, &parsed))
        }
        "worldmap:renderTile" => {
            let parsed: TileQuery = match serde_json::from_value(request()) {
                Ok(v) => v,
                Err(e) => return Some(Err(e.to_string())),
            };
            Some(render_tile(app, &parsed))
        }
        _ => None,
    }
}

fn sanitize_world_name(world: &str) -> Result<(), String> {
    if world.is_empty()
        || world == "."
        || world == ".."
        || world.contains(['/', '\\', ':'])
        || world.chars().any(|c| c.is_control())
    {
        return Err("存档目录名不合法".into());
    }
    Ok(())
}

fn resolve_world(app: &tauri::AppHandle, instance_id: &str, world: &str) -> Result<WorldCtx, String> {
    sanitize_world_name(world)?;
    let game_dir = crate::schematic_preview::instance_full_path(app, instance_id)?;
    let path = game_dir.join("saves").join(world);
    if !path.join("level.dat").is_file() {
        return Err("存档不存在或已被删除".into());
    }
    let key = format!("{:016x}", fnv64a(&format!("{instance_id}\u{0}{world}")));
    Ok(WorldCtx { path, key })
}

// ---------------- info：世界元信息 + region 清单 ----------------

fn world_info(app: &tauri::AppHandle, query: &InfoQuery) -> Result<Value, String> {
    let world = resolve_world(app, &query.instance_id, &query.world)?;
    let raw = std::fs::read(world.path.join("level.dat")).map_err(|e| e.to_string())?;
    let root = read_nbt_any(&raw)?;
    let data = root.get::<_, &NbtCompound>("Data").unwrap_or(&root);
    let name = data
        .get::<_, &str>("LevelName")
        .map(str::to_string)
        .unwrap_or_else(|_| query.world.clone());
    let data_version = data.get::<_, i32>("DataVersion").ok();
    let supported = data_version.map(|v| v >= 1_519).unwrap_or(false);
    let spawn = json!({
        "x": data.get::<_, i32>("SpawnX").unwrap_or(0),
        "z": data.get::<_, i32>("SpawnZ").unwrap_or(0),
    });

    let mut dimensions = BTreeMap::new();
    for dim in [MapDim::Overworld, MapDim::Nether, MapDim::End] {
        let regions = list_regions_cached(&world.path, dim).unwrap_or_default();
        dimensions.insert(
            dim.key(),
            regions
                .iter()
                .map(|r| json!({"x": r.x, "z": r.z, "mtimeMs": r.mtime_ms, "size": r.size}))
                .collect::<Vec<_>>(),
        );
    }

    Ok(json!({
        "name": name,
        "worldPath": world.path.to_string_lossy(),
        "dataVersion": data_version,
        "supported": supported,
        "unsupportedReason": if supported {
            Value::Null
        } else {
            Value::String("该存档由 1.13 之前的版本创建，地图预览暂不支持".into())
        },
        "spawn": spawn,
        "dimensions": dimensions,
    }))
}

fn read_nbt_any(raw: &[u8]) -> Result<NbtCompound, String> {
    let flavors = [
        quartz_nbt::io::Flavor::GzCompressed,
        quartz_nbt::io::Flavor::Uncompressed,
        quartz_nbt::io::Flavor::ZlibCompressed,
    ];
    let mut last_error = None;
    for flavor in flavors {
        match quartz_nbt::io::read_nbt(&mut Cursor::new(raw), flavor) {
            Ok((root, _)) => return Ok(root),
            Err(error) => last_error = Some(error),
        }
    }
    Err(match last_error {
        Some(error) => format!("NBT 解析失败：{error}"),
        None => "NBT 解析失败".into(),
    })
}

// ---------------- region 清单（带 TTL 进程缓存） ----------------

/// region 目录定位：先按标准单人布局（主世界 region/、下界 DIM-1/region/、末地 DIM1/region/），
/// 目录不存在再回退 dimensions/minecraft/<维度>/region/（服务端搬来的存档常用这种布局）。
fn region_dir(world: &Path, dim: MapDim) -> PathBuf {
    let vanilla = match dim {
        MapDim::Overworld => world.join("region"),
        MapDim::Nether => world.join("DIM-1").join("region"),
        MapDim::End => world.join("DIM1").join("region"),
    };
    if vanilla.is_dir() {
        return vanilla;
    }
    world
        .join("dimensions")
        .join("minecraft")
        .join(dim.dimension_key())
        .join("region")
}

fn parse_region_name(name: &str) -> Option<(i32, i32)> {
    let rest = name.strip_prefix("r.")?.strip_suffix(".mca")?;
    let (x, z) = rest.split_once('.')?;
    Some((x.parse().ok()?, z.parse().ok()?))
}

fn mtime_ms(meta: &std::fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn list_regions(world: &Path, dim: MapDim) -> Result<Vec<RegionEntry>, String> {
    let entries = match std::fs::read_dir(region_dir(world, dim)) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.to_string()),
    };
    let mut regions = Vec::new();
    for entry in entries.flatten() {
        let Some((x, z)) = parse_region_name(&entry.file_name().to_string_lossy()) else {
            continue;
        };
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        regions.push(RegionEntry { x, z, mtime_ms: mtime_ms(&meta), size: meta.len() });
    }
    regions.sort_by_key(|r| (r.x, r.z));
    Ok(regions)
}

fn region_listing_cache() -> &'static Mutex<HashMap<(PathBuf, &'static str), (Instant, Result<Vec<RegionEntry>, String>)>> {
    static CACHE: OnceLock<Mutex<HashMap<(PathBuf, &'static str), (Instant, Result<Vec<RegionEntry>, String>)>>> =
        OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn list_regions_cached(world: &Path, dim: MapDim) -> Result<Vec<RegionEntry>, String> {
    let mut cache = region_listing_cache().lock().unwrap();
    if let Some((at, cached)) = cache.get(&(world.to_path_buf(), dim.key())) {
        if at.elapsed() < REGION_LISTING_TTL {
            return cached.clone();
        }
    }
    let fresh = list_regions(world, dim);
    cache.insert(
        (world.to_path_buf(), dim.key()),
        (Instant::now(), fresh.clone()),
    );
    fresh
}

// ---------------- 瓦片渲染入口 ----------------

static TILE_WRITES: AtomicU32 = AtomicU32::new(0);
static TILE_PART_SEQ: AtomicU64 = AtomicU64::new(0);

fn tile_cache_dir(app: &tauri::AppHandle, world_key: &str, dim: MapDim) -> PathBuf {
    let dir = crate::settings::user_data_dir(app)
        .join("launcher-managed")
        .join("map-tile-cache")
        .join(world_key)
        .join(dim.key());
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn render_tile(app: &tauri::AppHandle, query: &TileQuery) -> Result<Value, String> {
    let world = resolve_world(app, &query.instance_id, &query.world)?;
    let dim = MapDim::parse(&query.dimension)?;
    if query.level > MAX_LEVEL {
        return Err("缩放级别超出范围".into());
    }
    match ensure_tile(app, &world, dim, query.level, query.tx, query.tz)? {
        Some(path) => Ok(json!({ "path": path.to_string_lossy() })),
        None => Ok(json!({ "empty": true })),
    }
}

/// 渲染或复用一张瓦片；None = 该范围内没有任何已生成区块。
fn ensure_tile(
    app: &tauri::AppHandle,
    world: &WorldCtx,
    dim: MapDim,
    level: u32,
    tx: i32,
    tz: i32,
) -> Result<Option<PathBuf>, String> {
    let regions = list_regions_cached(&world.path, dim)?;
    let span = 1i64 << level;
    let (lo_x, hi_x) = (tx as i64 * span, (tx as i64 + 1) * span);
    let (lo_z, hi_z) = (tz as i64 * span, (tz as i64 + 1) * span);
    let footprint: Vec<RegionEntry> = regions
        .iter()
        .filter(|r| (r.x as i64) >= lo_x && (r.x as i64) < hi_x && (r.z as i64) >= lo_z && (r.z as i64) < hi_z)
        .cloned()
        .collect();
    if footprint.is_empty() {
        return Ok(None);
    }

    let mut signature = format!("{}\u{0}{}\u{0}{level}\u{0}{tx}\u{0}{tz}\u{0}", world.key, dim.key());
    for r in &footprint {
        signature.push_str(&format!("{},{},{},{}|", r.x, r.z, r.mtime_ms, r.size));
    }
    let cache = tile_cache_dir(app, &world.key, dim)
        .join(format!("L{level}_{tx}_{tz}_{:016x}.png", fnv64a(&signature)));
    if cache.is_file() {
        return Ok(Some(cache));
    }

    let rgba = render_tile_pixels(app, &world, dim, level, tx, tz, &footprint)?;
    write_tile_png(&cache, &rgba)?;
    maybe_prune_tile_cache(app, &world.key, dim);
    Ok(Some(cache))
}

fn write_tile_png(target: &Path, rgba: &[u8]) -> Result<(), String> {
    let image = image::RgbaImage::from_raw(TILE_PX as u32, TILE_PX as u32, rgba.to_vec())
        .ok_or("瓦片像素缓冲尺寸不正确")?;
    let file_name = target.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let staged = target.with_file_name(format!(
        ".{}.{}.{}.part",
        file_name,
        std::process::id(),
        // 同一瓦片被并发渲染时各自写独立临时文件，避免互相截断出半截 PNG
        TILE_PART_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    image
        .save_with_format(&staged, image::ImageFormat::Png)
        .map_err(|e| format!("瓦片 PNG 编码失败：{e}"))?;
    // 同 key 并发生成：已有人写好就直接复用，避免覆盖半截文件
    match std::fs::rename(&staged, target) {
        Ok(()) => Ok(()),
        Err(_) if target.is_file() => {
            let _ = std::fs::remove_file(&staged);
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::remove_file(&staged);
            Err(e.to_string())
        }
    }
}

/// 缓存超限时清理最旧的一半（瓦片随时可重建，删除无损）
fn maybe_prune_tile_cache(app: &tauri::AppHandle, world_key: &str, dim: MapDim) {
    if TILE_WRITES.fetch_add(1, Ordering::Relaxed) % 64 != 0 {
        return;
    }
    let dir = tile_cache_dir(app, world_key, dim);
    let Ok(rd) = std::fs::read_dir(&dir) else { return };
    let mut rows: Vec<(SystemTime, PathBuf)> = rd
        .flatten()
        .filter(|e| e.path().extension().map(|x| x == "png").unwrap_or(false))
        .filter_map(|e| {
            let modified = e.metadata().ok()?.modified().ok()?;
            Some((modified, e.path()))
        })
        .collect();
    if rows.len() < TILE_CACHE_LIMIT {
        return;
    }
    rows.sort_by_key(|(t, _)| *t);
    for (_, path) in rows.iter().take(rows.len() / 2) {
        let _ = std::fs::remove_file(path);
    }
}

// ---------------- 瓦片渲染：按级别直接采样 ----------------
//
// 自底向上建塔在大世界首次浏览时要全量扫描（数百 region × 千区块），不可接受。
// level k 只需每个采样列（间隔 2^k 方块）的地表颜色：
// - region 的 level-0 瓦片已在缓存 → 从图上抽样，零解析；
// - 否则只解析含采样列的区块（level ≥ 5 时是全区块的 1/4 ~ 1/256）。

fn render_tile_pixels(
    app: &tauri::AppHandle,
    world: &WorldCtx,
    dim: MapDim,
    level: u32,
    tx: i32,
    tz: i32,
    footprint: &[RegionEntry],
) -> Result<Vec<u8>, String> {
    let shift = 1_i64 << level; // 每输出像素覆盖的方块边长
    let samples_per_region = (TILE_PX as i64) >> level; // 单个 region 在输出里占的像素宽
    let mut out = vec![0_u8; TILE_PX * TILE_PX * 4];
    for region in footprint {
        let px0f = (region.x as i64 - tx as i64 * shift) * samples_per_region;
        let pz0f = (region.z as i64 - tz as i64 * shift) * samples_per_region;
        let px0 = px0f.max(0) as usize;
        let pz0 = pz0f.max(0) as usize;
        let px1 = (px0f + samples_per_region).clamp(0, TILE_PX as i64) as usize;
        let pz1 = (pz0f + samples_per_region).clamp(0, TILE_PX as i64) as usize;
        if px0 >= px1 || pz0 >= pz1 {
            continue;
        }
        render_region_samples(app, world, dim, level, region, px0f, pz0f, px0, px1, pz0, pz1, &mut out)?;
    }
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
fn render_region_samples(
    app: &tauri::AppHandle,
    world: &WorldCtx,
    dim: MapDim,
    level: u32,
    region: &RegionEntry,
    px0f: i64,
    pz0f: i64,
    px0: usize,
    px1: usize,
    pz0: usize,
    pz1: usize,
    out: &mut [u8],
) -> Result<(), String> {
    // level-0 缓存命中 → 纯图像抽样
    let l0_cache = level0_cache_path(app, world, dim, region);
    if l0_cache.is_file() {
        if let Ok(decoded) = image::open(&l0_cache) {
            let img = decoded.to_rgba8();
            for pz in pz0..pz1 {
                let mz = (((pz as i64 - pz0f) as usize) << level) as u32;
                for px in px0..px1 {
                    let mx = (((px as i64 - px0f) as usize) << level) as u32;
                    let sample = img.get_pixel(mx, mz).0;
                    let at = (pz * TILE_PX + px) * 4;
                    out[at..at + 4].copy_from_slice(&sample);
                }
            }
            return Ok(());
        }
    }

    // 未命中：解析含采样列的区块。m = region 内的采样序号（0..512>>level），local = m<<level 方块
    let m0x = (px0 as i64 - px0f) as usize;
    let m1x = (px1 as i64 - px0f) as usize;
    let m0z = (pz0 as i64 - pz0f) as usize;
    let m1z = (pz1 as i64 - pz0f) as usize;
    let chunk_step = ((1_usize << level) >> 4).max(1);
    let cx0 = (m0x << level) >> 4;
    let cx1 = ((m1x - 1) << level) >> 4;
    let cz0 = (m0z << level) >> 4;
    let cz1 = ((m1z - 1) << level) >> 4;
    let mut needed = Vec::new();
    let mut cz = cz0;
    loop {
        let mut cx = cx0;
        loop {
            needed.push((cx, cz));
            if cx == cx1 {
                break;
            }
            cx = (cx + chunk_step).min(cx1);
        }
        if cz == cz1 {
            break;
        }
        cz = (cz + chunk_step).min(cz1);
    }
    let columns = parse_region_chunks(&world.path, dim, region, &needed)?;
    for pz in pz0..pz1 {
        let mz = (pz as i64 - pz0f) as usize;
        for px in px0..px1 {
            let mx = (px as i64 - px0f) as usize;
            let local_x = mx << level;
            let local_z = mz << level;
            let Some(cols) = columns.get(&(local_x >> 4, local_z >> 4)) else {
                continue;
            };
            let color = cols[((local_z & 15) << 4) | (local_x & 15)];
            let pixel = (pz * TILE_PX + px) * 4;
            if color == 0 {
                continue; // 未生成/纯空气 → 透明
            }
            out[pixel] = (color >> 16) as u8;
            out[pixel + 1] = (color >> 8) as u8;
            out[pixel + 2] = color as u8;
            out[pixel + 3] = 255;
        }
    }
    // k≤4 且 region 被完整覆盖时全区块都已解析，顺带落 level-0 缓存：
    // 之后任何级别（含放大后的本 region）都能对它零解析抽样
    let samples_per_region = (TILE_PX as i64) >> level;
    let fully_covered = px0f >= 0
        && pz0f >= 0
        && px1 - px0 == samples_per_region as usize
        && pz1 - pz0 == samples_per_region as usize;
    if level > 0 && level <= 4 && fully_covered {
        write_level0_byproduct(app, world, dim, region, &columns);
    }
    Ok(())
}

/// level-0 瓦片缓存路径（签名口径与 ensure_tile 的 level 0 完全一致）
fn level0_cache_path(app: &tauri::AppHandle, world: &WorldCtx, dim: MapDim, region: &RegionEntry) -> PathBuf {
    let mut signature = format!(
        "{}\u{0}{}\u{0}0\u{0}{}\u{0}{}\u{0}",
        world.key, dim.key(), region.x, region.z
    );
    signature.push_str(&format!("{},{},{},{}|", region.x, region.z, region.mtime_ms, region.size));
    tile_cache_dir(app, &world.key, dim)
        .join(format!("L0_{}_{}_{:016x}.png", region.x, region.z, fnv64a(&signature)))
}

/// 把整 region 的区块颜色列拼成 512×512 level-0 瓦片落盘（解析失败的区块留透明）
fn write_level0_byproduct(
    app: &tauri::AppHandle,
    world: &WorldCtx,
    dim: MapDim,
    region: &RegionEntry,
    columns: &HashMap<(usize, usize), [u32; 256]>,
) {
    let target = level0_cache_path(app, world, dim, region);
    if target.is_file() {
        return;
    }
    let mut rgba = vec![0_u8; TILE_PX * TILE_PX * 4];
    for (&(cx, cz), cols) in columns {
        for czl in 0..16_usize {
            for cxl in 0..16_usize {
                let color = cols[czl * 16 + cxl];
                if color == 0 {
                    continue;
                }
                let pixel = ((cz * 16 + czl) * TILE_PX + (cx * 16 + cxl)) * 4;
                rgba[pixel] = (color >> 16) as u8;
                rgba[pixel + 1] = (color >> 8) as u8;
                rgba[pixel + 2] = color as u8;
                rgba[pixel + 3] = 255;
            }
        }
    }
    let _ = write_tile_png(&target, &rgba);
}

/// 定位并解析 region 里所需区块的地表颜色列；文件读不到按空处理（游戏运行中的竞态），解析失败按缺块处理
fn parse_region_chunks(
    world: &Path,
    dim: MapDim,
    region: &RegionEntry,
    needed: &[(usize, usize)],
) -> Result<HashMap<(usize, usize), [u32; 256]>, String> {
    let path = region_dir(world, dim).join(format!("r.{}.{}.mca", region.x, region.z));
    let Ok(result) = std::fs::read(&path) else {
        // 游戏运行中的读竞态按空处理；签名含 mtime，等游戏写完自然失效重渲
        return Ok(HashMap::new());
    };
    if result.len() as u64 > REGION_FILE_LIMIT {
        return Err("region 文件异常过大，已拒绝读取".into());
    }
    if result.len() < 8192 {
        // 空壳/截断的 region 文件（游戏崩溃或刚创建没写区块）
        return Ok(HashMap::new());
    }
    let raw = result; // 整文件一次读入：随机 seek 在机械盘上是灾难，顺序读后全部在内存里切
    let jobs: Vec<(usize, usize, usize, usize)> = needed
        .iter()
        .map(|&(cx, cz)| {
            let index = (cz & 31) * 32 + (cx & 31);
            let head = &raw[index * 4..index * 4 + 4];
            let offset = ((head[0] as usize) << 16) | ((head[1] as usize) << 8) | head[2] as usize;
            (cx, cz, offset, head[3] as usize)
        })
        .filter(|&(_, _, offset, sectors)| offset > 0 && sectors > 0)
        .collect();

    let mut result: HashMap<(usize, usize), [u32; 256]> = HashMap::new();
    // 区块多时并行解析（每线程借一个解析许可），少时串行省开销
    if jobs.len() > 64 {
        let cursor = AtomicUsize::new(0);
        let results: Mutex<HashMap<(usize, usize), [u32; 256]>> = Mutex::new(HashMap::new());
        std::thread::scope(|scope| {
            for _ in 0..jobs.len().div_ceil(64).clamp(1, 4) {
                scope.spawn(|| loop {
                    let next = cursor.fetch_add(1, Ordering::Relaxed);
                    if next >= jobs.len() {
                        break;
                    }
                    let (cx, cz, offset, sectors) = jobs[next];
                    with_parse_permit(|| {
                        if let Ok((comp, body)) = chunk_body_at(&raw, offset, sectors) {
                            if let Ok(columns) = parse_chunk_body(comp, body, dim) {
                                results.lock().unwrap().insert((cx, cz), columns);
                            }
                        }
                    });
                });
            }
        });
        return Ok(results.into_inner().unwrap());
    }
    for (cx, cz, offset, sectors) in jobs {
        if let Ok((comp, body)) = chunk_body_at(&raw, offset, sectors) {
            if let Ok(columns) = parse_chunk_body(comp, body, dim) {
                result.insert((cx, cz), columns);
            }
        }
    }
    Ok(result)
}

/// 从已读入内存的 region 字节流切出一个区块的压缩字节（compression byte, body）
fn chunk_body_at<'a>(raw: &'a [u8], offset: usize, sectors: usize) -> Result<(u8, &'a [u8]), String> {
    let start = offset * 4096;
    if start < 8192 || sectors == 0 {
        return Err("区块位置无效".into());
    }
    let Some(head) = raw.get(start..start + 5) else {
        return Err("区块数据越界".into());
    };
    let length = u32::from_be_bytes([head[0], head[1], head[2], head[3]]) as usize;
    let usable = length.saturating_sub(1).min(sectors * 4096 - 4).min(32 * 1024 * 1024);
    let Some(body) = raw.get(start + 5..start + 5 + usable) else {
        return Err("区块数据越界".into());
    };
    Ok((head[4], body))
}

// ---------------- 精简 NBT 扫描器 ----------------
//
// 地图渲染只需要区块里的 DataVersion、sections（方块调色板/位数据）、biomes 和
// WORLD_SURFACE 高度图；quartz_nbt 全量解析会把 BlockEntities 等大负载也分配出来，
// 单区块毫秒级开销。这里手工走 NBT 结构：无关负载纯指针跳过、零分配，配合全局
// 许可池的多核并行，是放大缩小时首渲流畅的关键。

struct ScannedSection {
    y: i32,
    palette: Vec<String>,
    data: Option<Vec<i64>>,
    biome_palette: Vec<String>,
    biome_data: Option<Vec<i64>>,
}

struct ScannedChunk {
    data_version: i32,
    sections: Vec<ScannedSection>,
    heights: Option<Vec<i64>>,
}

struct NbtScan<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> NbtScan<'a> {
    /// NBT 嵌套深度上限（防恶意数据栈溢出）
    const MAX_DEPTH: usize = 32;

    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        if end > self.buf.len() {
            return None;
        }
        let out = &self.buf[self.pos..end];
        self.pos = end;
        Some(out)
    }
    fn u8_(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn u16_(&mut self) -> Option<u16> {
        Some(u16::from_be_bytes(self.take(2)?.try_into().ok()?))
    }
    fn i32_(&mut self) -> Option<i32> {
        Some(i32::from_be_bytes(self.take(4)?.try_into().ok()?))
    }
    fn name(&mut self) -> Option<&'a [u8]> {
        let len = self.u16_()? as usize;
        self.take(len)
    }
    fn skip_string(&mut self) -> Option<()> {
        let len = self.u16_()? as usize;
        self.take(len)?;
        Some(())
    }

    /// 跳过一个负载（按标签类型）；列表/复合结构递归下钻
    fn skip(&mut self, tag: u8, depth: usize) -> Option<()> {
        if depth > Self::MAX_DEPTH {
            return None;
        }
        match tag {
            1 => {
                self.take(1)?;
            }
            2 | 5 => {
                self.take(2)?;
            }
            3 | 6 => {
                self.take(4)?;
            }
            4 => {
                self.take(8)?;
            }
            7 => {
                let n = self.i32_()? as usize;
                self.take(n)?;
            }
            11 => {
                let n = self.i32_()? as usize;
                self.take(n.checked_mul(4)?)?;
            }
            12 => {
                let n = self.i32_()? as usize;
                self.take(n.checked_mul(8)?)?;
            }
            8 => {
                self.skip_string()?;
            }
            9 => {
                let elem = self.u8_()?;
                let n = self.i32_()? as usize;
                match elem {
                    0 if n == 0 => {}
                    1 => {
                        self.take(n)?;
                    }
                    2 | 5 => {
                        self.take(n.checked_mul(2)?)?;
                    }
                    3 | 6 => {
                        self.take(n.checked_mul(4)?)?;
                    }
                    4 => {
                        self.take(n.checked_mul(8)?)?;
                    }
                    8..=12 => {
                        for _ in 0..n {
                            self.skip(elem, depth + 1)?;
                        }
                    }
                    _ => return None,
                }
            }
            10 => loop {
                let inner = self.u8_()?;
                if inner == 0 {
                    return Some(());
                }
                self.name()?;
                self.skip(inner, depth + 1)?;
            },
            _ => return None,
        }
        Some(())
    }

    fn long_array(&mut self) -> Option<Vec<i64>> {
        let n = self.i32_()? as usize;
        let bytes = self.take(n.checked_mul(8)?)?;
        Some(
            bytes
                .chunks_exact(8)
                .map(|c| i64::from_be_bytes(c.try_into().unwrap()))
                .collect(),
        )
    }

    fn string(&mut self) -> Option<String> {
        let len = self.u16_()? as usize;
        Some(String::from_utf8_lossy(self.take(len)?).into_owned())
    }

    /// 走一个复合结构，逐键回调；回调返回 true = 已消费负载，false = 交给跳过器
    fn walk_compound<F>(&mut self, depth: usize, mut on_key: F) -> Option<()>
    where
        F: FnMut(&mut Self, u8, &[u8]) -> Option<bool>,
    {
        if depth > Self::MAX_DEPTH {
            return None;
        }
        loop {
            let tag = self.u8_()?;
            if tag == 0 {
                return Some(());
            }
            let name = self.name()?;
            match on_key(self, tag, name)? {
                true => {}
                false => self.skip(tag, depth + 1)?,
            }
        }
    }

    /// 方块调色板：List<Compound{Name, Properties}>，只取 Name
    fn block_palette(&mut self) -> Option<Vec<String>> {
        let elem = self.u8_()?;
        let count = self.i32_()? as usize;
        if elem == 0 {
            return Some(Vec::new());
        }
        if elem != 10 || count > 4096 {
            return None;
        }
        let mut out = Vec::with_capacity(count);
        for _ in 0..count {
            let mut name = String::new();
            self.walk_compound(1, |scan, tag, key| {
                match (tag, key) {
                    (8, b"Name") => {
                        name = scan.string()?;
                        Some(true)
                    }
                    _ => Some(false),
                }
            })?;
            out.push(name);
        }
        Some(out)
    }

    fn string_list(&mut self) -> Option<Vec<String>> {
        let elem = self.u8_()?;
        let count = self.i32_()? as usize;
        if elem == 0 {
            return Some(Vec::new());
        }
        if elem != 8 {
            return None;
        }
        let mut out = Vec::with_capacity(count.min(4096));
        for _ in 0..count {
            out.push(self.string()?);
        }
        Some(out)
    }

    /// block_states 内嵌复合：palette(List<Compound>) + data(LongArray)
    fn block_states_compound(&mut self) -> Option<(Vec<String>, Option<Vec<i64>>)> {
        let mut palette = Vec::new();
        let mut data = None;
        self.walk_compound(1, |scan, tag, name| {
            match (tag, name) {
                (9, b"palette") => {
                    palette = scan.block_palette()?;
                    Some(true)
                }
                (12, b"data") => {
                    data = Some(scan.long_array()?);
                    Some(true)
                }
                _ => Some(false),
            }
        })?;
        Some((palette, data))
    }

    /// biomes 内嵌复合：palette(List<String>) + data(LongArray)
    fn biomes_compound(&mut self) -> Option<(Vec<String>, Option<Vec<i64>>)> {
        let mut palette = Vec::new();
        let mut data = None;
        self.walk_compound(1, |scan, tag, name| {
            match (tag, name) {
                (9, b"palette") => {
                    palette = scan.string_list()?;
                    Some(true)
                }
                (12, b"data") => {
                    data = Some(scan.long_array()?);
                    Some(true)
                }
                _ => Some(false),
            }
        })?;
        Some((palette, data))
    }

    fn parse_sections_list(&mut self) -> Option<Vec<ScannedSection>> {
        let elem = self.u8_()?;
        let count = self.i32_()? as usize;
        if elem == 0 {
            return Some(Vec::new());
        }
        if elem != 10 || count > 4096 {
            return None;
        }
        let mut sections = Vec::with_capacity(count);
        for _ in 0..count {
            let mut y = 0_i32;
            let mut palette = Vec::new();
            let mut data = None;
            let mut biome_palette = Vec::new();
            let mut biome_data = None;
            self.walk_compound(1, |scan, tag, name| {
                match (tag, name) {
                    // Y 是带符号字节（-4..19），不是 Int
                    (1, b"Y") => {
                        y = scan.u8_()? as i8 as i32;
                        Some(true)
                    }
                    // 方块调色板是 Compound 列表（含 Name），生物群系调色板才是字符串列表
                    (9, b"palette") | (9, b"Palette") => {
                        palette = scan.block_palette()?;
                        Some(true)
                    }
                    (12, b"data") | (12, b"BlockStates") => {
                        data = Some(scan.long_array()?);
                        Some(true)
                    }
                    (10, b"block_states") => {
                        let (inner_palette, inner_data) = scan.block_states_compound()?;
                        palette = inner_palette;
                        data = inner_data;
                        Some(true)
                    }
                    (10, b"biomes") => {
                        let (inner_palette, inner_data) = scan.biomes_compound()?;
                        biome_palette = inner_palette;
                        biome_data = inner_data;
                        Some(true)
                    }
                    _ => Some(false),
                }
            })?;
            sections.push(ScannedSection { y, palette, data, biome_palette, biome_data });
        }
        Some(sections)
    }

    fn heightmaps_compound(&mut self, heights: &mut Option<Vec<i64>>) -> Option<()> {
        self.walk_compound(1, |scan, tag, name| {
            match (tag, name) {
                (12, b"WORLD_SURFACE") => {
                    *heights = Some(scan.long_array()?);
                    Some(true)
                }
                _ => Some(false),
            }
        })
    }

    fn chunk_root(&mut self) -> Option<ScannedChunk> {
        if self.u8_()? != 10 {
            return None;
        }
        self.name()?;
        let mut out = ScannedChunk { data_version: 0, sections: Vec::new(), heights: None };
        self.walk_compound(0, |scan, tag, name| {
            match (tag, name) {
                (3, b"DataVersion") => {
                    out.data_version = scan.i32_()?;
                    Some(true)
                }
                (9, b"sections") | (9, b"Sections") => {
                    out.sections = scan.parse_sections_list()?;
                    Some(true)
                }
                (10, b"Heightmaps") => {
                    scan.heightmaps_compound(&mut out.heights)?;
                    Some(true)
                }
                // 1.13-1.17 的 Level 包装层
                (10, b"Level") => {
                    scan.walk_compound(1, |scan, tag, name| {
                        match (tag, name) {
                            (9, b"Sections") => {
                                out.sections = scan.parse_sections_list()?;
                                Some(true)
                            }
                            (10, b"Heightmaps") => {
                                scan.heightmaps_compound(&mut out.heights)?;
                                Some(true)
                            }
                            _ => Some(false),
                        }
                    })?;
                    Some(true)
                }
                _ => Some(false),
            }
        })?;
        Some(out)
    }
}

fn scan_chunk(comp: u8, body: &[u8]) -> Result<ScannedChunk, String> {
    use flate2::read::{GzDecoder, ZlibDecoder};
    let data: Vec<u8> = match comp {
        1 => {
            let mut out = Vec::new();
            GzDecoder::new(body)
                .read_to_end(&mut out)
                .map_err(|e| format!("区块 gzip 解压失败：{e}"))?;
            out
        }
        2 => {
            let mut out = Vec::new();
            ZlibDecoder::new(body)
                .read_to_end(&mut out)
                .map_err(|e| format!("区块 zlib 解压失败：{e}"))?;
            out
        }
        3 => body.to_vec(),
        other => return Err(format!("不支持的区块压缩类型 {other}")),
    };
    let mut scan = NbtScan { buf: &data, pos: 0 };
    scan.chunk_root().ok_or_else(|| "区块 NBT 结构异常".into())
}

fn parse_chunk_body(comp: u8, body: &[u8], dim: MapDim) -> Result<[u32; 256], String> {
    let scanned = scan_chunk(comp, body)?;
    Ok(chunk_surface_columns(&scanned, dim))
}

// ---------------- 全局解析许可（把并行线程数压在核数内） ----------------

fn parse_permits() -> &'static (Mutex<usize>, Condvar) {
    static PERMITS: OnceLock<(Mutex<usize>, Condvar)> = OnceLock::new();
    PERMITS.get_or_init(|| {
        let total = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .clamp(2, 12);
        (Mutex::new(total), Condvar::new())
    })
}

fn with_parse_permit<T>(job: impl FnOnce() -> T) -> T {
    let (lock, cv) = parse_permits();
    let mut available = lock.lock().unwrap();
    while *available == 0 {
        available = cv.wait(available).unwrap();
    }
    *available -= 1;
    drop(available);
    let out = job();
    *lock.lock().unwrap() += 1;
    cv.notify_one();
    out
}

// ---------------- 区块 → 每列地表颜色 ----------------

/// 一列地表的判定结果：颜色 + 是否该被穿透（空气系/下界顶盖基岩）+ 是否水面
#[derive(Clone, Copy)]
struct SurfaceEntry {
    color: u32,
    skippable: bool,
    bedrock: bool,
    water: bool,
}

impl SurfaceEntry {
    fn usable(&self, dim: MapDim, y: i32) -> bool {
        if self.skippable {
            return false;
        }
        if dim == MapDim::Nether && self.bedrock && y >= 100 {
            return false;
        }
        true
    }
}

struct ChunkSection {
    y: i32,
    /// 方块调色板（minecraft: 名称）
    names: Vec<String>,
    bits: u32,
    data: Option<Vec<i64>>,
    spanning: bool,
    decoded: Option<Vec<u16>>,
    /// 生物群系调色板（1.18+ 区块；旧版为空 → 染色走默认值）
    biome_names: Vec<String>,
    biome_decoded: Option<Vec<u16>>,
}

impl ChunkSection {
    fn state_at(&mut self, index: usize) -> usize {
        let Some(data) = self.data.as_ref() else { return 0 };
        if self.decoded.is_none() {
            self.decoded = Some(unpack_packed(data, self.bits, 4096, self.spanning));
        }
        self.decoded.as_ref().unwrap().get(index).copied().unwrap_or(0) as usize
    }

    /// 4×4×4 生物群系四分格下标 → 调色板下标
    fn biome_at(&self, quartet: usize) -> usize {
        match self.biome_decoded.as_ref() {
            Some(decoded) => decoded.get(quartet).copied().unwrap_or(0) as usize,
            None => 0,
        }
    }

    fn resolve_entry(&self, palette_idx: usize, biome_idx: usize, set: &ColorSet) -> SurfaceEntry {
        let Some(name) = self.names.get(palette_idx).map(String::as_str) else {
            return SurfaceEntry { color: 0, skippable: true, bedrock: false, water: false };
        };
        let biome = self.biome_names.get(biome_idx).map(String::as_str);
        SurfaceEntry {
            color: resolve_block_color(set, name, biome),
            skippable: is_air_name(name),
            bedrock: name == "minecraft:bedrock",
            water: name == "minecraft:water" || name == "minecraft:bubble_column",
        }
    }
}

/// 位压缩数组解包：1.16 起条目不跨 long，更早的版本条目连续跨 long
fn unpack_packed(data: &[i64], bits: u32, count: usize, spanning: bool) -> Vec<u16> {
    if bits == 0 {
        return vec![0; count];
    }
    let mut out = Vec::with_capacity(count);
    if spanning {
        let total_bits = data.len() as u64 * 64;
        for i in 0..count {
            let bit = i as u64 * bits as u64;
            if bit + bits as u64 > total_bits {
                out.push(0);
                continue;
            }
            let mut value = 0_u64;
            for b in 0..bits as u64 {
                let pos = bit + b;
                let long = data[(pos / 64) as usize] as u64;
                if (long >> (pos % 64)) & 1 == 1 {
                    value |= 1 << b;
                }
            }
            out.push(value as u16);
        }
    } else {
        let per_long = (64 / bits) as usize;
        let mask = (1_u64 << bits) - 1;
        for i in 0..count {
            let Some(long) = data.get(i / per_long) else {
                out.push(0);
                continue;
            };
            let shift = (i % per_long) * bits as usize;
            out.push((((*long as u64) >> shift) & mask) as u16);
        }
    }
    out
}

fn section_from_scanned(sc: &ScannedSection, spanning: bool) -> ChunkSection {
    let mut bits = match sc.palette.len() {
        0 | 1 => 0,
        n => (u32::BITS - (n as u32 - 1).leading_zeros()).max(4),
    };
    if sc.data.is_none() {
        bits = 0;
    }
    let biome_bits = match sc.biome_palette.len() {
        0 | 1 => 0,
        n => (u32::BITS - (n as u32 - 1).leading_zeros()).max(4),
    };
    let biome_decoded = sc
        .biome_data
        .as_ref()
        .map(|data| unpack_packed(data, biome_bits, 64, false));
    ChunkSection {
        y: sc.y,
        names: sc.palette.clone(),
        bits,
        data: sc.data.clone(),
        spanning,
        decoded: None,
        biome_names: sc.biome_palette.clone(),
        biome_decoded,
    }
}

/// 高度图缺失时的兜底：自顶向下逐 section 扫描首个非空气方块
fn fallback_heights(sections: &mut [ChunkSection]) -> Vec<u16> {
    let mut heights = vec![0_u16; 256];
    let mut order: Vec<usize> = (0..sections.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(sections[i].y));
    for i in order {
        let y_base = sections[i].y * 16;
        if y_base < -16 || y_base > 400 {
            continue;
        }
        if sections[i].names.iter().all(|n| is_air_name(n)) {
            continue;
        }
        for y_local in (0..16).rev() {
            for z in 0..16_usize {
                for x in 0..16_usize {
                    if heights[z * 16 + x] != 0 {
                        continue;
                    }
                    let index = (y_local << 8) | (z << 4) | x;
                    let state = sections[i].state_at(index);
                    let name = sections[i].names.get(state).map(String::as_str).unwrap_or("");
                    if is_air_name(name) {
                        continue;
                    }
                    heights[z * 16 + x] = (y_base + y_local as i32 + 1) as u16;
                }
            }
        }
        if heights.iter().all(|&h| h != 0) {
            break;
        }
    }
    heights
}

/// 顶层入口：一个区块 256 列的地表颜色（0 = 透明）
fn chunk_surface_columns(scanned: &ScannedChunk, dim: MapDim) -> [u32; 256] {
    let data_version = scanned.data_version;
    // 区块版本未知时按现代格式处理（非跨 long 打包）
    let spanning = data_version != 0 && data_version < 2_529;
    let set = color_set_for(data_version);
    let mut sections: Vec<ChunkSection> = scanned
        .sections
        .iter()
        .map(|s| section_from_scanned(s, spanning))
        .collect();
    if sections.is_empty() {
        return [0; 256];
    }
    let mut heights = scanned
        .heights
        .as_ref()
        .map(|data| unpack_packed(data, 9, 256, spanning))
        .unwrap_or_default();
    if heights.len() != 256 {
        heights = fallback_heights(&mut sections);
    }
    // 1.18+ 的高度图值 = 地表最高方块 y+1-世界最低点(-64)，换算回绝对 y 要减回
    let min_y = if data_version >= 2_860 { -64 } else { 0 };

    let section_map: HashMap<i32, usize> =
        sections.iter().enumerate().map(|(i, s)| (s.y, i)).collect();
    // (方块名, 生物群系名) 的 FNV → 解析结果；同一区块内组合极少，重复列直接命中
    let mut resolved: HashMap<u64, SurfaceEntry> = HashMap::new();
    let mut columns = [0_u32; 256];
    for cz in 0..16_usize {
        for cx in 0..16_usize {
            let column = cz * 16 + cx;
            let h = heights[column] as i32;
            if h <= 0 {
                continue;
            }
            // 顶部若是空气系（或下界顶盖基岩），向下穿透找真实地表
            let mut y = h - 1 + min_y;
            let mut entry =
                surface_entry(&mut sections, &section_map, &mut resolved, set, dim, cx, cz, y);
            let mut steps = 0;
            while entry.is_none() && y > -64 && steps < 448 {
                y -= 1;
                steps += 1;
                entry =
                    surface_entry(&mut sections, &section_map, &mut resolved, set, dim, cx, cz, y);
            }
            let Some(entry) = entry else { continue };
            if entry.color == 0 {
                continue;
            }
            // 明暗与水深压暗（同 MinedMap）：陆地按高度，水域按水底深度向下穿透计深
            let depth = if entry.water {
                let mut floor = y;
                let mut fy = y;
                let mut probed = 0;
                while fy > -64 && probed < 448 {
                    fy -= 1;
                    probed += 1;
                    match surface_entry(&mut sections, &section_map, &mut resolved, set, dim, cx, cz, fy) {
                        Some(below) if !below.water => {
                            floor = fy;
                            break;
                        }
                        _ => floor = fy,
                    }
                }
                floor
            } else {
                y
            };
            columns[column] = shade(entry.color, depth);
        }
    }
    columns
}

/// 取一列在 y 处的地表判定；None = 该处是空气系/下界顶盖基岩，需要继续向下
#[allow(clippy::too_many_arguments)]
fn surface_entry(
    sections: &mut [ChunkSection],
    section_map: &HashMap<i32, usize>,
    resolved: &mut HashMap<u64, SurfaceEntry>,
    set: &ColorSet,
    dim: MapDim,
    cx: usize,
    cz: usize,
    y: i32,
) -> Option<SurfaceEntry> {
    let &si = section_map.get(&(y >> 4))?;
    let index = ((y & 15) as usize) << 8 | cz << 4 | cx;
    let palette_idx = sections[si].state_at(index);
    let quartet = (((y & 15) >> 2) as usize) << 4 | ((cz & 15) >> 2) << 2 | ((cx & 15) >> 2);
    let biome_idx = sections[si].biome_at(quartet);
    let Some(name) = sections[si].names.get(palette_idx).map(String::as_str) else {
        return None;
    };
    let biome_key = sections[si].biome_names.get(biome_idx).map(String::as_str).unwrap_or("");
    // 数值 memo 键：水深下探每列要走几十步，字符串分配在这里是显著热点
    let mut memo_key: u64 = 0xcbf29ce484222325;
    for byte in name.as_bytes().iter().chain(b"\0").chain(biome_key.as_bytes()) {
        memo_key = (memo_key ^ *byte as u64).wrapping_mul(0x100000001b3);
    }
    if let Some(entry) = resolved.get(&memo_key) {
        return entry.usable(dim, y).then_some(*entry);
    }
    let entry = sections[si].resolve_entry(palette_idx, biome_idx, set);
    resolved.insert(memo_key, entry);
    entry.usable(dim, y).then_some(entry)
}

/// 明暗与水深压暗（参考 MinedMap block_color 终式）：系数 = 0.5 + 0.005 × 深度；
/// 陆地深度 = 方块高度（越高越亮），水域深度 = 水底高度（越深越暗）
fn shade(color: u32, depth: i32) -> u32 {
    let factor = 0.5 + 0.005 * depth as f32;
    let channel = |c: u32| ((c as f32 * factor).round() as u32).min(255);
    (channel(color >> 16) << 16) | (channel((color >> 8) & 0xFF) << 8) | channel(color & 0xFF)
}

// ---------------- 签名哈希 + 方块配色（vendor 自 Querz/mcaselector 预生成数据） ----------------
//
// 配色数据在 vendor/mcaselector-colors/（来源与格式见该目录 README.md）：按游戏版本从真实
// 方块纹理预生成的平均色，附按生物群系的草/叶/水/干叶染色色值；按区块 DataVersion 选版本段。

fn fnv64a(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in s.as_bytes() {
        hash = (hash ^ *byte as u64).wrapping_mul(0x100000001b3);
    }
    hash
}

struct ColorSet {
    /// 方块名 → 静态色，或需按生物群系染色的基色
    colors: HashMap<String, BlockColorEntry>,
    /// 生物群系名 → [草, 叶, 水, 干叶] 染色色值
    biome_tints: HashMap<String, [u32; TINT_KINDS]>,
}

#[derive(Clone, Copy)]
enum BlockColorEntry {
    Static(u32),
    /// 基色 + 染色类别（0=草 1=叶 2=水 3=干叶）
    Tinted { base: u32, kind: usize },
}

const TINT_KINDS: usize = 4;
/// 平原生物群系的默认染色（mcaselector ColorMapping 的 DEFAULT_*_TINT）
const DEFAULT_TINTS: [u32; TINT_KINDS] = [0x91bd59, 0x77ab2f, 0x3f76e4, 0x77ab2f];
/// 未知方块的兜底灰
const UNKNOWN_BLOCK_COLOR: u32 = 0x8F8F8F;

fn parse_hex_rgb(hex: &str) -> Option<u32> {
    u32::from_str_radix(hex.trim(), 16).ok().map(|value| value & 0xFFFFFF)
}

fn parse_color_set(raw: &str) -> ColorSet {
    let root: serde_json::Value = match serde_json::from_str(raw) {
        Ok(root) => root,
        Err(_) => return ColorSet { colors: HashMap::new(), biome_tints: HashMap::new() },
    };
    let mut colors = HashMap::new();
    if let Some(map) = root.get("colors").and_then(Value::as_object) {
        for (name, value) in map {
            let entry = match value {
                Value::String(hex) => {
                    BlockColorEntry::Static(parse_hex_rgb(hex).unwrap_or(UNKNOWN_BLOCK_COLOR))
                }
                Value::Object(obj) => {
                    let color = obj
                        .get("color")
                        .and_then(Value::as_str)
                        .and_then(parse_hex_rgb)
                        .unwrap_or(UNKNOWN_BLOCK_COLOR);
                    let properties = obj.get("properties").and_then(Value::as_u64).unwrap_or(0);
                    // 位含义见 vendor README（2=草 4=叶 8=水 128=干叶）
                    let kind = if properties & 2 != 0 {
                        Some(0)
                    } else if properties & 4 != 0 {
                        Some(1)
                    } else if properties & 8 != 0 {
                        Some(2)
                    } else if properties & 128 != 0 {
                        Some(3)
                    } else {
                        None
                    };
                    match kind {
                        Some(kind) => BlockColorEntry::Tinted { base: color, kind },
                        None => BlockColorEntry::Static(color),
                    }
                }
                _ => continue,
            };
            colors.insert(name.clone(), entry);
        }
    }
    let mut biome_tints = HashMap::new();
    if let Some(map) = root.get("tints").and_then(Value::as_object) {
        for (biome, buckets) in map {
            let mut tints = [0_u32; TINT_KINDS];
            match buckets {
                // 新格式：["91bd59", ...] 依次为 草/叶/水/干叶
                Value::Array(items) => {
                    for (kind, item) in items.iter().enumerate() {
                        if kind >= TINT_KINDS {
                            break;
                        }
                        if let Some(color) = item.as_str().and_then(parse_hex_rgb) {
                            tints[kind] = color;
                        }
                    }
                }
                // 旧格式：类别下标 → 逐字符展开的色值 map
                Value::Object(obj) => {
                    for (bucket, chars) in obj {
                        let Ok(kind) = bucket.parse::<usize>() else { continue };
                        if kind >= TINT_KINDS {
                            continue;
                        }
                        let mut digits: Vec<(usize, char)> = Vec::new();
                        if let Some(parts) = chars.as_object() {
                            for (position, ch) in parts {
                                if let (Ok(position), Some(ch)) = (
                                    position.parse::<usize>(),
                                    ch.as_str().and_then(|s| s.chars().next()),
                                ) {
                                    digits.push((position, ch));
                                }
                            }
                        }
                        digits.sort_by_key(|(position, _)| *position);
                        let hex: String = digits.iter().map(|(_, ch)| ch).collect();
                        if let Some(color) = parse_hex_rgb(&hex) {
                            tints[kind] = color;
                        }
                    }
                }
                _ => {}
            }
            biome_tints.insert(biome.clone(), tints);
        }
    }
    ColorSet { colors, biome_tints }
}

/// 按区块 DataVersion 选择版本段（边界见 vendor README）
fn color_set_source(data_version: i32) -> (&'static str, &'static str) {
    match data_version {
        3_940.. => ("24w18a", include_str!("../vendor/mcaselector-colors/colors-24w18a.json")),
        3_465.. => ("23w12a", include_str!("../vendor/mcaselector-colors/colors-23w12a.json")),
        3_120.. => ("22w11a", include_str!("../vendor/mcaselector-colors/colors-22w11a.json")),
        2_860.. => ("21w37a", include_str!("../vendor/mcaselector-colors/colors-21w37a.json")),
        2_724.. => ("20w45a", include_str!("../vendor/mcaselector-colors/colors-20w45a.json")),
        2_566.. => ("20w06a", include_str!("../vendor/mcaselector-colors/colors-20w06a.json")),
        2_225.. => ("19w34a", include_str!("../vendor/mcaselector-colors/colors-19w34a.json")),
        _ => ("18w43a", include_str!("../vendor/mcaselector-colors/colors-18w43a.json")),
    }
}

/// 惰性解析并进程内缓存（每版本段首次 ~10ms，之后零开销）
fn color_set_for(data_version: i32) -> &'static ColorSet {
    static CACHE: OnceLock<Mutex<HashMap<&'static str, &'static ColorSet>>> = OnceLock::new();
    let (label, raw) = color_set_source(data_version);
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache.lock().unwrap();
    guard
        .entry(label)
        .or_insert_with(|| Box::leak(Box::new(parse_color_set(raw))))
}

fn resolve_block_color(set: &ColorSet, name: &str, biome: Option<&str>) -> u32 {
    let Some(entry) = set.colors.get(name) else {
        return UNKNOWN_BLOCK_COLOR;
    };
    match entry {
        BlockColorEntry::Static(color) => *color,
        BlockColorEntry::Tinted { base, kind } => {
            let tint = biome
                .and_then(|biome| set.biome_tints.get(biome))
                .map(|tints| tints[*kind])
                .unwrap_or_else(|| DEFAULT_TINTS[*kind]);
            multiply_tint(*base, tint)
        }
    }
}

/// 染色 = 每通道 (基色 × 染色) / 255（同 mcaselector applyTint）
fn multiply_tint(color: u32, tint: u32) -> u32 {
    let channel = |c: u32, t: u32| (((c * t) + 127) / 255).min(255);
    (channel(color >> 16, tint >> 16) << 16)
        | (channel((color >> 8) & 0xFF, (tint >> 8) & 0xFF) << 8)
        | channel(color & 0xFF, tint & 0xFF)
}

fn is_air_name(name: &str) -> bool {
    matches!(
        name,
        "minecraft:air"
            | "minecraft:cave_air"
            | "minecraft:void_air"
            | "minecraft:light"
            | "minecraft:barrier"
            | "minecraft:structure_void"
    )
}
