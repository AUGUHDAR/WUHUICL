//! 起飞 · Takeoff — WUHUICL 原创场景（非 vendor 代码，风格对齐 LumilioCL 场景群）：
//! 长江边的夜航发射场。对岸城市灯火与江面月光，发射塔红灯呼吸，白火箭待命排
//! 汽；倒计时归零后点火、升空、马赫环尾焰照亮全场，最后按抖动溶解回收至新火
//! 箭。`flare`（0–1，指针悬停开始游戏）点亮探照灯，把火箭和塔架照得通亮。

use crate::hero_scenes::noise::{bayer4, hash1, hash2, smoothstep, value1};
use crate::hero_scenes::raster::{PixelGrid, Rgb};

/// 一个完整发射周期：待机排汽 → 点火 → 升空离场 → 溶解回收。
pub const CYCLE: f32 = 26.;
/// 主引擎点火。
pub const IGNITE_AT: f32 = 8.;
/// 离地瞬间。
pub const LIFTOFF_AT: f32 = 9.;
/// 火箭离场后开始向新火箭溶解回收。
pub const RECOVER_AT: f32 = 20.;
/// reduceMotion 静止帧取的时刻：刚离地，火焰与闪光俱在。
pub const STILL_AGE: f32 = 10.2;
/// 升空加速度（texel/s²）：离地 8 秒后上升约 96 texel，刚好出画。
const ASCENT: f32 = 3.;
const NOSE: Rgb = Rgb::hex(0xff5a3a);
const BODY: Rgb = Rgb::hex(0xe8ecf2);
const BODY_SHADE: Rgb = Rgb::hex(0xb8c2ce);
const FLAME_CORE: u32 = 0xfff4b0;
const FLAME_MID: u32 = 0xffb43a;
const FLAME_OUT: u32 = 0xff6a1a;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Layout {
    pub horizon: i32,
    pub ground: i32,
    pub pad_x: i32,
    pub tower_x: i32,
    pub moon_x: i32,
    pub moon_y: i32,
}

impl Layout {
    pub fn new(width: i32, height: i32) -> Self {
        Self {
            horizon: (height as f32 * 0.6) as i32,
            ground: (height as f32 * 0.74) as i32,
            pad_x: (width as f32 * 0.62) as i32,
            tower_x: (width as f32 * 0.62) as i32 - 9,
            moon_x: (width as f32 * 0.18) as i32,
            moon_y: (height as f32 * 0.16) as i32,
        }
    }
}

/// 火箭离地高度（texel）；点火前为 0。
pub fn rise(t: f32) -> f32 {
    if t <= LIFTOFF_AT {
        0.
    } else {
        0.5 * ASCENT * (t - LIFTOFF_AT) * (t - LIFTOFF_AT)
    }
}

/// 引擎推力从点火持续到火箭出画。
fn thrust(t: f32) -> bool {
    t >= IGNITE_AT && rise(t) < 140.
}

pub fn hud(age: f32) -> String {
    let t = age.rem_euclid(CYCLE);
    if t < IGNITE_AT {
        format!("T-{:04.1}s", IGNITE_AT - t)
    } else if t < LIFTOFF_AT {
        "点火".to_owned()
    } else {
        format!("T+{:04.1}s · {}m", t - LIFTOFF_AT, (rise(t) * 4.) as i32)
    }
}

pub fn render(grid: &mut PixelGrid, age: f32, flare: f32) {
    let t = age.rem_euclid(CYCLE);
    if t < RECOVER_AT {
        draw(grid, t, flare);
        return;
    }
    // 回收段：发射后的余烟与待命的新火箭按有序抖动交叉溶解。
    let progress = smoothstep(RECOVER_AT, CYCLE, t);
    let (w, h) = (grid.width(), grid.height());
    let mut fresh = PixelGrid::new(w, h);
    draw(&mut fresh, 0., flare);
    draw(grid, t, flare);
    for y in 0..h {
        for x in 0..w {
            let old = grid.get(x, y);
            let fresh = fresh.get(x, y);
            let texel = if bayer4(x as i32, y as i32) < progress {
                fresh
            } else {
                old.lerp(fresh, progress * 0.4)
            };
            grid.set(x, y, texel);
        }
    }
}

