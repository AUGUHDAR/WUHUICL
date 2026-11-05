//! Vendored from EdwinZhanCN/LumilioCL v0.2.3 (AGPL-3.0), crates/lumilio-ui/src/hero/scenes/hearth.rs. Changes: module paths only.
//! 营火 · Hearth — Continue's own scene: a night camp whose fire is still
//! burning where the player left it. A cottage window glows, campfire smoke
//! climbs in a tall column, and the fire's block light falls off one level per
//! block across the ground. The instance's world appears as a landmark on the
//! far hill. `flare` (0–1) makes the fire leap when the pointer rests on
//! **继续**: taller flames, more sparks, light reaching further.

use super::caves::brightness;
use crate::hero_scenes::noise::{hash1, hash2, smoothstep, value1, value2};
use crate::hero_scenes::raster::{PixelGrid, Rgb};

/// What the far hill shows, chosen from the instance's world.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum Landmark {
    /// Distant village windows.
    #[default]
    Village,
    Portal,
    Mine,
    Lamp,
}

pub const FIRE_LEVEL: f32 = 15.;
/// Extra blocks of light reach at full flare.
const FLARE_REACH: f32 = 3.;
const BLOCK: i32 = 4;
const NIGHT: Rgb = Rgb::new(0.5, 0.58, 0.92);
const FIRELIGHT: Rgb = Rgb::new(1., 0.8, 0.56);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Layout {
    pub horizon: i32,
    pub ground: i32,
    pub fire_x: i32,
    pub house_x: i32,
    pub landmark_x: i32,
}

impl Layout {
    pub fn new(width: i32, height: i32) -> Self {
        Self {
            horizon: (height as f32 * 0.58) as i32,
            ground: (height as f32 * 0.78) as i32,
            fire_x: (width as f32 * 0.5) as i32,
            house_x: (width as f32 * 0.66) as i32,
            landmark_x: (width as f32 * 0.88) as i32,
        }
    }

    fn hill_top(&self, x: i32) -> i32 {
        let bx = x.div_euclid(3);
        let rise = 2 + (value1(bx as f32 * 0.13, 101) * 5.).round() as i32;
        self.horizon - rise * 3 + 6
    }
}

/// Block light from the campfire at a texel: one level lost per block of
/// Manhattan distance, reaching further while the fire flares.
pub fn fire_light(x: i32, y: i32, layout: &Layout, flare: f32) -> f32 {
    let fire = (
        layout.fire_x.div_euclid(BLOCK),
        (layout.ground - 1).div_euclid(BLOCK),
    );
    let distance = (x.div_euclid(BLOCK) - fire.0).abs() + (y.div_euclid(BLOCK) - fire.1).abs();
    (FIRE_LEVEL + flare * FLARE_REACH - distance as f32).clamp(0., FIRE_LEVEL)
}

fn lit(color: Rgb, level: f32) -> Rgb {
    let exposure = 0.2 + 0.8 * brightness(level).powf(0.6);
    color
        .tint(NIGHT.lerp(FIRELIGHT, smoothstep(0., 5., level)))
        .scale(exposure)
}

/// Flame height in texels.
pub fn flame_height(age: f32, flare: f32) -> i32 {
    let flicker = (hash1((age * 10.) as i32, 111) * 2.) as i32;
    6 + (flare * 5.).round() as i32 + flicker
}

