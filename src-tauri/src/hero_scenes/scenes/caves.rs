//! Vendored from EdwinZhanCN/LumilioCL v0.2.3 (AGPL-3.0), crates/lumilio-ui/src/hero/scenes/caves.rs.
//! Changes: module paths; added `Cave::cached` (per-size world cache — render runs
//! every animation frame, world generation only depends on the grid size).
//! 洞穴 · Light levels — a cross-section in which a tunnel is mined block by
//! block (ten crack stages, selection outline, debris), torches are placed as
//! it advances, and block light floods through open air losing exactly one
//! level per block. The tunnel finally breaks into a lava cave on the right.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};

use crate::hero_scenes::noise::{fbm2, hash2, smoothstep, value2};
use crate::hero_scenes::raster::{PixelGrid, Rgb};

pub const TORCH_LEVEL: f32 = 14.;
pub const LAVA_LEVEL: f32 = 15.;
const MINE_START: f32 = 0.7;
/// The tunnel breaks into the lava cave at this age regardless of aspect ratio.
const BREAKTHROUGH: f32 = 8.4;
const TORCH_RAMP: f32 = 0.35;
const TORCH_SPACING: i32 = 5;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Ore {
    Coal,
    Copper,
    Iron,
    Lapis,
    Redstone,
    Diamond,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Cell {
    Stone { deep: bool, ore: Option<Ore> },
    Air,
    Lava,
}

impl Cell {
    fn is_open(self) -> bool {
        matches!(self, Self::Air | Self::Lava)
    }
}

/// The static world plus the mining schedule that opens it over time.
pub struct Cave {
    pub block: i32,
    pub columns: i32,
    pub rows: i32,
    cells: Vec<Cell>,
    /// Blocks mined in order, as (column, row).
    pub path: Vec<(i32, i32)>,
    mine_seconds: f32,
    /// Per cell: when it is mined out (infinite for cells off the path).
    mined: Vec<f32>,
    /// Torches as (column, row, placed_at).
    pub torches: Vec<(i32, i32, f32)>,
}

impl Cave {
    /// 按尺寸缓存的世界：Cave::new 生成细胞、矿道与火把表，结果只依赖 (w,h)，
    /// 而渲染每帧都会调——40fps 下不该重复生成。光传播（light()）仍逐帧算，
    /// 因为它随挖掘进度实时变化。
    pub fn cached(width: i32, height: i32) -> Arc<Self> {
        static CACHE: OnceLock<Mutex<HashMap<(i32, i32), Arc<Cave>>>> = OnceLock::new();
        let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
        let mut guard = cache.lock().unwrap();
        if let Some(hit) = guard.get(&(width, height)) {
            return hit.clone();
        }
        let cave = Arc::new(Cave::new(width, height));
        guard.insert((width, height), cave.clone());
        cave
    }

    pub fn new(width: i32, height: i32) -> Self {
        let block = (height / 11).clamp(4, 9);
        let columns = (width + block - 1) / block;
        let rows = (height + block - 1) / block;
        let upper = (rows as f32 * 0.42) as i32;
        let lower = upper + 1;
        let deep_row = (rows as f32 * 0.66) as i32;

        let chamber = |c: i32, r: i32| {
            let dx = (c as f32 - columns as f32 * 0.83) / (columns as f32 * 0.17);
            let dy = (r as f32 - rows as f32 * 0.72) / (rows as f32 * 0.34);
            dx * dx + dy * dy
        };
        let lavafall = (columns as f32 * 0.9) as i32;
        let pocket = |c: i32, r: i32| {
            let dx = (c as f32 - 2.2) / 2.6;
            let dy = (r as f32 - upper as f32 - 0.5) / 1.9;
            dx * dx + dy * dy < 1.
        };

        let mut path = Vec::new();
        let mut column = 4;
        while column < columns && chamber(column, upper) >= 1. && chamber(column, lower) >= 1. {
            path.push((column, upper));
            path.push((column, lower));
            column += 1;
        }

        let mut cells = Vec::with_capacity((columns * rows) as usize);
        for r in 0..rows {
            for c in 0..columns {
                let on_path = path.contains(&(c, r));
                let cell = if chamber(c, r) < 1. {
                    if r >= rows - 2 || c == lavafall {
                        Cell::Lava
                    } else {
                        Cell::Air
                    }
                } else if pocket(c, r)
                    || (!on_path && r > 0 && fbm2(c as f32 * 0.21, r as f32 * 0.34, 31) > 0.66)
                {
                    Cell::Air
                } else {
                    let deep = r >= deep_row;
                    let roll = hash2(c, r, 11);
                    let ore = if roll > 0.975 {
                        Some(if deep { Ore::Diamond } else { Ore::Iron })
                    } else if roll > 0.95 {
                        Some(if deep { Ore::Redstone } else { Ore::Copper })
                    } else if roll > 0.91 {
                        Some(if deep { Ore::Lapis } else { Ore::Coal })
                    } else {
                        None
                    };
                    Cell::Stone { deep, ore }
                };
                cells.push(cell);
            }
        }

        let mut cave = Self {
            block,
            columns,
            rows,
            cells,
            mine_seconds: (BREAKTHROUGH - MINE_START) / path.len().max(1) as f32,
            mined: vec![f32::INFINITY; (columns * rows) as usize],
            path,
            torches: vec![(2, lower, 0.)],
        };
        for index in 0..cave.path.len() {
            let (c, r) = cave.path[index];
            let cell = cave.index(c, r);
            cave.mined[cell] = cave.mined_at(index);
        }
        // A guaranteed diamond just under the tunnel floor, for the first torch to find.
        let showcase = (4 + TORCH_SPACING + 2, lower + 1);
        if cave
            .cell(showcase.0, showcase.1)
            .is_some_and(|c| !c.is_open())
        {
            let index = cave.index(showcase.0, showcase.1);
            cave.cells[index] = Cell::Stone {
                deep: false,
                ore: Some(Ore::Diamond),
            };
        }
        let placed: Vec<_> = cave
            .path
            .iter()
            .enumerate()
            .filter(|(_, (c, r))| *r == lower && (c - 4) % TORCH_SPACING == TORCH_SPACING - 3)
            .map(|(index, &(c, r))| (c, r, cave.mined_at(index) + 0.12))
            .collect();
        cave.torches.extend(placed);
        cave
    }

    fn index(&self, c: i32, r: i32) -> usize {
        (r * self.columns + c) as usize
    }

    pub fn cell(&self, c: i32, r: i32) -> Option<Cell> {
        (c >= 0 && r >= 0 && c < self.columns && r < self.rows)
            .then(|| self.cells[self.index(c, r)])
    }

    fn starts_at(&self, index: usize) -> f32 {
        MINE_START + index as f32 * self.mine_seconds
    }

    pub fn mined_at(&self, index: usize) -> f32 {
        self.starts_at(index + 1)
    }

    /// The world at a moment: mined path blocks have become air.
    pub fn cell_at(&self, c: i32, r: i32, age: f32) -> Option<Cell> {
        let cell = self.cell(c, r)?;
        if age >= self.mined[self.index(c, r)] {
            Some(Cell::Air)
        } else {
            Some(cell)
        }
    }

    /// Block light per cell: every source floods through open cells and loses
    /// one level per block travelled. Solid faces take the brightest neighbour.
    /// Returns (torch light, lava light).
    pub fn light(&self, age: f32) -> (Vec<f32>, Vec<f32>) {
        let size = (self.columns * self.rows) as usize;
        let mut torch = vec![0f32; size];
        let mut lava = vec![0f32; size];

        let lava_sources: Vec<_> = (0..self.rows)
            .flat_map(|r| (0..self.columns).map(move |c| (c, r)))
            .filter(|&(c, r)| self.cell(c, r) == Some(Cell::Lava))
            .collect();
        self.flood(&lava_sources, LAVA_LEVEL, age, &mut lava);
        for &(c, r, placed) in &self.torches {
            let level = TORCH_LEVEL * smoothstep(placed, placed + TORCH_RAMP, age);
            if level > 0. {
                self.flood(&[(c, r)], level, age, &mut torch);
            }
        }
        (self.lit_faces(&torch, age), self.lit_faces(&lava, age))
    }

    fn flood(&self, sources: &[(i32, i32)], level: f32, age: f32, field: &mut [f32]) {
        let mut distance = vec![u16::MAX; field.len()];
        let mut queue = VecDeque::new();
        for &(c, r) in sources {
            distance[self.index(c, r)] = 0;
            queue.push_back((c, r));
        }
        while let Some((c, r)) = queue.pop_front() {
            let here = distance[self.index(c, r)];
            let value = level - here as f32;
            if value <= 0. {
                continue;
            }
            let slot = &mut field[self.index(c, r)];
            *slot = slot.max(value);
            for (dc, dr) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let (nc, nr) = (c + dc, r + dr);
                if self.cell_at(nc, nr, age).is_some_and(Cell::is_open) {
                    let next = self.index(nc, nr);
                    if distance[next] == u16::MAX {
                        distance[next] = here + 1;
                        queue.push_back((nc, nr));
                    }
                }
            }
        }
    }

    fn lit_faces(&self, field: &[f32], age: f32) -> Vec<f32> {
        let mut faces = field.to_vec();
        for r in 0..self.rows {
            for c in 0..self.columns {
                if self.cell_at(c, r, age).is_some_and(Cell::is_open) {
                    continue;
                }
                let brightest = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                    .into_iter()
                    .filter(|(dc, dr)| self.cell_at(c + dc, r + dr, age).is_some_and(Cell::is_open))
                    .map(|(dc, dr)| field[self.index(c + dc, r + dr)])
                    .fold(0f32, f32::max);
                faces[self.index(c, r)] = brightest;
            }
        }
        faces
    }

    /// Index of the block being mined at this moment, with its progress.
    pub fn mining(&self, age: f32) -> Option<(usize, f32)> {
        let index = ((age - MINE_START) / self.mine_seconds).floor();
        if index < 0. || index as usize >= self.path.len() {
            return None;
        }
        let index = index as usize;
        Some((index, (age - self.starts_at(index)) / self.mine_seconds))
    }

    pub fn torches_lit(&self, age: f32) -> usize {
        self.torches.iter().filter(|t| age >= t.2).count()
    }
}

