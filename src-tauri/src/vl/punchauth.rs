//! PunchAuth 帧签名（对应原版 voxlink/punchAuth.ts，LGPL-3.0-only，VoxLink PunchAuth.java 6b11d93）

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

pub fn derive_punch_key(code: &str, client_id: &str) -> Option<[u8; 32]> {
    if code.is_empty() || client_id.is_empty() {
        return None;
    }
    let mut hash = Sha256::new();
    hash.update(b"VOXLINK-PUNCH-AUTH-V1");
    hash.update(code.as_bytes());
    hash.update([0u8]);
    hash.update(client_id.as_bytes());
    Some(hash.finalize().into())
}

fn tag4(frame: &[u8], key: &[u8; 32]) -> [u8; 4] {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC 接受任意长度密钥");
    // 原版 frame.subarray(2)：不足 2 字节时取到空切片
    mac.update(frame.get(2..).unwrap_or(&[]));
    let digest = mac.finalize().into_bytes();
    let mut out = [0u8; 4];
    out.copy_from_slice(&digest[..4]);
    out
}

pub fn sign_punch_frame(frame: &[u8], key: Option<&[u8; 32]>) -> Vec<u8> {
    let Some(key) = key else {
        return frame.to_vec();
    };
    let mut out = frame.to_vec();
    out.extend_from_slice(&tag4(frame, key));
    out
}

pub fn verify_punch_frame(frame: &[u8], key: Option<&[u8; 32]>) -> Option<Vec<u8>> {
    let Some(key) = key else {
        return Some(frame.to_vec());
    };
    if frame.len() < 9 {
        return None;
    }
    let body = &frame[..frame.len() - 4];
    let want = tag4(body, key);
    let got = &frame[frame.len() - 4..];
    // 原版 crypto.timingSafeEqual：定长逐字节异或累加，不提前返回
    let mut diff = 0u8;
    for i in 0..4 {
        diff |= want[i] ^ got[i];
    }
    if diff == 0 {
        Some(body.to_vec())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_derivation_matches_layout() {
        // 只有拼接顺序（含中间那个 0x00 分隔符）全对才会得到这个摘要
        let key = derive_punch_key("ABCDEF", "client-1").unwrap();
        let mut expect = Sha256::new();
        expect.update(b"VOXLINK-PUNCH-AUTH-V1");
        expect.update(b"ABCDEF");
        expect.update([0u8]);
        expect.update(b"client-1");
        assert_eq!(key, <[u8; 32]>::from(expect.finalize()));
        assert!(derive_punch_key("", "client-1").is_none());
        assert!(derive_punch_key("ABCDEF", "").is_none());
    }

    #[test]
    fn sign_appends_four_bytes_and_skips_without_key() {
        let frame = vec![86u8, 76, 3, 0, 0, 0, 1, 0, 0, 0, 0, 7];
        let key = derive_punch_key("ABCDEF", "c").unwrap();
        let signed = sign_punch_frame(&frame, Some(&key));
        assert_eq!(signed.len(), frame.len() + 4);
        assert_eq!(&signed[..frame.len()], frame.as_slice());
        assert_eq!(sign_punch_frame(&frame, None), frame);
    }

    #[test]
    fn verify_rejects_tampered_tag_and_short_frames() {
        let key = derive_punch_key("ABCDEF", "c").unwrap();
        let signed = sign_punch_frame(&[86, 76, 3, 0, 0, 0, 1, 0, 0, 0, 0, 7], Some(&key));
        assert_eq!(verify_punch_frame(&signed, Some(&key)).unwrap().len(), 12);
        let mut bad = signed.clone();
        let last = bad.len() - 1;
        bad[last] ^= 0x01;
        assert!(verify_punch_frame(&bad, Some(&key)).is_none());
        // 原版 frame.length < 9 → null（8 字节带签名恰好落到这条）
        assert!(verify_punch_frame(&[86, 76, 3, 1, 2, 3, 4, 5], Some(&key)).is_none());
        assert_eq!(verify_punch_frame(&[1, 2, 3], None).unwrap(), vec![1, 2, 3]);
    }
}