pub fn render(grid: &mut PixelGrid, age: f32, landmark: Landmark, flare: f32) {
    let (w, h) = (grid.width() as i32, grid.height() as i32);
    let layout = Layout::new(w, h);
    let sky_top = Rgb::hex(0x060a1c);
    let sky_low = Rgb::hex(0x1d2446);
    let (moon_x, moon_y) = ((w as f32 * 0.3) as i32, (h as f32 * 0.17) as i32);
    let moon_half = (h / 22).max(2);

    // Night sky, stars, and the moon's square halo.
    grid.shade(|x, y| {
        let (xi, yi) = (x as i32, y as i32);
        let s = (yi as f32 / layout.horizon as f32).clamp(0., 1.);
        let banded = ((s * 12.).floor() / 12.).powf(1.3);
        let mut color = sky_top.lerp(sky_low, banded);
        if yi < layout.horizon * 4 / 5 && hash2(xi, yi, 91) > 0.984 {
            let twinkle = 0.5 + 0.5 * (age * 1.9 + hash2(xi, yi, 92) * 40.).sin();
            color = color.lerp(Rgb::hex(0xeef0ff), 0.35 + 0.6 * twinkle);
        }
        let ring = (xi - moon_x).abs().max((yi - moon_y).abs());
        if ring > moon_half && ring <= moon_half * 3 {
            let falloff = 1. - (ring - moon_half) as f32 / (moon_half * 2) as f32;
            color = color.add(Rgb::hex(0x7f90c8).scale(((falloff * 3.).ceil() / 3.) * 0.1));
        }
        color
    });
    // A square moon with a shaded limb and two diagonal craters (never a face).
    for dy in -moon_half..=moon_half {
        for dx in -moon_half..=moon_half {
            let crater = (dx, dy) == (1, -1) || (dx, dy) == (-1, 1);
            let limb = dx == -moon_half || dy == moon_half;
            let color = if crater {
                0xb9c1d6
            } else if limb {
                0xd3d9e8
            } else {
                0xe9edf7
            };
            grid.put(moon_x + dx, moon_y + dy, Rgb::hex(color));
        }
    }

    // Far hills.
    for x in 0..w {
        let top = layout.hill_top(x);
        for y in top.max(0)..h {
            let color = if y == top { 0x1f2a4e } else { 0x141b36 };
            grid.put(x, y, Rgb::hex(color));
        }
    }
    draw_landmark(grid, &layout, landmark, age);

    // Spruce silhouettes, kept clear of the camp itself.
    let camp = (layout.fire_x - 8)..(layout.house_x + 16);
    for anchor in (0..w).step_by(7) {
        let jitter = (hash1(anchor, 94) * 4.) as i32;
        let x0 = anchor + jitter;
        if hash1(anchor, 95) < 0.45 || camp.contains(&x0) {
            continue;
        }
        let height = 7 + (hash1(anchor, 96) * 7.) as i32;
        for level in 0..height {
            let half = ((height - level) / 3).max(if level < 2 { 0 } else { 1 });
            for dx in -half..=half {
                grid.put(x0 + dx, layout.ground - 1 - level, Rgb::hex(0x0b1122));
            }
        }
    }

    // Ground, lit by the fire.
    for y in layout.ground.max(0)..h {
        for x in 0..w {
            let depth = y - layout.ground;
            let speckle = hash2(x, y, 97);
            let base = if depth == 0 || (depth == 1 && hash1(x, 98) > 0.5) {
                if speckle > 0.7 { 0x4f8e3c } else { 0x3f7a33 }
            } else if speckle < 0.18 {
                0x4a311e
            } else if speckle > 0.88 {
                0x6b4a2f
            } else {
                0x5b3d26
            };
            grid.set(
                x as usize,
                y as usize,
                lit(Rgb::hex(base), fire_light(x, y, &layout, flare)),
            );
        }
    }

    draw_cottage(grid, &layout, age, flare);
    draw_fire(grid, &layout, age, flare);

    // Fireflies drifting low over the grass.
    for i in 0..12 {
        let drift = age * 0.25 + hash1(i, 99) * 50.;
        let x = (hash1(i, 100) * w as f32 + value1(drift, 102) * 16. - 8.) as i32;
        let y = layout.ground - 2 - (value1(drift + 7., 103) * 10.) as i32;
        let blink = (age * 2.1 + hash1(i, 104) * 9.).sin();
        if blink > 0.1 {
            grid.blend(x, y, Rgb::hex(0xd8ff6a), 0.4 + blink * 0.5);
        }
    }
}

