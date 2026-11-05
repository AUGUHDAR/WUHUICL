//! Vendored from EdwinZhanCN/LumilioCL v0.2.3 (AGPL-3.0), crates/lumilio-ui/src/hero/scenes/portal.rs. Changes: module paths only.
//! 下界 · Portal — a 4×5 obsidian frame in a netherrack cavern. Sparks fly,
//! the portal fills upward from its base, then swirls and pulls particles in
//! while its violet light spills over the frame and floor in block-sized steps.

use std::f32::consts::TAU;

use crate::hero_scenes::noise::{hash1, hash2, smoothstep, value1, value2};
use crate::hero_scenes::raster::{PixelGrid, Rgb};

pub const FRAME_BLOCKS: (i32, i32) = (4, 5);
pub const IGNITE_AT: f32 = 1.3;
const FILL_SECONDS: f32 = 0.8;
const SWIRL: [u32; 5] = [0x2c0a64, 0x4b17a3, 0x6f2bd6, 0x9a55f5, 0xd7b2ff];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Layout {
    pub block: i32,
    pub floor: i32,
    /// Outer frame rectangle in texels: x, y, width, height.
    pub frame: (i32, i32, i32, i32),
}

impl Layout {
    pub fn new(width: i32, height: i32) -> Self {
        let block = (height / 9).clamp(4, 12);
        let floor = height - block - block / 2;
        let (fw, fh) = (FRAME_BLOCKS.0 * block, FRAME_BLOCKS.1 * block);
        let left = (width as f32 * 0.7) as i32 - fw / 2;
        Self {
            block,
            floor,
            frame: (left, floor - fh, fw, fh),
        }
    }

    /// Interior rectangle (2×3 blocks).
    pub fn inner(&self) -> (i32, i32, i32, i32) {
        let (x, y, w, h) = self.frame;
        let b = self.block;
        (x + b, y + b, w - 2 * b, h - 2 * b)
    }

    fn contains_frame(&self, x: i32, y: i32) -> bool {
        let (fx, fy, fw, fh) = self.frame;
        x >= fx && y >= fy && x < fx + fw && y < fy + fh
    }
}

/// 0 before ignition, rising to 1 as the portal fills.
pub fn charge(age: f32) -> f32 {
    smoothstep(IGNITE_AT, IGNITE_AT + FILL_SECONDS, age)
}

fn netherrack(x: i32, y: i32, block: i32) -> Rgb {
    let tone = 1. + (hash2(x.div_euclid(block), y.div_euclid(block), 61) - 0.5) * 0.14;
    let speckle = hash2(x, y, 62);
    let base = if speckle > 0.84 {
        Rgb::hex(0x8e3434)
    } else if speckle < 0.2 {
        Rgb::hex(0x4e1616)
    } else {
        Rgb::hex(0x6c2322)
    };
    base.scale(tone)
}

fn lava_fall(x: i32, y: i32, age: f32) -> Rgb {
    let flow = value2(x as f32 * 0.5, y as f32 * 0.22 - age * 4., 63);
    Rgb::hex(match (flow * 4.) as u32 {
        0 => 0xb8340c,
        1 => 0xe8641a,
        2 => 0xffa030,
        _ => 0xffd060,
    })
}