/// Perceived brightness of a light level, with the game's characteristic
/// curve: bright near the source and falling off steeply into darkness.
pub fn brightness(level: f32) -> f32 {
    let f = (level / 15.).clamp(0., 1.);
    f / (4. - 3. * f)
}

/// What the slide shows: the same curve with a raised gamma and a faint cool
/// ambient so unlit rock still reads as rock rather than a black void.
fn exposure(level: f32) -> f32 {
    0.2 + 0.8 * brightness(level).powf(0.6)
}

const AMBIENT: Rgb = Rgb::new(0.5, 0.58, 0.85);

fn ore_color(ore: Ore) -> Rgb {
    Rgb::hex(match ore {
        Ore::Coal => 0x262626,
        Ore::Copper => 0xe0784a,
        Ore::Iron => 0xd9b095,
        Ore::Lapis => 0x2d55c8,
        Ore::Redstone => 0xf02818,
        Ore::Diamond => 0x62ece2,
    })
}

fn rock_texel(deep: bool, x: i32, y: i32) -> Rgb {
    let speckle = hash2(x, y, 3);
    if deep {
        let base = if y.rem_euclid(3) == 0 {
            Rgb::hex(0x42424a)
        } else {
            Rgb::hex(0x4e4e58)
        };
        if speckle > 0.86 {
            base.scale(1.18)
        } else {
            base
        }
    } else if speckle < 0.2 {
        Rgb::hex(0x6b6b6b)
    } else if speckle > 0.86 {
        Rgb::hex(0x939393)
    } else {
        Rgb::hex(0x7e7e7e)
    }
}

