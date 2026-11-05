//! Vendored from EdwinZhanCN/LumilioCL v0.2.3 (AGPL-3.0), crates/lumilio-ui/src/hero/raster.rs. Changes: module paths; added `packed()` (flat pixel buffer for IPC).
//! A small CPU texel grid that the hero scenes draw into.
//!
//! The grid is deliberately GPUI-free: scenes write linear colours per texel,
//! and the view later turns each row into runs of identical packed colours so
//! a whole scene is painted with a few thousand quads.

use super::noise::{bayer4, smoothstep};

/// A linear RGB colour with components in `[0, 1]` (values may overshoot while
/// compositing; they are clamped when packed).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rgb {
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

impl Rgb {
    pub const BLACK: Self = Self::new(0., 0., 0.);

    pub const fn new(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b }
    }

    /// `0xRRGGBB` → colour.
    pub const fn hex(value: u32) -> Self {
        Self {
            r: ((value >> 16) & 0xff) as f32 / 255.,
            g: ((value >> 8) & 0xff) as f32 / 255.,
            b: (value & 0xff) as f32 / 255.,
        }
    }

    pub fn lerp(self, other: Self, t: f32) -> Self {
        Self {
            r: self.r + (other.r - self.r) * t,
            g: self.g + (other.g - self.g) * t,
            b: self.b + (other.b - self.b) * t,
        }
    }

    pub fn scale(self, factor: f32) -> Self {
        Self {
            r: self.r * factor,
            g: self.g * factor,
            b: self.b * factor,
        }
    }

    pub fn tint(self, light: Self) -> Self {
        Self {
            r: self.r * light.r,
            g: self.g * light.g,
            b: self.b * light.b,
        }
    }

    pub fn add(self, other: Self) -> Self {
        Self {
            r: self.r + other.r,
            g: self.g + other.g,
            b: self.b + other.b,
        }
    }

    pub fn luma(self) -> f32 {
        self.r * 0.2126 + self.g * 0.7152 + self.b * 0.0722
    }

    /// Packs to `0xRRGGBB` after clamping.
    pub fn pack(self) -> u32 {
        let channel = |value: f32| (value.clamp(0., 1.) * 255. + 0.5) as u32;
        (channel(self.r) << 16) | (channel(self.g) << 8) | channel(self.b)
    }
}

/// A horizontal span of identically coloured texels in one row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Run {
    pub row: usize,
    pub start: usize,
    pub len: usize,
    pub color: u32,
}

/// A run stretched down over `rows` consecutive identical rows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rect {
    pub row: usize,
    pub rows: usize,
    pub start: usize,
    pub len: usize,
    pub color: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PixelGrid {
    width: usize,
    height: usize,
    texels: Vec<Rgb>,
}

