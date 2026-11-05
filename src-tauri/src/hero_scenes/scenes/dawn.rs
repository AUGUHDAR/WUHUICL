//! Vendored from EdwinZhanCN/LumilioCL v0.2.3 (AGPL-3.0), crates/lumilio-ui/src/hero/scenes/dawn.rs. Changes: module paths only.
//! 主世界 · Dawn — a square sun rises over a slowly panning, block-quantised
//! landscape. Sky bands, stars, cloud slabs, terrain light and the water's sun
//! glitter are all driven by one daylight value, so the slide *is* a sunrise.

use crate::hero_scenes::noise::{bayer4, hash1, hash2, smoothstep, value1};
use crate::hero_scenes::raster::{PixelGrid, Rgb};

/// Daylight in `[0, 1]`: 0 is deep night, ~0.55 the orange moment, 1 morning.
pub fn daylight(age: f32) -> f32 {
    0.06 + 0.94 * smoothstep(0.4, 8.8, age)
}

pub fn horizon_row(height: i32) -> i32 {
    (height as f32 * 0.62) as i32
}

/// Sun centre and half-size in texels. It starts below the horizon.
pub fn sun(width: i32, height: i32, day: f32) -> (i32, i32, i32) {
    let half = (height / 16).max(3);
    let low = (horizon_row(height) + half + 2) as f32;
    let high = height as f32 * 0.2;
    let y = low + (high - low) * smoothstep(0.1, 1., day);
    ((width as f32 * 0.7) as i32, y.round() as i32, half)
}

