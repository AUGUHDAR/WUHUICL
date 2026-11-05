//! Vendored from EdwinZhanCN/LumilioCL v0.2.3 (AGPL-3.0), crates/lumilio-ui/src/hero/scenes/redstone.rs. Changes: module paths only.
//! 红石 · Signal strength — a top-down circuit with two tracks of equal
//! length fed by one lever. Dust power decays one level per block, so the
//! upper track fades to zero and its lamp stays dark; the lower track passes a
//! repeater, is restored to 15, and lights its lamp. The signal visibly
//! travels as a wave when the lever flips, then recedes when it flips back.

use crate::hero_scenes::noise::hash2;
use crate::hero_scenes::raster::{PixelGrid, Rgb};

pub const BOARD_COLUMNS: i32 = 17;
pub const BOARD_ROWS: i32 = 9;
pub const MAX_POWER: u8 = 15;
/// Presentation delay per dust block so the signal reads as a travelling wave.
const STEP_SECONDS: f32 = 1. / 18.;
const REPEATER_SECONDS: f32 = 0.2;
const LEVER_FIRST_ON: f32 = 0.9;
const LEVER_PERIOD: f32 = 6.;
const LEVER_ON_SPAN: f32 = 4.2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Part {
    Lever,
    Dust,
    /// Faces east: input from the west, output to the east.
    Repeater,
    Lamp,
}

pub fn board() -> Vec<((i32, i32), Part)> {
    let mut parts = vec![
        ((0, 4), Part::Lever),
        ((1, 4), Part::Dust),
        ((2, 4), Part::Dust),
        ((5, 7), Part::Repeater),
        ((16, 1), Part::Lamp),
        ((16, 7), Part::Lamp),
    ];
    parts.extend([1, 2, 3, 5, 6, 7].map(|r| ((2, r), Part::Dust)));
    parts.extend((3..=15).map(|c| ((c, 1), Part::Dust)));
    parts.extend((3..=4).chain(6..=15).map(|c| ((c, 7), Part::Dust)));
    parts
}

/// Steady-state power with the lever on, and the delay after which each cell
/// sees the change. Unpowered-but-connected dust reports power 0.
#[derive(Clone, Debug)]
pub struct Circuit {
    parts: Vec<((i32, i32), Part)>,
    power: Vec<u8>,
    delay: Vec<f32>,
}