fn draw_cottage(grid: &mut PixelGrid, layout: &Layout, age: f32, flare: f32) {
    let (left, right) = (layout.house_x - 11, layout.house_x + 11);
    let wall_top = layout.ground - 12;
    for y in wall_top..layout.ground {
        for x in left..=right {
            let post = x == left || x == right;
            let seam = (y - wall_top) % 3 == 2;
            let base = if post {
                0x4a3220
            } else if seam {
                0x5a3d24
            } else {
                0x6e4c2e
            };
            let level = fire_light(x, y, layout, flare);
            grid.put(x, y, lit(Rgb::hex(base), level));
        }
    }
    // Stepped roof.
    for step in 0..7 {
        let y = wall_top - 1 - step;
        for x in (left - 2 + step * 2)..=(right + 2 - step * 2) {
            let edge = x == left - 2 + step * 2 || x == right + 2 - step * 2;
            let base = if edge { 0x2a1d17 } else { 0x3a2a22 };
            grid.put(
                x,
                y,
                lit(Rgb::hex(base), fire_light(x, y, layout, flare) * 0.8),
            );
        }
    }
    // A warm window that never quite holds still, and the door.
    let glow = 0.9 + 0.1 * (age * 3.3).sin();
    for dy in 0..4 {
        for dx in 0..5 {
            let bar = dx == 2 || dy == 2;
            let color = if bar { 0x6e4c2e } else { 0xffd27a };
            let texel = if bar {
                lit(
                    Rgb::hex(color),
                    fire_light(left + 4 + dx, wall_top + 3, layout, flare),
                )
            } else {
                Rgb::hex(color).scale(glow)
            };
            grid.put(left + 4 + dx, wall_top + 3 + dy, texel);
        }
    }
    for dy in 0..7 {
        for dx in 0..4 {
            let (x, y) = (right - 7 + dx, layout.ground - 7 + dy);
            grid.put(
                x,
                y,
                lit(Rgb::hex(0x3a2616), fire_light(x, y, layout, flare)),
            );
        }
    }
}

fn draw_fire(grid: &mut PixelGrid, layout: &Layout, age: f32, flare: f32) {
    let (fx, ground) = (layout.fire_x, layout.ground);
    let slot = (age * 12.) as u32;

    // Logs and embers.
    for dx in -3..=3 {
        let log = if (dx + 3) % 2 == 0 {
            0x5a3a22
        } else {
            0x3e2818
        };
        grid.put(fx + dx, ground - 1, Rgb::hex(log));
        if dx.abs() <= 1 {
            let hot = hash2(fx + dx, 0, slot) > 0.4;
            grid.put(
                fx + dx,
                ground - 2,
                Rgb::hex(if hot { 0xff8a2a } else { 0xc24a18 }),
            );
        }
    }

    // Flame.
    let height = flame_height(age, flare);
    for k in 0..height {
        let rise = k as f32 / height as f32;
        let width = ((1. - rise) * (3. + flare)).ceil() as i32;
        let sway = ((age * 9. + k as f32 * 0.7).sin() * 1.2 * rise).round() as i32;
        let y = ground - 3 - k;
        for dx in -width..=width {
            if rise > 0.5 && hash2(fx + dx, y, slot) > 0.78 {
                continue;
            }
            let core = dx.abs() as f32 / (width.max(1) as f32);
            let color = if rise < 0.35 && core < 0.6 {
                0xfff4b0
            } else if rise < 0.7 {
                0xffb43a
            } else {
                0xff6a1a
            };
            grid.put(fx + dx + sway, y, Rgb::hex(color));
        }
    }

    // Campfire smoke: a tall column of puffs drifting with the wind.
    let top = ground - 3 - height;
    let reach = (grid.height() as f32 * 0.62).max(20.);
    for i in 0..16 {
        let phase = (age * 0.09 + i as f32 / 16.).fract();
        let x = fx as f32 + (phase * 5. + i as f32).sin() * 1.4 + phase * phase * 12.;
        let y = top as f32 - phase * reach;
        let size = 1 + (phase * 2.6) as i32;
        let alpha = (1. - phase) * 0.42 * smoothstep(0., 0.06, phase);
        let color = Rgb::hex(0xc9a080).lerp(Rgb::hex(0x8a90a0), smoothstep(0., 0.25, phase));
        for dy in 0..size {
            for dx in 0..size {
                grid.blend(x as i32 + dx, y as i32 - dy, color, alpha);
            }
        }
    }

    // Sparks: more and livelier while the fire flares.
    let sparks = 8 + (flare * 12.) as i32;
    for i in 0..sparks {
        let speed = 0.7 + hash1(i, 105) * 0.7 + flare * 0.5;
        let phase = (age * speed + hash1(i, 106)).fract();
        if phase > 0.85 {
            continue;
        }
        let x =
            fx as f32 + (hash1(i, 107) - 0.5) * 7. * phase + value2(age * 2., i as f32, 108) * 2.
                - 1.;
        let y = (top + 2) as f32 - phase * (12. + flare * 12.);
        let color = Rgb::hex(0xffd25a).lerp(Rgb::hex(0xff5a1a), phase);
        grid.put(x as i32, y as i32, color);
    }
}