/// In-game clock shown in the HUD chip.
pub fn clock(age: f32) -> String {
    let minutes = (5. * 60. + 4.) + smoothstep(0., 9.5, age) * 92.;
    let minutes = minutes as u32;
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

fn dawnness(day: f32) -> f32 {
    (1. - (day - 0.55).abs() / 0.45).clamp(0., 1.)
}

fn ramp(day: f32, night: u32, dawn: u32, morning: u32) -> Rgb {
    if day < 0.55 {
        Rgb::hex(night).lerp(Rgb::hex(dawn), day / 0.55)
    } else {
        Rgb::hex(dawn).lerp(Rgb::hex(morning), (day - 0.55) / 0.45)
    }
}

pub fn render(grid: &mut PixelGrid, age: f32) {
    let (w, h) = (grid.width() as i32, grid.height() as i32);
    let day = daylight(age);
    let dawn = dawnness(day);
    let sky_top = ramp(day, 0x080c26, 0x26387a, 0x4b7fd8);
    let sky_low = ramp(day, 0x2a2350, 0xf0956a, 0xf2cfa2);
    let light = ramp(day, 0x3a4070, 0xf2b894, 0xfff3e6);
    let horizon = horizon_row(h);
    let (sun_x, sun_y, half) = sun(w, h, day);
    let stars = (1. - day * 1.8).max(0.);

    // Sky: dithered bands, a warm lobe under the sun, stars and a square halo.
    grid.shade(|x, y| {
        let (xi, yi) = (x as i32, y as i32);
        let s = (y as f32 / horizon as f32).clamp(0., 1.);
        // Hard colour bands, like a low-colour sky; dithering only on the
        // band seams keeps them from looking like a smooth gradient.
        let seam = (s * 14.).fract() > 0.82 && bayer4(xi, yi) > 0.5;
        let banded = (((s * 14.).floor() + f32::from(u8::from(seam))) / 14.).clamp(0., 1.);
        let mut color = sky_top.lerp(sky_low, banded.powf(1.5));

        let lobe = (1. - (xi - sun_x).abs() as f32 / (w as f32 * 0.5)).max(0.);
        let warm = ((lobe * banded * banded * dawn * 4.).floor() / 4.).min(1.);
        color = color.lerp(Rgb::hex(0xff8a48), warm * 0.45);

        if stars > 0. && yi < horizon * 2 / 3 && hash2(xi, yi, 7) > 0.986 {
            let twinkle = 0.55 + 0.45 * (age * 2.3 + hash2(xi, yi, 8) * 40.).sin();
            color = color.lerp(Rgb::hex(0xf4f2ff), stars * twinkle);
        }

        let ring = (xi - sun_x).abs().max((yi - sun_y).abs());
        if ring > half && ring <= half * 4 {
            let falloff = 1. - (ring - half) as f32 / (half * 3) as f32;
            let stepped = (falloff * 4.).ceil() / 4.;
            color = color.add(Rgb::hex(0xffb070).scale(stepped * stepped * 0.22));
        }
        color
    });

    // The sun itself: a hard square with a hot core.
    for dy in -half..=half {
        for dx in -half..=half {
            let ring = dx.abs().max(dy.abs());
            let color = if ring == half {
                Rgb::hex(0xffc35c)
            } else if ring >= half - 1 {
                Rgb::hex(0xffe6a0)
            } else {
                Rgb::hex(0xfff8dc)
            };
            grid.put(sun_x + dx, sun_y + dy, color);
        }
    }

    let cloud_top = Rgb::hex(0xffffff).lerp(sky_low, 0.3).tint(light);
    let cloud_under = cloud_top.lerp(Rgb::hex(0x6a5a8a), 0.35);
    let high = CloudLayer {
        row: (h as f32 * 0.07) as i32,
        thickness: (h / 28).max(1),
        cell: (w / 18).max(6),
        scroll: 13. + age * 0.9,
        seed: 21,
        opacity: 0.5,
    };
    high.draw(grid, cloud_top, cloud_under);

    // Far mountains, hazed into the horizon colour.
    let far_block = (h / 26).max(2);
    let far_scroll = (60. + age * 0.6) as i32;
    for x in 0..w {
        let bx = (x + far_scroll).div_euclid(far_block);
        let blocks = 3
            + (value1(bx as f32 * 0.09, 3) * 9. + value1(bx as f32 * 0.27, 4) * 3.).round() as i32;
        let top = horizon + far_block * 2 - blocks * far_block;
        let tone = 1. - hash1(bx, 5) * 0.06;
        for y in top.max(0)..h {
            let snow = blocks >= 10 && y - top < far_block;
            let base = if snow {
                Rgb::hex(0xdfe6f5)
            } else {
                Rgb::hex(0x56618c).scale(tone)
            };
            let mut color = base.tint(light).lerp(sky_low, 0.5);
            if y == top {
                color = color.add(Rgb::hex(0xff9a50).scale(0.22 * dawn));
            }
            grid.put(x, y, color);
        }
    }

    let low = CloudLayer {
        row: (h as f32 * 0.17) as i32,
        thickness: (h / 20).max(2),
        cell: (w / 26).max(5),
        scroll: 71. + age * 1.7,
        seed: 22,
        opacity: 0.88,
    };
    low.draw(grid, cloud_top, cloud_under);

    // Mid hills with block oaks.
    let mid_block = (h / 18).max(3);
    let mid_scroll = (140. + age * 1.4) as i32;
    let mid_top = |bx: i32| {
        let blocks = 1 + (value1(bx as f32 * 0.16, 9) * 4.).round() as i32;
        horizon + mid_block * 3 - blocks * mid_block
    };
    let is_tree = |bx: i32| hash1(bx, 10) > 0.8 && hash1(bx - 1, 10) <= 0.8;
    for x in 0..w {
        let wx = x + mid_scroll;
        let bx = wx.div_euclid(mid_block);
        let top = mid_top(bx);
        for y in top.max(0)..h {
            let grass = y - top < 1;
            let base = if grass {
                Rgb::hex(0x4f8f3a)
            } else {
                Rgb::hex(0x3a5a2c).scale(1. - hash2(wx, y, 11) * 0.12)
            };
            let mut color = base.tint(light).lerp(sky_low, 0.26);
            if y == top {
                color = color.add(Rgb::hex(0xff9a50).scale(0.16 * dawn));
            }
            grid.put(x, y, color);
        }
        for anchor in bx - 1..=bx + 1 {
            if !is_tree(anchor) {
                continue;
            }
            let ground = mid_top(anchor);
            let column = bx - anchor;
            for y in (ground - 5 * mid_block).max(0)..ground {
                let level = (ground - 1 - y) / mid_block; // 0 = trunk base
                let part = match (level, column) {
                    (0 | 1, 0) => Some(Rgb::hex(0x5a3d22)),
                    (2 | 3, -1..=1) => Some(Rgb::hex(0x3d7a2c)),
                    (4, 0) => Some(Rgb::hex(0x3d7a2c)),
                    _ => None,
                };
                if let Some(base) = part {
                    let shade = if level >= 2 && hash2(wx, y, 12) > 0.8 {
                        1.15
                    } else {
                        1.
                    };
                    grid.put(x, y, base.scale(shade).tint(light).lerp(sky_low, 0.26));
                }
            }
        }
    }

    // Near ground: grass cap with overhang, dirt, stone, sandy shallows and water.
    let near_block = (h / 12).max(4);
    let near_scroll = (230. + age * 2.8) as i32;
    let water_line = h - near_block * 2 - near_block / 2;
    let sun_up = smoothstep(0.25, 0.6, day);
    for x in 0..w {
        let wx = x + near_scroll;
        let bx = wx.div_euclid(near_block);
        let blocks = 1 + (value1(bx as f32 * 0.12, 13) * 4.4).round() as i32;
        let top = h - blocks * near_block;
        let submerged = top > water_line;
        for y in top.max(0)..h {
            let depth = y - top;
            let speckle = hash2(wx, y, 14);
            let base = if submerged && depth < 2 {
                if speckle > 0.5 {
                    Rgb::hex(0xd8c890)
                } else {
                    Rgb::hex(0xc4b27a)
                }
            } else if depth == 0 || (depth == 1 && hash1(wx, 15) > 0.45) {
                if speckle > 0.7 {
                    Rgb::hex(0x7cbd46)
                } else {
                    Rgb::hex(0x62a338)
                }
            } else if depth < near_block * 2 {
                if speckle < 0.18 {
                    Rgb::hex(0x5c3b22)
                } else if speckle > 0.88 {
                    Rgb::hex(0x946a44)
                } else {
                    Rgb::hex(0x7a5230)
                }
            } else if speckle > 0.8 {
                Rgb::hex(0x858585)
            } else {
                Rgb::hex(0x6c6c6c)
            };
            grid.put(x, y, base.tint(light));
        }

        if submerged {
            for y in water_line.max(0)..top {
                let surface = y == water_line;
                let base = if surface {
                    Rgb::hex(0x6a9ce6)
                } else {
                    Rgb::hex(0x2d5fb4)
                };
                let mut color = base.tint(light).lerp(sky_low, 0.28);
                let glitter =
                    (x - sun_x).abs() <= half + 1 && hash2(x, y, (age * 5.) as u32) > 0.55;
                if glitter && sun_up > 0. {
                    color = color.lerp(Rgb::hex(0xffe2a8), 0.75 * sun_up);
                }
                grid.put(x, y, color);
            }
        } else if hash1(wx, 16) > 0.92 {
            // Grass tuft or flower on the cap.
            let flower = match hash1(wx, 17) {
                v if v > 0.7 => Rgb::hex(0xd8402e),
                v if v > 0.45 => Rgb::hex(0xf0d03a),
                _ => Rgb::hex(0x86c64e),
            };
            grid.put(x, top - 1, flower.tint(light));
        }
    }
}

struct CloudLayer {
    row: i32,
    thickness: i32,
    cell: i32,
    scroll: f32,
    seed: u32,
    opacity: f32,
}

impl CloudLayer {
    /// Flat cloud slabs whose lengths snap to a coarse cell grid.
    fn draw(&self, grid: &mut PixelGrid, top: Rgb, under: Rgb) {
        let scroll = self.scroll as i32;
        for x in 0..grid.width() as i32 {
            let cell = (x + scroll).div_euclid(self.cell);
            if value1(cell as f32 * 0.45, self.seed) < 0.55 {
                continue;
            }
            for y in self.row..self.row + self.thickness {
                let color = if y == self.row + self.thickness - 1 && self.thickness > 1 {
                    under
                } else {
                    top
                };
                grid.blend(x, y, color, self.opacity);
            }
        }
    }
}