impl PixelGrid {
    pub fn new(width: usize, height: usize) -> Self {
        let width = width.max(1);
        let height = height.max(1);
        Self {
            width,
            height,
            texels: vec![Rgb::BLACK; width * height],
        }
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    /// Row-major packed `0xRRGGBB` texels (one per cell), for shipping the
    /// frame as a flat pixel buffer instead of thousands of dither-fragmented
    /// rectangles.
    pub fn packed(&self) -> Vec<u32> {
        self.texels.iter().map(|texel| texel.pack()).collect()
    }

    pub fn get(&self, x: usize, y: usize) -> Rgb {
        self.texels[y * self.width + x]
    }

    pub fn set(&mut self, x: usize, y: usize, color: Rgb) {
        self.texels[y * self.width + x] = color;
    }

    /// Signed, bounds-checked write for sprites and particles.
    pub fn put(&mut self, x: i32, y: i32, color: Rgb) {
        if x >= 0 && y >= 0 && (x as usize) < self.width && (y as usize) < self.height {
            self.set(x as usize, y as usize, color);
        }
    }

    /// Signed, bounds-checked alpha blend.
    pub fn blend(&mut self, x: i32, y: i32, color: Rgb, alpha: f32) {
        if x >= 0 && y >= 0 && (x as usize) < self.width && (y as usize) < self.height {
            let (x, y) = (x as usize, y as usize);
            let base = self.get(x, y);
            self.set(x, y, base.lerp(color, alpha.clamp(0., 1.)));
        }
    }

    /// Signed, bounds-checked additive light.
    pub fn glow(&mut self, x: i32, y: i32, color: Rgb) {
        if x >= 0 && y >= 0 && (x as usize) < self.width && (y as usize) < self.height {
            let (x, y) = (x as usize, y as usize);
            let base = self.get(x, y);
            self.set(x, y, base.add(color));
        }
    }

    pub fn fill_rect(&mut self, x: i32, y: i32, width: i32, height: i32, color: Rgb) {
        for row in y..y + height {
            for column in x..x + width {
                self.put(column, row, color);
            }
        }
    }

    /// Paints the whole grid from a per-texel function.
    pub fn shade(&mut self, mut shader: impl FnMut(usize, usize) -> Rgb) {
        for y in 0..self.height {
            for x in 0..self.width {
                self.texels[y * self.width + x] = shader(x, y);
            }
        }
    }

    /// Darkens the lower-left corner in dithered bands so overlaid copy stays
    /// legible without a smooth, non-pixel gradient.
    pub fn apply_scrim(&mut self, strength: f32) {
        let (width, height) = (self.width as f32, self.height as f32);
        for y in 0..self.height {
            for x in 0..self.width {
                let nx = x as f32 / width;
                let ny = y as f32 / height;
                let reach = smoothstep(0.62, 0.0, nx) * smoothstep(0.25, 1.0, ny);
                let bottom = smoothstep(0.55, 1.0, ny) * 0.45;
                let amount = (reach.max(bottom) * 8. + bayer4(x as i32, y as i32) - 0.5)
                    .floor()
                    .clamp(0., 8.)
                    / 8.;
                if amount > 0. {
                    let texel = self.get(x, y);
                    self.set(x, y, texel.scale(1. - amount * strength));
                }
            }
        }
    }

    /// Grows the grid to `height` rows by repeating its last row, so a scene
    /// composed for a shorter frame keeps its layout when the frame grows.
    pub fn extend_to(&mut self, height: usize) {
        if height <= self.height {
            return;
        }
        let last = self.texels[(self.height - 1) * self.width..].to_vec();
        for _ in self.height..height {
            self.texels.extend_from_slice(&last);
        }
        self.height = height;
    }

    /// Dissolves the bottom `rows` into the page colour with an ordered
    /// dither: the world breaks up into the page texel by texel and the last
    /// row is exactly the page, so art and page meet without a seam. Texels
    /// that survive are pulled toward the page first, so a dark world over a
    /// light page never becomes a harsh checkerboard.
    pub fn fade_bottom(&mut self, rows: usize, page: Rgb) {
        let rows = rows.min(self.height);
        for band in 0..rows {
            let y = self.height - rows + band;
            let t = (band + 1) as f32 / rows as f32;
            for x in 0..self.width {
                let texel = if bayer4(x as i32, y as i32) < smoothstep(0., 1., t) {
                    page
                } else {
                    self.get(x, y).lerp(page, t * 0.65)
                };
                self.set(x, y, texel);
            }
        }
    }

    /// The same dissolve as `fade_bottom`, turned to the left edge: the first
    /// column is exactly the page and the art thickens toward the right.
    pub fn fade_left(&mut self, columns: usize, page: Rgb) {
        let columns = columns.min(self.width);
        for band in 0..columns {
            let t = 1. - band as f32 / columns as f32;
            for y in 0..self.height {
                let texel = if bayer4(band as i32, y as i32) < smoothstep(0., 1., t) {
                    page
                } else {
                    self.get(band, y).lerp(page, t * 0.65)
                };
                self.set(band, y, texel);
            }
        }
    }

    /// Darkens and slightly desaturates the whole grid, for frames that sit
    /// behind foreground content while nothing is happening in the world.
    pub fn dim(&mut self, brightness: f32) {
        for texel in &mut self.texels {
            let grey = texel.luma();
            *texel = texel
                .lerp(Rgb::new(grey, grey, grey), 0.35)
                .scale(brightness);
        }
    }

    /// Row-major runs of identical packed colours.
    pub fn runs(&self) -> Vec<Run> {
        let mut runs = Vec::with_capacity(self.height * 16);
        for row in 0..self.height {
            let mut current: Option<Run> = None;
            for column in 0..self.width {
                let color = self.get(column, row).pack();
                match current.as_mut() {
                    Some(run) if run.color == color => run.len += 1,
                    _ => {
                        if let Some(run) = current.replace(Run {
                            row,
                            start: column,
                            len: 1,
                            color,
                        }) {
                            runs.push(run);
                        }
                    }
                }
            }
            if let Some(run) = current {
                runs.push(run);
            }
        }
        runs
    }

    /// Runs merged vertically where consecutive rows repeat the exact same
    /// span, so flat regions (sky bands, dark rock) cost a single quad.
    pub fn rects(&self) -> Vec<Rect> {
        let mut rects: Vec<Rect> = Vec::new();
        // Rects still open from the previous row, sorted by start column.
        let mut open: Vec<usize> = Vec::new();
        let mut next: Vec<usize> = Vec::new();
        let runs = self.runs();
        let mut runs = runs.iter().peekable();
        for row in 0..self.height {
            next.clear();
            let mut candidate = open.iter().copied().peekable();
            while let Some(run) = runs.next_if(|run| run.row == row) {
                // Both lists are ordered by start, so a single forward walk
                // finds the rect directly above this run, if any.
                while candidate.next_if(|&i| rects[i].start < run.start).is_some() {}
                let above = candidate.next_if(|&i| {
                    let rect = rects[i];
                    rect.start == run.start && rect.len == run.len && rect.color == run.color
                });
                let index = match above {
                    Some(index) => {
                        rects[index].rows += 1;
                        index
                    }
                    None => {
                        rects.push(Rect {
                            row,
                            rows: 1,
                            start: run.start,
                            len: run.len,
                            color: run.color,
                        });
                        rects.len() - 1
                    }
                };
                next.push(index);
            }
            std::mem::swap(&mut open, &mut next);
        }
        rects
    }
}

