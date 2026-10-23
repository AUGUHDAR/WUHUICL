//! STUN 客户端（对应原版 voxlink/stun.ts 73 行，LGPL-3.0-only）
//! RFC 5389 + StunProbe.java 的顺序采样/重发策略，VoxLink 924845e。

use std::net::{IpAddr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub const STUN_MAGIC_COOKIE: u32 = 0x2112_a442;
pub const STUN_REQ_TYPE: u16 = 1;
pub const STUN_RESP_TYPE: u16 = 0x101;
pub const STUN_HEADER_LEN: usize = 20;

pub const STUN_SERVERS: [&str; 4] = [
    "stun.qq.com:3478",
    "stun.miwifi.com:3478",
    "stun.cloudflare.com:3478",
    "stun.l.google.com:19302",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StunMappedAddr {
    pub ip: String,
    pub port: u16,
}

pub fn valid_mapped_addr(value: &StunMappedAddr) -> bool {
    // 原版还判 port<=65535，u16 天然满足
    !value.ip.is_empty() && value.port > 0
}

pub fn stun_build_request() -> [u8; 20] {
    let mut request = [0u8; 20];
    request[0..2].copy_from_slice(&STUN_REQ_TYPE.to_be_bytes());
    request[4..8].copy_from_slice(&STUN_MAGIC_COOKIE.to_be_bytes());
    // 事务 ID 12 字节随机：与原版 randomBytes(12).copy(request,8) 同位
    let mut seed = [0u8; 12];
    rand::RngCore::fill_bytes(&mut rand::rng(), &mut seed);
    request[8..20].copy_from_slice(&seed);
    request
}

/// 解析一条 STUN 响应；None 表示"这个包不算，继续等"（原版的 return 语义）
pub fn stun_parse(packet: &[u8], request: &[u8]) -> Option<StunMappedAddr> {
    if packet.len() < STUN_HEADER_LEN {
        return None;
    }
    if u16::from_be_bytes([packet[0], packet[1]]) != STUN_RESP_TYPE {
        return None;
    }
    if &packet[4..20] != &request[4..20] {
        return None;
    }
    let end = STUN_HEADER_LEN + u16::from_be_bytes([packet[2], packet[3]]) as usize;
    if end > packet.len() {
        return None;
    }
    let mut cursor = STUN_HEADER_LEN;
    while cursor + 4 <= end {
        let atype = u16::from_be_bytes([packet[cursor], packet[cursor + 1]]);
        let size = u16::from_be_bytes([packet[cursor + 2], packet[cursor + 3]]) as usize;
        let start = cursor + 4;
        if start + size > end {
            return None;
        }
        if (atype == 1 || atype == 0x20 || atype == 0x8020) && size >= 8 && packet[start + 1] == 1 {
            let mask = if atype == 1 { 0u32 } else { STUN_MAGIC_COOKIE };
            let raw = u32::from_be_bytes([packet[start + 4], packet[start + 5], packet[start + 6], packet[start + 7]]);
            let ip = raw ^ mask;
            let port = u16::from_be_bytes([packet[start + 2], packet[start + 3]]) ^ ((mask >> 16) as u16);
            let value = StunMappedAddr {
                ip: std::net::Ipv4Addr::from(ip).to_string(),
                port,
            };
            // 原版：命中候选属性就结束本次解析，端口非法也不回退去扫后面的属性
            return if valid_mapped_addr(&value) { Some(value) } else { None };
        }
        cursor = start + ((size + 3) & !3);
    }
    None
}

/// StunProbe.java resendDelay：200 起步、800 封顶、±20% 抖动
pub fn stun_resend_delay(retransmissions: u32, random: f64) -> u64 {
    let rto = (200u64 << retransmissions.min(20)).min(800);
    let jitter = (rto as f64 * 0.2).trunc() as u64;
    rto - jitter + (random * (2.0 * jitter as f64 + 1.0)).floor() as u64
}

fn resolve_v4(server: &str) -> Option<SocketAddr> {
    let (host, port) = match server.rsplit_once(':') {
        Some((h, p)) => (h, p.parse::<u16>().ok()?),
        None => (server, 3478),
    };
    let addrs = (host, port).to_socket_addrs().ok()?;
    addrs.into_iter().find(|a| a.is_ipv4())
}

/// 对应原版 stunQuery：同一 socket 上多次尝试，每次按重发计划补包
pub fn stun_query(
    socket: &UdpSocket,
    server: &str,
    attempts: u32,
    timeout_ms: u64,
    cancelled: &AtomicBool,
    resends: u32,
) -> Option<StunMappedAddr> {
    let remote = resolve_v4(server)?;
    for _ in 0..attempts {
        if cancelled.load(Ordering::SeqCst) {
            return None;
        }
        let request = stun_build_request();
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        let _ = socket.send_to(&request, remote);
        let mut next_resend = if resends >= 1 { Some(Instant::now() + Duration::from_millis(stun_resend_delay(1, rand_random()))) } else { None };
        let mut attempt = 1u32;
        loop {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            if let Some(at) = next_resend {
                if now >= at {
                    let _ = socket.send_to(&request, remote);
                    attempt += 1;
                    next_resend = if attempt > resends {
                        None
                    } else {
                        Some(now + Duration::from_millis(stun_resend_delay(attempt, rand_random())))
                    };
                }
            }
            let _ = socket.set_read_timeout(Some(Duration::from_millis(20)));
            let mut buf = [0u8; 1024];
            match socket.recv_from(&mut buf) {
                Ok((size, from)) => {
                    if from != remote {
                        continue;
                    }
                    if let Some(answer) = stun_parse(&buf[..size], &request) {
                        return Some(answer);
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => {}
                Err(_) => return None,
            }
        }
    }
    None
}

fn rand_random() -> f64 {
    rand::Rng::random::<f64>(&mut rand::rng())
}

/// 原版模块级 preferred 集合：上次成功的服务器优先
#[derive(Default)]
pub struct StunSampler {
    preferred: Vec<String>,
}

impl StunSampler {
    /// 对应 stunSampleSeries：按 preferred 优先排序后逐个采样
    pub fn sample_series(&mut self, socket: &UdpSocket, servers: &[String], count: usize, tries: u32, timeout_ms: u64, cancelled: &AtomicBool) -> Vec<StunMappedAddr> {
        let mut ordered: Vec<String> = servers.to_vec();
        // 原版 Number(has(b)) - Number(has(a))：上次成功的服务器排前面
        ordered.sort_by(|a, b| self.preferred.contains(b).cmp(&self.preferred.contains(a)));
        let mut samples = Vec::new();
        for server in ordered {
            if samples.len() >= count || cancelled.load(Ordering::SeqCst) {
                break;
            }
            match stun_query(socket, &server, tries, timeout_ms, cancelled, 0) {
                Some(result) => {
                    if !self.preferred.contains(&server) {
                        self.preferred.push(server.clone());
                    }
                    samples.push(result);
                }
                None => self.preferred.retain(|s| *s != server),
            }
        }
        samples
    }
}

/// 对应 stunDeltaFromSamples：末次与首次端口差，样本不足记 0
pub fn stun_delta_from_samples(samples: &[StunMappedAddr]) -> i64 {
    if samples.len() > 1 {
        samples[samples.len() - 1].port as i64 - samples[0].port as i64
    } else {
        0
    }
}

/// 对应 quickNatType：两次采样拿不到就是 unknown
pub fn quick_nat_type(socket: &UdpSocket, cancelled: &AtomicBool) -> String {
    let servers: Vec<String> = STUN_SERVERS.iter().map(|s| s.to_string()).collect();
    let mut sampler = StunSampler::default();
    let samples = sampler.sample_series(socket, &servers, 2, 1, 1200, cancelled);
    if samples.len() < 2 {
        return "unknown".into();
    }
    if stun_delta_from_samples(&samples) != 0 {
        "symmetric".into()
    } else {
        "cone".into()
    }
}

/// 对应 samplePortsSequential：先并发抢一个可用服务器，再复用它的地址连续采样
pub fn sample_ports_sequential(
    socket: &UdpSocket,
    servers: &[String],
    count: usize,
    interval_ms: u64,
    cancelled: &AtomicBool,
) -> Vec<StunMappedAddr> {
    if servers.is_empty() || count == 0 {
        return Vec::new();
    }
    let mut selected: Option<(String, StunMappedAddr)> = None;
    for server in servers {
        if cancelled.load(Ordering::SeqCst) {
            break;
        }
        if let Some(result) = stun_query(socket, server, 1, 800, cancelled, 1) {
            selected = Some((server.clone(), result));
            break;
        }
    }
    let Some((winner, first)) = selected else { return Vec::new() };
    let mut samples = vec![first];
    for i in 1..count {
        if cancelled.load(Ordering::SeqCst) {
            break;
        }
        if let Some(result) = stun_query(socket, &winner, 1, 800, cancelled, 2) {
            samples.push(result);
        }
        if i < count - 1 {
            let deadline = Instant::now() + Duration::from_millis(interval_ms);
            while Instant::now() < deadline {
                if cancelled.load(Ordering::SeqCst) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }
    samples
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(transaction: &[u8], attrs: &[u8]) -> Vec<u8> {
        let mut packet = vec![0u8; 20];
        packet[0..2].copy_from_slice(&STUN_RESP_TYPE.to_be_bytes());
        packet[2..4].copy_from_slice(&(attrs.len() as u16).to_be_bytes());
        packet[4..8].copy_from_slice(&STUN_MAGIC_COOKIE.to_be_bytes());
        packet[8..20].copy_from_slice(&transaction[..12].to_vec()[..]);
        packet.extend_from_slice(attrs);
        packet
    }

    fn request_with_tail(tail: [u8; 12]) -> [u8; 20] {
        let mut request = [0u8; 20];
        request[0..2].copy_from_slice(&STUN_REQ_TYPE.to_be_bytes());
        request[4..8].copy_from_slice(&STUN_MAGIC_COOKIE.to_be_bytes());
        request[8..20].copy_from_slice(&tail);
        request
    }

    #[test]
    fn request_header_matches_wire_layout() {
        let r = stun_build_request();
        assert_eq!(&r[0..2], &[0x00, 0x01]);
        assert_eq!(&r[4..8], &[0x21, 0x12, 0xa4, 0x42]);
        assert_ne!(&r[8..20], &[0u8; 12]);
    }

    #[test]
    fn mapped_address_decodes_plain() {
        let tail = [1u8; 12];
        let request = request_with_tail(tail);
        let mut attrs = vec![0u8; 12];
        attrs[0..2].copy_from_slice(&1u16.to_be_bytes());
        attrs[2..4].copy_from_slice(&8u16.to_be_bytes());
        attrs[5] = 1;
        attrs[6..8].copy_from_slice(&25000u16.to_be_bytes());
        attrs[8..12].copy_from_slice(&[203, 0, 113, 1]);
        let parsed = stun_parse(&response(&tail, &attrs), &request).unwrap();
        assert_eq!(parsed.ip, "203.0.113.1");
        assert_eq!(parsed.port, 25000);
    }

    #[test]
    fn xor_mapped_address_un_masks_ip_and_port() {
        let tail = [2u8; 12];
        let request = request_with_tail(tail);
        let real_ip: u32 = u32::from_be_bytes([203, 0, 113, 9]);
        let real_port: u16 = 12345;
        let mut attrs = vec![0u8; 12];
        attrs[0..2].copy_from_slice(&0x0020u16.to_be_bytes());
        attrs[2..4].copy_from_slice(&8u16.to_be_bytes());
        attrs[5] = 1;
        attrs[6..8].copy_from_slice(&(real_port ^ (STUN_MAGIC_COOKIE >> 16) as u16).to_be_bytes());
        attrs[8..12].copy_from_slice(&(real_ip ^ STUN_MAGIC_COOKIE).to_be_bytes());
        let parsed = stun_parse(&response(&tail, &attrs), &request).unwrap();
        assert_eq!(parsed.ip, "203.0.113.9");
        assert_eq!(parsed.port, 12345);
    }

    #[test]
    fn ignores_wrong_transaction_type_and_short_body() {
        let tail = [3u8; 12];
        let request = request_with_tail(tail);
        let other = request_with_tail([9u8; 12]);
        let mut attrs = vec![0u8; 12];
        attrs[0..2].copy_from_slice(&1u16.to_be_bytes());
        attrs[2..4].copy_from_slice(&8u16.to_be_bytes());
        attrs[5] = 1;
        attrs[6..8].copy_from_slice(&7u16.to_be_bytes());
        attrs[8..12].copy_from_slice(&[10, 0, 0, 1]);
        let packet = response(&tail, &attrs);
        assert!(stun_parse(&packet, &other).is_none());
        let mut bad_type = packet.clone();
        bad_type[1] = 0x99;
        assert!(stun_parse(&bad_type, &request).is_none());
        assert!(stun_parse(&packet[..19], &request).is_none());
        // 声明长度超过实际包：继续等，不报错
        let mut lying = packet.clone();
        lying[2..4].copy_from_slice(&(9999u16).to_be_bytes());
        assert!(stun_parse(&lying, &request).is_none());
    }

    #[test]
    fn invalid_port_candidate_does_not_scan_further_attributes() {
        let tail = [4u8; 12];
        let request = request_with_tail(tail);
        let mut attrs = Vec::new();
        let mut first = vec![0u8; 12];
        first[0..2].copy_from_slice(&1u16.to_be_bytes());
        first[2..4].copy_from_slice(&8u16.to_be_bytes());
        first[5] = 1;
        first[6..8].copy_from_slice(&0u16.to_be_bytes());
        first[8..12].copy_from_slice(&[1, 1, 1, 1]);
        attrs.append(&mut first);
        let mut second = vec![0u8; 12];
        second[0..2].copy_from_slice(&1u16.to_be_bytes());
        second[2..4].copy_from_slice(&8u16.to_be_bytes());
        second[5] = 1;
        second[6..8].copy_from_slice(&80u16.to_be_bytes());
        second[8..12].copy_from_slice(&[2, 2, 2, 2]);
        attrs.append(&mut second);
        // 原版命中第一个候选属性就 return，端口 0 也不会退回去用第二个
        assert!(stun_parse(&response(&tail, &attrs), &request).is_none());
    }

    #[test]
    fn resend_delay_stays_in_jittered_window() {
        for n in 0..6u32 {
            let rto = (200u64 << n).min(800);
            let jitter = (rto as f64 * 0.2) as u64;
            for r in [0.0f64, 0.5, 0.999_999] {
                let value = stun_resend_delay(n, r);
                assert!(value >= rto - jitter && value <= rto + jitter, "n={} r={} value={}", n, r, value);
            }
        }
        // 随机数为 0 时落在窗口下沿：rto - jitter
        assert_eq!(stun_resend_delay(0, 0.0), 160);
        assert_eq!(stun_resend_delay(1, 0.0), 320);
        assert_eq!(stun_resend_delay(2, 0.0), 640);
        assert_eq!(stun_resend_delay(9, 0.0), 640);
        assert_eq!(stun_resend_delay(2, 0.999999), 960);
    }

    #[test]
    fn delta_and_nat_shape_rules() {
        let s = |p: u16| StunMappedAddr { ip: "1.1.1.1".into(), port: p };
        assert_eq!(stun_delta_from_samples(&[s(100)]), 0);
        assert_eq!(stun_delta_from_samples(&[s(100), s(140)]), 40);
        assert_eq!(stun_delta_from_samples(&[s(100), s(100)]), 0);
        assert!(valid_mapped_addr(&s(65535)));
        assert!(!valid_mapped_addr(&StunMappedAddr { ip: "1.1.1.1".into(), port: 0 }));
        assert!(!valid_mapped_addr(&StunMappedAddr { ip: String::new(), port: 80 }));
    }

    #[test]
    fn resolve_v4_defaults_port_and_prefers_ipv4() {
        assert_eq!(resolve_v4("127.0.0.1").unwrap().port(), 3478);
        assert_eq!(resolve_v4("127.0.0.1:1234").unwrap().port(), 1234);
        assert!(resolve_v4("not a host").is_none());
    }

    #[test]
    fn cancelled_query_returns_immediately() {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let flag = AtomicBool::new(true);
        assert!(stun_query(&socket, "127.0.0.1:3478", 3, 50, &flag, 0).is_none());
    }

    /// 真实对端验证：向公网 STUN 服务器实发一次（原版联机启动即会做，只读）
    #[test]
    #[ignore = "需要公网"]
    fn live_stun_query_against_public_servers() {
        let socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        let flag = AtomicBool::new(false);
        let servers: Vec<String> = STUN_SERVERS.iter().map(|v| v.to_string()).collect();
        let mut sampler = StunSampler::default();
        let samples = sampler.sample_series(&socket, &servers, 2, 2, 1500, &flag);
        println!("STUN 采样 {} 条: {:?}", samples.len(), samples);
        assert!(!samples.is_empty(), "四个公网 STUN 全部无响应");
        for sample in &samples {
            let parsed: IpAddr = sample.ip.parse().expect("应是合法 IPv4");
            assert!(parsed.is_ipv4());
            assert!(sample.port > 0);
        }
    }

    #[test]
    #[ignore = "需要公网"]
    fn live_quick_nat_type_shape() {
        let socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        let flag = AtomicBool::new(false);
        let kind = quick_nat_type(&socket, &flag);
        println!("NAT 类型: {}", kind);
        assert!(["unknown", "symmetric", "cone"].contains(&kind.as_str()));
    }
}