fn lava_texel(x: i32, y: i32, age: f32) -> Rgb {
    let flow = value2(x as f32 * 0.28 + age * 0.35, y as f32 * 0.4 - age * 0.5, 41);
    Rgb::hex(match (flow * 4.) as u32 {
        0 => 0xc2400e,
        1 => 0xe8661a,
        2 => 0xffa030,
        _ => 0xffd566,
    })
}

pub fn render(grid: &mut PixelGrid, age: f32) {
    let (w, h) = (grid.width() as i32, grid.height() as i32);
    let cave = Cave::cached(w, h);
    let b = cave.block;
    let (torch, lava) = cave.light(age);
    let torch_tint = Rgb::hex(0xffe0b0);
    let lava_tint = Rgb::hex(0xff9a58);
    let mining = cave.mining(age);

    for y in 0..h {
        for x in 0..w {
            let (c, r) = (x / b, y / b);
            let Some(cell) = cave.cell_at(c, r, age) else {
                continue;
            };
            let index = cave.index(c, r);
            let (t, l) = (torch[index], lava[index]);
            let open = cell.is_open();
            // Solid faces sit a little behind the air in front of them, so the
            // open tunnel reads as the brightest thing in the rock.
            let level = if open {
                t.max(l)
            } else {
                (t.max(l) - 2.).max(0.)
            };
            let warmth = if t + l > 0. { l / (t + l) } else { 0. };
            let tint = AMBIENT.lerp(
                torch_tint.lerp(lava_tint, warmth),
                smoothstep(0., 4., level),
            );
            let lit = |color: Rgb| color.tint(tint).scale(exposure(level));

            let color = match cell {
                Cell::Lava => lava_texel(x, y, age),
                Cell::Air => lit(rock_texel(r >= cave.rows * 2 / 3, x, y).scale(0.8)),
                Cell::Stone { deep, ore } => {
                    let (u, v) = (x % b, y % b);
                    let spot = u > 0 && v > 0 && u < b - 1 && v < b - 1;
                    let texel = match ore {
                        Some(ore) if spot && hash2(x, y, 17) > 0.72 => {
                            if ore == Ore::Diamond
                                && level > 5.
                                && hash2(c, r, (age * 3.) as u32) > 0.8
                                && (u + v) % 3 == 0
                            {
                                Rgb::hex(0xffffff)
                            } else {
                                ore_color(ore)
                            }
                        }
                        _ => rock_texel(deep, x, y),
                    };
                    lit(texel)
                }
            };
            grid.set(x as usize, y as usize, color);
        }
    }

    // Crack stages and the selection outline on the block being mined.
    if let Some((index, progress)) = mining {
        let (c, r) = cave.path[index];
        let stage = ((progress * 10.) as i32).min(9) + 1;
        let (x0, y0) = (c * b, r * b);
        let centre = (b - 1) as f32 / 2.;
        for v in 0..b {
            for u in 0..b {
                let (x, y) = (x0 + u, y0 + v);
                let spread = ((u as f32 - centre).abs() + (v as f32 - centre).abs()) / b as f32
                    * 0.9
                    + hash2(x, y, 23) * 0.35;
                if spread < stage as f32 / 10. * 1.15 && hash2(x, y, 29) > 0.4 {
                    grid.blend(x, y, Rgb::BLACK, 0.7);
                }
                if u == 0 || v == 0 || u == b - 1 || v == b - 1 {
                    grid.blend(x, y, Rgb::hex(0x101010), 0.75);
                }
            }
        }
    }

    // Debris from freshly mined blocks.
    for (index, &(c, r)) in cave.path.iter().enumerate() {
        let since = age - cave.mined_at(index);
        if !(0. ..0.65).contains(&since) {
            continue;
        }
        let base = match cave.cell(c, r) {
            Some(Cell::Stone { ore: Some(ore), .. }) => ore_color(ore),
            _ => Rgb::hex(0x8a8a8a),
        };
        let light = exposure(torch[cave.index(c, r)].max(lava[cave.index(c, r)]) + 3.);
        for particle in 0..7 {
            let seed = index as i32 * 13 + particle;
            let vx = (hash2(seed, 1, 43) - 0.5) * b as f32 * 3.;
            let vy = -(0.6 + hash2(seed, 2, 43)) * b as f32 * 2.4;
            let gravity = b as f32 * 14.;
            let px = (c * b) as f32 + b as f32 * 0.5 + vx * since;
            let py = (r * b) as f32 + b as f32 * 0.5 + vy * since + 0.5 * gravity * since * since;
            grid.put(px as i32, py as i32, base.scale(light));
        }
    }

    // Torch sprites, always full bright.
    for &(c, r, placed) in &cave.torches {
        if age < placed {
            continue;
        }
        let x = c * b + b / 2;
        let floor = r * b + b - 1;
        let stick = (b * 3 / 5).max(2);
        for i in 0..stick {
            grid.put(x, floor - i, Rgb::hex(0x7a5530));
        }
        let flicker = hash2(c, r, (age * 9.) as u32) > 0.5;
        grid.put(
            x,
            floor - stick,
            Rgb::hex(if flicker { 0xffd65a } else { 0xffb03a }),
        );
        grid.put(
            x,
            floor - stick - 1,
            Rgb::hex(if flicker { 0xff9a2e } else { 0xffe070 }),
        );
    }
}