fn draw(grid: &mut PixelGrid, t: f32, flare: f32) {
    let (w, h) = (grid.width() as i32, grid.height() as i32);
    let layout = Layout::new(w, h);
    let up = rise(t).round() as i32;

    // 夜空：色带 + 星星 + 方月亮光环。
    grid.shade(|x, y| {
        let (xi, yi) = (x as i32, y as i32);
        let s = (yi as f32 / layout.horizon.max(1) as f32).clamp(0., 1.);
        let seam = (s * 12.).fract() > 0.82 && bayer4(xi, yi) > 0.5;
        let banded = (((s * 12.).floor() + f32::from(u8::from(seam))) / 12.).clamp(0., 1.);
        let mut color = Rgb::hex(0x050914).lerp(Rgb::hex(0x1c2848), banded.powf(1.2));
        if yi < layout.horizon * 3 / 4 && hash2(xi, yi, 201) > 0.986 {
            let twinkle = 0.5 + 0.5 * (t * 2.2 + hash2(xi, yi, 202) * 40.).sin();
            color = color.lerp(Rgb::hex(0xe8ecff), 0.3 + 0.6 * twinkle);
        }
        let ring = (xi - layout.moon_x).abs().max((yi - layout.moon_y).abs());
        let moon_half = (h / 26).max(2);
        if ring > moon_half && ring <= moon_half * 3 {
            let falloff = 1. - (ring - moon_half) as f32 / (moon_half * 2) as f32;
            color = color.add(Rgb::hex(0x8a9ac8).scale(((falloff * 3.).ceil() / 3.) * 0.1));
        }
        color
    });
    // 方月亮（带明暗界线，同炉火月亮的路数但不画坑）。
    let moon_half = (h / 26).max(2);
    for dy in -moon_half..=moon_half {
        for dx in -moon_half..=moon_half {
            let limb = dx == -moon_half || dy == moon_half;
            let color = if limb { 0xc9d2e4 } else { 0xe9eef8 };
            grid.put(layout.moon_x + dx, layout.moon_y + dy, Rgb::hex(color));
        }
    }

    draw_city(grid, &layout, t);
    draw_water(grid, &layout, t);
    draw_ground(grid, &layout);
    draw_tower(grid, &layout, t, flare);
    // 探照灯（flare）：把火箭与塔架打亮，随悬停渐亮。
    if flare > 0.01 {
        for y in (layout.ground - 26).max(0)..(layout.ground + 4).min(h) {
            for x in (layout.pad_x - 12).max(0)..(layout.pad_x + 12).min(w) {
                let d = (x - layout.pad_x).abs().max((y - (layout.ground - 10)).abs());
                grid.glow(x, y, Rgb::hex(0xbcd8ff).scale(0.1 * flare * (1. - d as f32 / 30.).max(0.)));
            }
        }
    }

    // 点火瞬间的全场景闪光，指数衰减。
    let flash = (-((t - IGNITE_AT) / 0.5).powi(2)).exp();
    if flash > 0.02 {
        let tint = Rgb::hex(0xffd9a0).scale(flash * 0.45);
        for y in 0..h {
            for x in 0..w {
                grid.glow(x, y, tint);
            }
        }
    }

    draw_smoke(grid, &layout, t);
    draw_rocket(grid, &layout, t, up);
}

/// 对岸城市天际线：楼体块剪影 + 随机亮窗，越靠左（城区）越高。
fn draw_city(grid: &mut PixelGrid, layout: &Layout, t: f32) {
    let block = 3;
    for bx in 0..grid.width() as i32 / block {
        let x0 = bx * block;
        let center = 1. - (x0 as f32 / grid.width() as f32) * 0.9;
        let rise = 2 + (value1(bx as f32 * 0.5, 211) * 6. * center).round() as i32;
        for y in (layout.horizon - rise).max(0)..layout.horizon {
            for x in x0..x0 + block {
                grid.put(x, y, Rgb::hex(0x101a30));
            }
        }
        // 亮窗：按楼留几个，少数慢闪。
        for window in 0..rise {
            if hash1(bx * 16 + window, 212) > 0.62 {
                let blink = hash1(bx * 16 + window, 213) > 0.85
                    && hash1(bx + window, (t * 0.7) as u32) < 0.4;
                if !blink {
                    let wx = x0 + (hash1(bx * 16 + window, 214) * block as f32) as i32;
                    grid.put(
                        wx,
                        layout.horizon - rise + window,
                        Rgb::hex(if hash1(bx + window, 215) > 0.5 { 0xffd270 } else { 0xffa048 }),
                    );
                }
            }
        }
    }
}

