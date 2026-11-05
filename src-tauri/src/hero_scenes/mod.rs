//! 首页启动卡像素动画场景：CPU 光栅化渲染器。
//!
//! 场景代码 vendor 自 EdwinZhanCN/LumilioCL v0.2.3（AGPL-3.0），另含 WUHUICL
//! 原创场景 Takeoff。渲染是无状态纯函数：前端（src/heroScenes.ts +
//! HeroSceneCanvas.vue）持有时钟，按 40fps 请求 `hero:frame`，本模块把场景画进
//! 小 texel 网格后按行内同色压缩成矩形（base64 u32 流）返回。合成管线（场景 →
//! 离场过渡 → 加载 reveal → 压暗 → scrim）改自上游 hero_carousel.rs 的
//! rasterize()，去掉了 GPUI 页面边缘溶解（PageDissolve）一节。

pub mod noise;
pub mod raster;
pub mod scenes;
pub mod transition;

use base64::Engine as _;
use std::sync::{Arc, Mutex, OnceLock};

use serde_json::{json, Value};

use raster::PixelGrid;
use scenes::{Landmark, Scene};

/// 底部/左侧暗带强度（上游 0.62；本项目首页卡另有 .hero-shade 渐变遮罩，略降防过暗）。
const SCRIM_STRENGTH: f32 = 0.55;
/// dim 帧的整体亮度（上游 director::DIM）。
const DIM: f32 = 0.55;
/// 网格尺寸护栏：前端按 7 逻辑像素/texel、列数 32..=190 计算布局，服务端只兜底防越界。
const MAX_COLUMNS: usize = 320;
const MAX_ROWS: usize = 200;

/// 一帧的渲染输入（FrameSpec 改自上游 hero_carousel.rs，时钟由前端持有）。
#[derive(Clone, Copy, Debug)]
pub struct FrameSpec {
    pub scene: Scene,
    pub age: f32,
    /// 离场场景、其时钟与转场进度（0..1，"区块加载"式过渡）。
    pub leaving: Option<(Scene, f32, f32)>,
    /// 世界已加载份额（<1 时其余为未加载 void），用于启动进度联动。
    pub reveal: f32,
    /// 前景有内容常驻时整体压暗。
    pub dim: bool,
    /// 指针悬停开始游戏（0..1），炉火/起飞回应。
    pub flare: f32,
}

/// 离场帧单条缓存：前端把离场时钟冻结在交接时刻（ageAtHandoff），整个转场期间
/// 离场画面逐帧相同——渲染一次，之后直接复用，转场开销近乎减半。
fn cached_outgoing(scene: Scene, age: f32, columns: usize, rows: usize) -> Arc<PixelGrid> {
    static CACHE: OnceLock<Mutex<Option<((String, u32, usize, usize), Arc<PixelGrid>)>>> =
        OnceLock::new();
    let key = (format!("{scene:?}"), age.to_bits(), columns, rows);
    let cache = CACHE.get_or_init(|| Mutex::new(None));
    let mut guard = cache.lock().unwrap();
    if let Some((cached_key, grid)) = guard.as_ref() {
        if *cached_key == key {
            return grid.clone();
        }
    }
    let mut grid = PixelGrid::new(columns, rows);
    scene.render(&mut grid, age, 0.);
    let arc = Arc::new(grid);
    *guard = Some((key, arc.clone()));
    arc
}

/// 渲染管线：场景 → 离场过渡 → 加载 reveal → 压暗 → scrim。
pub fn rasterize(frame: FrameSpec, columns: usize, rows: usize) -> PixelGrid {
    let mut grid = PixelGrid::new(columns, rows);
    frame.scene.render(&mut grid, frame.age, frame.flare);
    if let Some((scene, age, progress)) = frame.leaving {
        let outgoing = cached_outgoing(scene, age, columns, rows);
        transition::composite(&outgoing, &mut grid, progress.clamp(0., 1.));
    }
    if frame.reveal < 1. {
        let void = transition::void(columns, rows);
        transition::composite(&void, &mut grid, frame.reveal.clamp(0., 1.));
    }
    if frame.dim {
        grid.dim(DIM);
    }
    grid.apply_scrim(SCRIM_STRENGTH);
    grid
}

/// 场景 id（前端注册表）→ Scene。"hearth:*" 的地标由当前实例哈希决定，前端算好传入。
pub fn parse_scene(id: &str) -> Result<Scene, String> {
    let scene = match id {
        "dawn" => Scene::Dawn,
        "caves" => Scene::Caves,
        "redstone" => Scene::Redstone,
        "portal" => Scene::Portal,
        "takeoff" => Scene::Takeoff,
        "hearth:village" => Scene::Hearth(Landmark::Village),
        "hearth:portal" => Scene::Hearth(Landmark::Portal),
        "hearth:mine" => Scene::Hearth(Landmark::Mine),
        "hearth:lamp" => Scene::Hearth(Landmark::Lamp),
        other => return Err(format!("未知场景：{other}")),
    };
    Ok(scene)
}

fn frame_from(args: &[Value]) -> Result<FrameSpec, String> {
    let spec = args.first().cloned().unwrap_or(Value::Null);
    let obj = spec.as_object().ok_or("hero:frame 参数必须是对象")?;
    let scene = parse_scene(obj.get("scene").and_then(|v| v.as_str()).unwrap_or(""))?;
    let f = |key: &str| obj.get(key).and_then(|v| v.as_f64()).map(|v| v as f32);
    let leaving = match obj.get("leaving") {
        Some(Value::Object(leaving)) => {
            let scene = parse_scene(leaving.get("scene").and_then(|v| v.as_str()).unwrap_or(""))?;
            Some((
                scene,
                leaving.get("age").and_then(|v| v.as_f64()).unwrap_or(0.) as f32,
                leaving.get("progress").and_then(|v| v.as_f64()).unwrap_or(0.) as f32,
            ))
        }
        _ => None,
    };
    Ok(FrameSpec {
        scene,
        age: f("age").unwrap_or(0.),
        leaving,
        reveal: f("reveal").unwrap_or(1.),
        dim: obj.get("dim").and_then(|v| v.as_bool()).unwrap_or(false),
        flare: f("flare").unwrap_or(0.),
    })
}