impl Circuit {
    pub fn solve() -> Self {
        let parts = board();
        let find = |cell: (i32, i32)| parts.iter().position(|(at, _)| *at == cell);
        let mut power = vec![0u8; parts.len()];
        let mut delay = vec![f32::INFINITY; parts.len()];
        for (index, (_, part)) in parts.iter().enumerate() {
            if *part == Part::Lever {
                power[index] = MAX_POWER;
                delay[index] = 0.;
            }
        }

        // Relax to a fixpoint; the board is tiny.
        loop {
            let mut changed = false;
            for index in 0..parts.len() {
                let ((c, r), part) = parts[index];
                let candidate = match part {
                    Part::Lever => continue,
                    Part::Dust | Part::Lamp => [(1, 0), (-1, 0), (0, 1), (0, -1)]
                        .into_iter()
                        .filter_map(|(dc, dr)| find((c + dc, r + dr)))
                        .filter_map(|n| {
                            let ((nc, _), neighbour) = parts[n];
                            let source = match neighbour {
                                Part::Lever => Some(MAX_POWER),
                                Part::Dust => power[n].checked_sub(1),
                                // A repeater only drives the block in front of it.
                                Part::Repeater if nc + 1 == c => {
                                    (power[n] > 0).then_some(MAX_POWER)
                                }
                                _ => None,
                            }?;
                            let step = if neighbour == Part::Repeater {
                                0.
                            } else {
                                STEP_SECONDS
                            };
                            (source > 0).then_some((source, delay[n] + step))
                        })
                        .max_by(|a, b| a.0.cmp(&b.0).then(b.1.total_cmp(&a.1))),
                    Part::Repeater => find((c - 1, r))
                        .filter(|&n| power[n] > 0)
                        .map(|n| (1, delay[n] + REPEATER_SECONDS)),
                };
                if let Some((level, at)) = candidate {
                    if level > power[index] || (level == power[index] && at < delay[index]) {
                        power[index] = level;
                        delay[index] = at;
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }

        Self {
            parts,
            power,
            delay,
        }
    }

    pub fn at(&self, cell: (i32, i32)) -> Option<(Part, u8)> {
        let index = self.parts.iter().position(|(at, _)| *at == cell)?;
        Some((self.parts[index].1, self.power[index]))
    }

    /// Live power at a moment: the steady-state level if the lever was on
    /// when the wave left it, otherwise zero.
    fn live(&self, index: usize, age: f32) -> u8 {
        let delay = self.delay[index];
        if delay.is_finite() && lever_on(age - delay) {
            self.power[index]
        } else {
            0
        }
    }
}

pub fn lever_on(age: f32) -> bool {
    age >= LEVER_FIRST_ON && (age - LEVER_FIRST_ON).rem_euclid(LEVER_PERIOD) < LEVER_ON_SPAN
}

fn dust_color(level: u8) -> Rgb {
    if level == 0 {
        Rgb::hex(0x4a0c08)
    } else {
        let t = (level as f32 / MAX_POWER as f32).powf(0.8);
        Rgb::hex(0x7a1408).lerp(Rgb::hex(0xff3e1e), t)
    }
}

pub fn render(grid: &mut PixelGrid, age: f32) {
    let (w, h) = (grid.width() as i32, grid.height() as i32);
    let circuit = Circuit::solve();
    let b = ((w as f32 * 0.7 / BOARD_COLUMNS as f32).min(h as f32 * 0.9 / BOARD_ROWS as f32)
        as i32)
        .max(3);
    let ox = w - (BOARD_COLUMNS + 1) * b;
    let oy = (h - BOARD_ROWS * b) / 2;
    let live: Vec<u8> = (0..circuit.parts.len())
        .map(|index| circuit.live(index, age))
        .collect();
    let lamp_lit = |index: usize| live[index] > 0;

    // Floor: dark polished tiles with per-tile tone and seams.
    grid.shade(|x, y| {
        let (lx, ly) = (x as i32 - ox, y as i32 - oy);
        let (bx, by) = (lx.div_euclid(b), ly.div_euclid(b));
        let (u, v) = (lx.rem_euclid(b), ly.rem_euclid(b));
        let tone = 1. + (hash2(bx, by, 51) - 0.5) * 0.1;
        let base = if u == 0 || v == 0 {
            Rgb::hex(0x1b1b21)
        } else if hash2(x as i32, y as i32, 52) > 0.9 {
            Rgb::hex(0x34343c)
        } else {
            Rgb::hex(0x27272e)
        };
        base.scale(tone)
    });

    // Glow first so parts draw on top: dust tints its tile, lit lamps warm a
    // radius of tiles in block-sized steps.
    for (index, &((c, r), part)) in circuit.parts.iter().enumerate() {
        let (x0, y0) = (ox + c * b, oy + r * b);
        match part {
            Part::Dust if live[index] > 0 => {
                let glow = Rgb::hex(0xff2a10).scale(0.07 * live[index] as f32 / 15.);
                for y in y0..y0 + b {
                    for x in x0..x0 + b {
                        grid.glow(x, y, glow);
                    }
                }
            }
            Part::Lamp if lamp_lit(index) => {
                for dr in -3..=3i32 {
                    for dc in -3..=3i32 {
                        let reach = dc.abs().max(dr.abs());
                        if reach == 0 {
                            continue;
                        }
                        let glow = Rgb::hex(0xffb050).scale(0.16 * (1. - reach as f32 / 4.));
                        let (tx, ty) = (x0 + dc * b, y0 + dr * b);
                        for y in ty..ty + b {
                            for x in tx..tx + b {
                                grid.glow(x, y, glow);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    let thickness = ((b + 1) / 3).max(1);
    let lane = (b - thickness) / 2;
    for (index, &((c, r), part)) in circuit.parts.iter().enumerate() {
        let (x0, y0) = (ox + c * b, oy + r * b);
        match part {
            Part::Dust => {
                let power = live[index];
                let color = dust_color(power);
                let arms: Vec<(i32, i32)> = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                    .into_iter()
                    .filter(|&(dc, dr)| circuit.at((c + dc, r + dr)).is_some())
                    .collect();
                let arm_rect = |(dc, dr): (i32, i32), pad: i32| match (dc, dr) {
                    (1, 0) => (
                        x0 + lane - pad,
                        y0 + lane - pad,
                        b - lane + pad,
                        thickness + 2 * pad,
                    ),
                    (-1, 0) => (
                        x0,
                        y0 + lane - pad,
                        lane + thickness + pad,
                        thickness + 2 * pad,
                    ),
                    (0, 1) => (
                        x0 + lane - pad,
                        y0 + lane - pad,
                        thickness + 2 * pad,
                        b - lane + pad,
                    ),
                    _ => (
                        x0 + lane - pad,
                        y0,
                        thickness + 2 * pad,
                        lane + thickness + pad,
                    ),
                };
                // A one-texel halo hugging powered dust.
                if power > 0 {
                    let halo = Rgb::hex(0xff3a14).scale(0.16 * power as f32 / 15.);
                    let mut texels = std::collections::HashSet::new();
                    for &arm in &arms {
                        let (x, y, rw, rh) = arm_rect(arm, 1);
                        for ty in y..y + rh {
                            for tx in x..x + rw {
                                texels.insert((tx, ty));
                            }
                        }
                    }
                    for (tx, ty) in texels {
                        grid.glow(tx, ty, halo);
                    }
                }
                grid.fill_rect(x0 + lane, y0 + lane, thickness, thickness, color);
                for &arm in &arms {
                    let (x, y, rw, rh) = arm_rect(arm, 0);
                    grid.fill_rect(x, y, rw, rh, color);
                }
                // Sparks drifting off powered dust.
                let slot = (age * 6. + hash2(c, r, 53) * 6.) as i32;
                if live[index] > 0 && hash2(c * 31 + slot, r, 54) < 0.28 * live[index] as f32 / 15.
                {
                    let sx = x0 + (hash2(c, slot, 55) * b as f32) as i32;
                    let sy = y0 + (hash2(r, slot, 56) * b as f32) as i32;
                    grid.put(sx, sy, Rgb::hex(0xff8a6a));
                }
            }
            Part::Repeater => {
                grid.fill_rect(x0, y0, b, b, Rgb::hex(0x7c7c80));
                grid.fill_rect(x0 + 1, y0 + 1, b - 2, b - 2, Rgb::hex(0xa9a9ad));
                let torch = if live[index] > 0 {
                    Rgb::hex(0xff4a28)
                } else {
                    Rgb::hex(0x5a1a14)
                };
                grid.fill_rect(
                    x0 + lane,
                    y0 + lane,
                    b - 2 * lane,
                    thickness,
                    Rgb::hex(0x8a8a90),
                );
                grid.fill_rect(x0 + 1, y0 + lane, thickness, thickness, torch);
                grid.fill_rect(
                    x0 + b - 1 - thickness,
                    y0 + lane,
                    thickness,
                    thickness,
                    torch,
                );
            }
            Part::Lever => {
                grid.fill_rect(x0 + 1, y0 + 1, b - 2, b - 2, Rgb::hex(0x6e6e6e));
                for v in 1..b - 1 {
                    for u in 1..b - 1 {
                        if hash2(x0 + u, y0 + v, 57) > 0.7 {
                            grid.put(x0 + u, y0 + v, Rgb::hex(0x585858));
                        }
                    }
                }
                let on = lever_on(age);
                let (cx, cy) = (x0 + b / 2, y0 + b / 2);
                let direction = if on { 1 } else { -1 };
                for step in 0..=b / 2 {
                    grid.put(cx + direction * step / 2, cy - step, Rgb::hex(0x8a6a3a));
                }
                grid.put(cx + direction * (b / 4), cy - b / 2, Rgb::hex(0x4a3420));
            }
            Part::Lamp => {
                let lit = lamp_lit(index);
                let (frame, bright, dark) = if lit {
                    (0x9a6a3a, 0xffeab0, 0xf2a848)
                } else {
                    (0x4a3020, 0x6a4a2c, 0x3a2616)
                };
                grid.fill_rect(x0, y0, b, b, Rgb::hex(frame));
                for v in 1..b - 1 {
                    for u in 1..b - 1 {
                        let lattice = (u + v) % 2 == 0;
                        grid.put(
                            x0 + u,
                            y0 + v,
                            Rgb::hex(if lattice { bright } else { dark }),
                        );
                    }
                }
            }
        }
    }
}

/// HUD readout: the lever's output strength right now.
pub fn signal(age: f32) -> u8 {
    if lever_on(age) { MAX_POWER } else { 0 }
}