/// 江面：深水色带 + 月光洒在水面的一道碎光。
fn draw_water(grid: &mut PixelGrid, layout: &Layout, t: f32) {
    for y in layout.horizon..layout.ground {
        let depth = (y - layout.horizon) as f32 / (layout.ground - layout.horizon).max(1) as f32;
        let mut color = Rgb::hex(0x0a1226).lerp(Rgb::hex(0x18243e), depth);
        if (y - layout.horizon) % 2 == 0 {
            color = color.scale(1.06);
        }
        for x in 0..grid.width() as i32 {
            let mut texel = color;
            let lobe = (1. - (x - layout.moon_x).abs() as f32 / (grid.width() as f32 * 0.12)).max(0.);
            if lobe > 0. && hash2(x, y, (t * 4.) as u32) > 0.9 - lobe * 0.3 {
                texel = texel.lerp(Rgb::hex(0x9ab0e0), lobe * 0.8);
            }
            grid.set(x as usize, y as usize, texel);
        }
    }
}

/// 近岸地面：草地 + 混凝土发射坪（带分缝），坪中央烧灼痕。
fn draw_ground(grid: &mut PixelGrid, layout: &Layout) {
    let pad_half = 14;
    for y in layout.ground..grid.height() as i32 {
        for x in 0..grid.width() as i32 {
            let depth = y - layout.ground;
            let speckle = hash2(x, y, 221);
            let on_pad = (x - layout.pad_x).abs() <= pad_half;
            let base = if on_pad {
                let seam = (x - layout.pad_x).rem_euclid(4) == 0 || depth % 3 == 0;
                if (x - layout.pad_x).abs() <= 2 && depth < 2 {
                    Rgb::hex(0x2e2a28) // 导流槽烧灼痕
                } else if seam {
                    Rgb::hex(0x444a54)
                } else if speckle > 0.85 {
                    Rgb::hex(0x666e78)
                } else {
                    Rgb::hex(0x565c66)
                }
            } else if depth == 0 || (depth == 1 && hash1(x, 222) > 0.5) {
                Rgb::hex(if speckle > 0.7 { 0x39603a } else { 0x2c4a2e })
            } else if speckle < 0.18 {
                Rgb::hex(0x3c2f22)
            } else {
                Rgb::hex(0x2f3d28)
            };
            grid.set(x as usize, y as usize, base);
        }
    }
}

/// 红白格构发射塔，塔顶航标灯呼吸（倒计时最后 5 秒急闪）。
fn draw_tower(grid: &mut PixelGrid, layout: &Layout, t: f32, _flare: f32) {
    let top = layout.ground - 22;
    for y in top..layout.ground {
        for dx in -2..=2 {
            let x = layout.tower_x + dx;
            let edge = dx == -2 || dx == 2;
            let stripe = y % 4 == 0;
            let brace = !edge && (x + y) % 4 == 0;
            let base = if brace {
                0x5a2019
            } else if edge && stripe {
                0xd8d8d8
            } else if edge {
                0x8a2f28
            } else {
                0x0a0e18
            };
            grid.put(x, y, Rgb::hex(base));
        }
    }
    let urgent = t > IGNITE_AT - 5. && t < LIFTOFF_AT;
    let period = if urgent { 0.15 } else { 0.9 };
    let on = hash1((t / period) as i32, 223) > 0.5;
    if on {
        grid.put(layout.tower_x, top - 1, Rgb::hex(0xff3a30));
        grid.glow(layout.tower_x, top - 1, Rgb::hex(0xff4030).scale(0.3));
    } else {
        grid.put(layout.tower_x, top - 1, Rgb::hex(0x401512));
    }
}

/// 尾焰烟：离地后贴地翻涌向两侧，火箭身后留下随风飘斜的烟柱。
fn draw_smoke(grid: &mut PixelGrid, layout: &Layout, t: f32) {
    if t < LIFTOFF_AT {
        // 待机排汽：箭体侧面一小缕白汽。
        for i in 0..6 {
            let phase = (t * 0.6 + i as f32 / 6.).fract();
            let x = (layout.pad_x + 3) as f32 + phase * 6.;
            let y = (layout.ground - 8) as f32 - phase * 7.;
            grid.blend(
                x as i32,
                y as i32,
                Rgb::hex(0xd8dde8),
                (1. - phase) * 0.35,
            );
        }
        return;
    }
    let since = t - LIFTOFF_AT;
    // 贴地烟云：从坪中心向两侧铺开、随时间变淡。
    let spread = (since * 9.).min(grid.width() as f32 * 0.4);
    for i in 0..22 {
        let phase = (since * 0.8 + hash1(i, 230)).fract();
        let dir = if i % 2 == 0 { 1 } else { -1 };
        let x = layout.pad_x as f32 + dir as f32 * (2. + phase * spread);
        let y = (layout.ground - 1) as f32 - hash1(i, 231) * 3. * phase;
        let alpha = (1. - phase) * 0.5 * smoothstep(0., 0.4, since).min(1.)
            * (1. - smoothstep(12., 20., since));
        let color = Rgb::hex(0xcab898).lerp(Rgb::hex(0x8a92a4), phase);
        let size = 1 + (phase * 2.4) as i32;
        for dy in 0..size {
            for dx in 0..size {
                grid.blend(x as i32 + dx, y as i32 - dy, color, alpha);
            }
        }
    }
    // 升空烟柱：沿火箭轨迹一串烟团，被风吹向右。
    for i in 0..26 {
        let life = since - i as f32 * 0.28;
        if life <= 0. {
            continue;
        }
        let y = (layout.ground - 4) as f32 - rise(t - i as f32 * 0.28);
        let drift = life * 1.6;
        let x = layout.pad_x as f32 + drift * 0.4;
        let alpha = (1. - life / 8.).clamp(0., 1.) * 0.4;
        let color = Rgb::hex(0xd0c0a8).lerp(Rgb::hex(0x767e92), (life / 8.).min(1.));
        let size = 1 + (life * 0.8).min(3.) as i32;
        for dy in 0..size {
            for dx in 0..size {
                grid.blend(x as i32 + dx, y as i32 - dy, color, alpha);
            }
        }
    }
}