pub fn render(grid: &mut PixelGrid, age: f32) {
    let (w, h) = (grid.width() as i32, grid.height() as i32);
    let layout = Layout::new(w, h);
    let b = layout.block;
    let charge = charge(age);
    let falls = [(w as f32 * 0.16) as i32, (w as f32 * 0.93) as i32];
    let fall_width = (b * 3 / 4).max(2);
    let ceiling = |x: i32| {
        let bx = x.div_euclid(b);
        (1 + (value1(bx as f32 * 0.4, 64) * 2.4).round() as i32) * b
    };
    let glowstone = ((w as f32 * 0.44) as i32).div_euclid(b);

    // Cavern backdrop, ceiling, lava falls and the glow they cast.
    grid.shade(|x, y| {
        let (xi, yi) = (x as i32, y as i32);
        let fall_distance = falls
            .iter()
            .map(|&fx| (xi - fx).abs())
            .min()
            .unwrap_or(i32::MAX);
        if yi < ceiling(xi) {
            let bx = xi.div_euclid(b);
            let by = yi.div_euclid(b);
            if (glowstone..glowstone + 3).contains(&bx) && by * b + b == ceiling(xi) {
                let spot = hash2(xi, yi, 65);
                return Rgb::hex(if spot > 0.7 {
                    0xfff0b0
                } else if spot > 0.3 {
                    0xffcf6a
                } else {
                    0xb8803a
                });
            }
            return netherrack(xi, yi, b).scale(0.55);
        }
        if fall_distance * 2 < fall_width && yi < layout.floor {
            return lava_fall(xi, yi, age);
        }
        let depth = yi as f32 / h as f32;
        let mut color = Rgb::hex(0x16040a).lerp(Rgb::hex(0x4a1012), depth.powf(1.4));
        // Far wall mottling.
        if value2(xi as f32 * 0.08, yi as f32 * 0.12, 66) > 0.62 {
            color = color.scale(1.25);
        }
        let heat = (1. - fall_distance as f32 / (b as f32 * 2.5)).max(0.);
        color = color.add(Rgb::hex(0xff6a1a).scale(((heat * 3.).ceil() / 3.) * 0.22));
        let glow_x = (xi.div_euclid(b) - glowstone - 1).abs() as f32;
        let glow_y = (yi - ceiling(xi)) as f32 / b as f32;
        let warm = (1. - (glow_x + glow_y) / 5.).max(0.);
        color.add(Rgb::hex(0xffc060).scale(((warm * 3.).ceil() / 3.) * 0.12))
    });

    // Floor.
    for y in layout.floor..h {
        for x in 0..w {
            let mut color = netherrack(x, y, b);
            if y == layout.floor {
                color = color.scale(1.25);
            }
            grid.set(x as usize, y as usize, color);
        }
    }

    // Obsidian frame.
    let (ix, iy, iw, ih) = layout.inner();
    let (fx, fy, fw, fh) = layout.frame;
    for y in fy..fy + fh {
        for x in fx..fx + fw {
            let inside = x >= ix && x < ix + iw && y >= iy && y < iy + ih;
            if inside {
                continue;
            }
            let speckle = hash2(x, y, 67);
            let mut color = Rgb::hex(if speckle > 0.95 {
                0x3c2766
            } else if speckle > 0.8 {
                0x2a1a44
            } else {
                0x130b20
            });
            if (x - fx) % b == 0 || (y - fy) % b == 0 {
                color = color.scale(0.8);
            }
            grid.put(x, y, color);
        }
    }

    // Portal surface, filling from the base with a ragged leading edge.
    let centre = (ix as f32 + iw as f32 / 2., iy as f32 + ih as f32 / 2.);
    if charge > 0. {
        for y in iy..iy + ih {
            for x in ix..ix + iw {
                let height = (iy + ih - y) as f32;
                let edge = charge * ih as f32 * 1.1 + (hash2(x, 0, 68) - 0.5) * 3.;
                if height > edge {
                    continue;
                }
                let (dx, dy) = (
                    (x as f32 - centre.0) / b as f32,
                    (y as f32 - centre.1) / b as f32,
                );
                let angle = dy.atan2(dx);
                let radius = (dx * dx + dy * dy).sqrt();
                let spiral = (angle * 2. + radius * 2.2 - age * 2.6).sin() * 0.5 + 0.5;
                let churn = value2(x as f32 * 0.32 + age * 0.9, y as f32 * 0.32 - age * 1.7, 69);
                let level = ((spiral * 0.55 + churn * 0.6) * 4.).clamp(0., 3.99) as usize;
                let mut color = Rgb::hex(SWIRL[level]);
                if edge - height < 1.5 && charge < 1. {
                    color = Rgb::hex(SWIRL[4]);
                }
                grid.blend(x, y, color, 0.9);
            }
        }
    }

    // Violet light spilling out, stepped per block of distance.
    if charge > 0. {
        let pulse = 0.9 + 0.1 * (age * 3.1).sin();
        for y in 0..h {
            for x in 0..w {
                let outside_x = (ix - x).max(x - (ix + iw - 1)).max(0);
                let outside_y = (iy - y).max(y - (iy + ih - 1)).max(0);
                let reach = (outside_x.max(outside_y) + b - 1) / b;
                if reach == 0 || reach > 5 {
                    continue;
                }
                let strength = (1. - reach as f32 / 6.) * 0.2 * charge * pulse;
                let boost = if layout.contains_frame(x, y) { 0.7 } else { 1. };
                grid.glow(x, y, Rgb::hex(0x7a3cff).scale(strength * boost));
            }
        }
    }

    // Flint-and-steel sparks just before ignition.
    let spark = age - (IGNITE_AT - 0.35);
    if (0. ..0.6).contains(&spark) {
        for i in 0..9 {
            let vx = (hash1(i, 70) - 0.5) * b as f32 * 4.;
            let vy = -(0.5 + hash1(i, 71)) * b as f32 * 4.;
            let px = centre.0 + vx * spark;
            let py = (iy + ih - 1) as f32 + vy * spark + b as f32 * 9. * spark * spark;
            grid.put(
                px as i32,
                py as i32,
                Rgb::hex(if i % 2 == 0 { 0xffe08a } else { 0xff8a2a }),
            );
        }
    }

    // Particles drawn into the portal on a slow spiral.
    if charge > 0. {
        for i in 0..40 {
            let phase = (age * 0.32 + hash1(i, 72)).fract();
            let start_angle = hash1(i, 73) * TAU;
            let start_radius = b as f32 * (1.6 + hash1(i, 74) * 2.6);
            let angle = start_angle + phase * 2.4;
            let radius = start_radius * (1. - phase);
            let px = centre.0 + angle.cos() * radius;
            let py = centre.1 + angle.sin() * radius * 1.2;
            let color = Rgb::hex(0x9a62ff).lerp(Rgb::hex(0xe8d4ff), phase);
            grid.blend(
                px as i32,
                py as i32,
                color,
                charge * smoothstep(0., 0.15, phase),
            );
        }
    }

    // Ash drifting through the whole cavern.
    for i in 0..34 {
        let fall = (age * (0.03 + hash1(i, 75) * 0.03) + hash1(i, 76)).fract();
        let sway = (age * 0.8 + hash1(i, 77) * TAU).sin() * 2.;
        let px = hash1(i, 78) * w as f32 + sway + fall * w as f32 * 0.12;
        let py = fall * h as f32;
        grid.blend(px as i32 % w.max(1), py as i32, Rgb::hex(0xb0a0a0), 0.55);
    }

    // A brief violet flash as the portal catches.
    let flash = (-((age - IGNITE_AT) / 0.14).powi(2)).exp() * 0.2;
    if flash > 0.01 {
        let tint = Rgb::hex(0xc8a8ff).scale(flash);
        for y in 0..h {
            for x in 0..w {
                grid.glow(x, y, tint);
            }
        }
    }
}

