//! Vendored from EdwinZhanCN/LumilioCL v0.2.3 (AGPL-3.0), crates/lumilio-ui/src/hero/noise.rs. Changes: module paths only.
//! Deterministic hashing and value noise for the hero scenes.
//!
//! Everything is a pure function of integer lattice coordinates and a seed, so
//! a scene rendered twice at the same clock produces identical texels.

/// Uniform value in `[0, 1)` for a 2D lattice point.
pub fn hash2(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32)
        .wrapping_mul(0x8da6_b343)
        .wrapping_add((y as u32).wrapping_mul(0xd816_3841))
        .wrapping_add(seed.wrapping_mul(0xcb1a_b31f));
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297a_2d39);
    h ^= h >> 15;
    (h >> 8) as f32 / (1u32 << 24) as f32
}

/// Uniform value in `[0, 1)` for a 1D lattice point.
pub fn hash1(x: i32, seed: u32) -> f32 {
    hash2(x, 0x5bd1_e995_u32 as i32, seed)
}

/// Smoothly interpolated 1D value noise in `[0, 1)`.
pub fn value1(x: f32, seed: u32) -> f32 {
    let cell = x.floor();
    let t = smooth(x - cell);
    let i = cell as i32;
    lerp(hash1(i, seed), hash1(i + 1, seed), t)
}

/// Smoothly interpolated 2D value noise in `[0, 1)`.
pub fn value2(x: f32, y: f32, seed: u32) -> f32 {
    let (cx, cy) = (x.floor(), y.floor());
    let (tx, ty) = (smooth(x - cx), smooth(y - cy));
    let (ix, iy) = (cx as i32, cy as i32);
    let top = lerp(hash2(ix, iy, seed), hash2(ix + 1, iy, seed), tx);
    let bottom = lerp(hash2(ix, iy + 1, seed), hash2(ix + 1, iy + 1, seed), tx);
    lerp(top, bottom, ty)
}

/// Two-octave fractal value noise, normalised back into `[0, 1)`.
pub fn fbm2(x: f32, y: f32, seed: u32) -> f32 {
    (value2(x, y, seed) * 2. + value2(x * 2.1, y * 2.1, seed ^ 0x9e37)) / 3.
}

/// A 4×4 ordered-dither threshold in `[0, 1)`; used to band gradients the way
/// low-colour pixel art does instead of rendering smooth ramps.
pub fn bayer4(x: i32, y: i32) -> f32 {
    const MATRIX: [u8; 16] = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5];
    let index = (y.rem_euclid(4) * 4 + x.rem_euclid(4)) as usize;
    (MATRIX[index] as f32 + 0.5) / 16.
}

pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

pub fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0., 1.);
    smooth(t)
}

fn smooth(t: f32) -> f32 {
    t * t * (3. - 2. * t)
}