/// 火箭本体 + 尾焰（含马赫环）与火焰对地照度。
fn draw_rocket(grid: &mut PixelGrid, layout: &Layout, t: f32, up: i32) {
    let base_y = layout.ground - 1 - up; // 箭体底缘
    let body_h = 13;
    let nozzle = base_y + 1;
    let burning = thrust(t);

    // 尾焰：核心亮黄 → 外圈橙红；下方留两段马赫环。
    if burning {
        let flick = hash1((t * 12.) as i32, 241);
        let len = (6. + flick * 3.) as i32;
        for k in 0..len {
            let lift = k as f32 / len as f32;
            let width = (2. + (1. - lift) * 2.5).round() as i32;
            let sway = ((t * 8. + k as f32) * 0.9).sin().round() as i32 * if lift > 0.4 { 1 } else { 0 };
            let y = nozzle + 1 + k;
            for dx in -width..=width {
                let core = dx.abs() as f32 / width.max(1) as f32;
                let diamond = k % 4 >= 2;
                let color = if core < 0.4 {
                    FLAME_CORE
                } else if diamond {
                    0xffd070
                } else if core < 0.75 {
                    FLAME_MID
                } else {
                    FLAME_OUT
                };
                grid.put(layout.pad_x + dx + sway, y, Rgb::hex(color));
            }
        }
        // 火光照地：从箭底以曼哈顿距离分级铺光。
        let glow_level = (t - IGNITE_AT) < 1.5 || up < 30;
        if glow_level {
            for y in layout.ground.max(0)..grid.height() as i32 {
                for x in 0..grid.width() as i32 {
                    let d = (x - layout.pad_x).abs() + (y - layout.ground).abs() * 2;
                    let strength = (1. - d as f32 / 26.).max(0.);
                    if strength > 0. {
                        grid.glow(
                            x,
                            y,
                            Rgb::hex(0xffb060).scale(((strength * 4.).ceil() / 4.) * 0.16),
                        );
                    }
                }
            }
        }
    }

    // 箭体：锥头 / 白身（右缘暗一档）/ 舷窗 / 尾翼 / 喷口。
    if base_y - body_h < grid.height() as i32 + 4 {
        for k in 0..body_h {
            let y = base_y - k;
            let top = k >= body_h - 3; // 锥头三行
            for dx in -2..=2 {
                let x = layout.pad_x + dx;
                let color = if top {
                    let taper = (body_h - 1 - k) - dx.abs();
                    if taper < 0 { Rgb::BLACK } else { NOSE }
                } else if dx == 2 {
                    BODY_SHADE
                } else if dx == 0 && k == 5 {
                    Rgb::hex(0x62c8e8) // 舷窗
                } else if k == body_h - 5 {
                    NOSE // 腰环
                } else {
                    BODY
                };
                if color != Rgb::BLACK {
                    grid.put(x, y, color);
                }
            }
        }
        // 尾翼。
        for k in 0..3 {
            let y = base_y - k;
            grid.put(layout.pad_x - 3, y, Rgb::hex(if k == 2 { 0x9aa8b8 } else { 0xd0d8e2 }));
            grid.put(layout.pad_x + 3, y, Rgb::hex(if k == 2 { 0x9aa8b8 } else { 0xb8c2ce }));
        }
        grid.put(layout.pad_x - 1, nozzle, Rgb::hex(0x4a4e58));
        grid.put(layout.pad_x, nozzle, Rgb::hex(0x393d46));
        grid.put(layout.pad_x + 1, nozzle, Rgb::hex(0x4a4e58));
    }
}