fn draw_landmark(grid: &mut PixelGrid, layout: &Layout, landmark: Landmark, age: f32) {
    let x0 = layout.landmark_x;
    // Seat the landmark on the lowest point of the hill under it, so it
    // never floats above the ridge.
    let base = (x0 - 4..x0 + 4)
        .map(|x| layout.hill_top(x))
        .max()
        .unwrap_or(layout.horizon);
    match landmark {
        Landmark::Village => {
            for (dx, dy) in [(-6, 2), (-1, 4), (4, 3)] {
                let on = hash1(dx, (age * 0.7) as u32) > 0.08;
                if on {
                    grid.put(x0 + dx, base + dy, Rgb::hex(0xffc864));
                }
            }
        }
        Landmark::Portal => {
            let (left, top) = (x0 - 4, base - 10);
            for y in top..base {
                for x in left..left + 8 {
                    let inner = x > left + 1 && x < left + 6 && y > top + 1 && y < base - 1;
                    let color = if inner {
                        let swirl = value2(x as f32 * 0.7, y as f32 * 0.7 - age * 1.4, 109);
                        [0x3a0f7a, 0x6f2bd6, 0xa565ff][(swirl * 3.) as usize % 3]
                    } else {
                        0x150c24
                    };
                    grid.put(x, y, Rgb::hex(color));
                }
            }
            // Violet light in two stepped rings, not a box.
            for y in top - 3..base {
                for x in left - 3..left + 11 {
                    let dx = (left - x).max(x - (left + 7)).max(0);
                    let dy = (top - y).max(0);
                    let ring = dx.max(dy);
                    if ring > 0 && dx + dy <= 3 {
                        let strength = if ring == 1 { 0.1 } else { 0.05 };
                        grid.glow(x, y, Rgb::hex(0x7a3cff).scale(strength));
                    }
                }
            }
        }
        Landmark::Mine => {
            let (left, top) = (x0 - 3, base + 1);
            for y in top..top + 5 {
                for x in left..left + 7 {
                    let frame = x == left || x == left + 6 || y == top;
                    grid.put(x, y, Rgb::hex(if frame { 0x4a3220 } else { 0x04060c }));
                }
            }
            let bright = hash1((age * 8.) as i32, 110) > 0.4;
            grid.put(left + 7, top + 1, Rgb::hex(0x7a5530));
            grid.put(
                left + 7,
                top,
                Rgb::hex(if bright { 0xffd65a } else { 0xffa42e }),
            );
            grid.glow(left + 7, top - 1, Rgb::hex(0xff9a40).scale(0.25));
        }
        Landmark::Lamp => {
            for y in base - 5..base {
                grid.put(x0, y, Rgb::hex(0x4a3220));
            }
            let on = (age / 0.8) as i32 % 2 == 0;
            for dy in 0..4 {
                for dx in 0..4 {
                    let lattice = (dx + dy) % 2 == 0;
                    let color = match (on, lattice) {
                        (true, true) => 0xffe6a0,
                        (true, false) => 0xf2a848,
                        (false, true) => 0x6a4a2c,
                        (false, false) => 0x3a2616,
                    };
                    grid.put(x0 - 2 + dx, base - 9 + dy, Rgb::hex(color));
                }
            }
            if on {
                for dy in -2..6 {
                    for dx in -4..6 {
                        grid.glow(x0 - 2 + dx, base - 9 + dy, Rgb::hex(0xffb050).scale(0.05));
                    }
                }
            }
        }
    }
}

