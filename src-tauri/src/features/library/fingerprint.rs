//! CurseForge fingerprint calculation.

// ---------------- modIconIdentity.ts：CurseForge 指纹（MurmurHash2, seed 1, 去空白） ----------------

/// curseFingerprint()
pub fn curse_fingerprint(input: &[u8]) -> u32 {
    let mut length: u32 = 0;
    for &b in input {
        if !matches!(b, 9 | 10 | 13 | 32) {
            length = length.wrapping_add(1);
        }
    }
    const M: u32 = 0x5bd1e995;
    let mut h = 1u32.wrapping_sub(1).wrapping_add(length); // (1 ^ length) >>> 0
    let mut word: u32 = 0;
    let mut shift: u32 = 0;
    for &byte in input {
        if matches!(byte, 9 | 10 | 13 | 32) {
            continue;
        }
        word |= (byte as u32) << shift;
        if shift == 24 {
            word = word.wrapping_mul(M);
            word ^= word >> 24;
            word = word.wrapping_mul(M);
            h = h.wrapping_mul(M) ^ word;
            word = 0;
            shift = 0;
        } else {
            shift += 8;
        }
    }
    if shift != 0 {
        h ^= word;
        h = h.wrapping_mul(M);
    }
    h ^= h >> 13;
    h = h.wrapping_mul(M);
    h ^= h >> 15;
    h
}