/// hero:frame {scene, age, columns, rows, leaving?, reveal?, dim?, flare?}
/// → {bin: base64(u32 LE 0xRRGGBB 逐 texel 行主序), hud}
///
/// 帧以原始像素传输：暗角/星空的有序抖动会把矩形压碎成上万个 texel 碎片，
/// 矩形协议的 JSON 体积与前端逐矩形绘制都撑不住 40fps；像素协议体积恒定
/// （网格 ~40KB → base64 ~57KB），前端写一次 ImageData 再整体缩放即可。
fn frame(args: &[Value]) -> Result<Value, String> {
    let spec = args.first().cloned().unwrap_or(Value::Null);
    let obj = spec.as_object().ok_or("hero:frame 参数必须是对象")?;
    let columns = obj.get("columns").and_then(|v| v.as_u64()).unwrap_or(120) as usize;
    let rows = obj.get("rows").and_then(|v| v.as_u64()).unwrap_or(52) as usize;
    let columns = columns.clamp(8, MAX_COLUMNS);
    let rows = rows.clamp(8, MAX_ROWS);
    let frame = frame_from(args)?;
    let grid = rasterize(frame, columns, rows);
    let mut payload = Vec::with_capacity(grid.packed().len() * 4);
    for color in grid.packed() {
        payload.extend_from_slice(&color.to_le_bytes());
    }
    let bin = base64::engine::general_purpose::STANDARD.encode(payload);
    let hud = frame.scene.hud(frame.age, (columns as i32, rows as i32));
    Ok(json!({ "bin": bin, "hud": hud }))
}

/// 统一入口（ipc.rs 子系统认领链）
pub fn invoke(_app: &tauri::AppHandle, channel: &str, args: &[Value]) -> Option<Result<Value, String>> {
    match channel {
        "hero:frame" => Some(frame(args)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame_json(scene: &str, age: f32, columns: usize, rows: usize) -> Value {
        frame(&[json!({ "scene": scene, "age": age, "columns": columns, "rows": rows })])
            .expect("frame ok")
    }

    /// 解码 bin 成 texel 颜色数组（与前端 decodePixels 同构）。
    fn decode_pixels(value: &Value, columns: usize, rows: usize) -> Vec<u32> {
        let bin = value["bin"].as_str().expect("bin string");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(bin)
            .expect("bin is base64");
        assert_eq!(bytes.len(), columns * rows * 4, "one u32 per texel");
        bytes
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
            .collect()
    }

    fn assert_pixels_in_bounds(value: &Value, columns: usize, rows: usize) {
        let pixels = decode_pixels(value, columns, rows);
        assert_eq!(pixels.len(), columns * rows, "pixel count matches grid");
        assert!(
            pixels.iter().any(|&p| p != 0),
            "frame must not be fully black"
        );
    }

    #[test]
    fn deterministic_output() {
        for scene in ["dawn", "caves", "redstone", "portal", "hearth:mine", "takeoff"] {
            let a = frame_json(scene, 4.2, 96, 40);
            let b = frame_json(scene, 4.2, 96, 40);
            assert_eq!(a, b, "{scene} must render identically for identical input");
            assert!(a["hud"].as_str().is_some(), "{scene} needs a HUD readout");
        }
    }

    #[test]
    fn all_scenes_survive_sampled_ages() {
        for scene in ["dawn", "caves", "redstone", "portal", "hearth:village", "hearth:lamp", "takeoff"] {
            for age in [0., 1.3, 5.5, 9., 13.7, 21., 26., 40.] {
                let value = frame_json(scene, age, 120, 52);
                assert_pixels_in_bounds(&value, 120, 52);
            }
        }
    }

    #[test]
    fn transition_blends_two_scenes() {
        let to = frame_json("dawn", 3., 96, 40);
        let blended = frame(&[json!({
            "scene": "caves", "age": 3., "columns": 96, "rows": 40,
            "leaving": { "scene": "dawn", "age": 9., "progress": 0.5 }
        })])
        .unwrap();
        let end = frame_json("caves", 3., 96, 40);
        assert_ne!(decode_pixels(&blended, 96, 40), decode_pixels(&end, 96, 40), "mid-transition differs from the target");
        assert_ne!(decode_pixels(&blended, 96, 40), decode_pixels(&to, 96, 40), "mid-transition differs from the outgoing");
        assert_pixels_in_bounds(&blended, 96, 40);
    }

    #[test]
    fn reveal_shows_partial_void() {
        let partial = frame(&[json!({
            "scene": "dawn", "age": 5., "columns": 96, "rows": 40, "reveal": 0.5
        })])
        .unwrap();
        let full = frame_json("dawn", 5., 96, 40);
        assert_ne!(decode_pixels(&partial, 96, 40), decode_pixels(&full, 96, 40));
        assert_pixels_in_bounds(&partial, 96, 40);
    }

    #[test]
    fn scene_ids_parse_or_reject() {
        assert_eq!(parse_scene("hearth:mine").unwrap(), Scene::Hearth(Landmark::Mine));
        assert_eq!(parse_scene("takeoff").unwrap(), Scene::Takeoff);
        assert!(parse_scene("nether").is_err());
    }
}
