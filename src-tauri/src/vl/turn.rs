//! VoxLink 私有 TURN 中继协议 v1（对应原版 voxlink/turn.ts 149 行，LGPL-3.0-only）
//! 上游 TurnRelayClient.java @6b11d93 / 1.1.5。注意：这不是 RFC TURN。

use crate::vl::rudp::{now_ms, RudpCodec, RudpTarget};
use crate::vl::turntcp::{open_turn_tcp, TurnTcpChannel};
use std::net::{IpAddr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const TURN_HEADER: [u8; 3] = [0x56, 0x4c, 1];
pub const TURN_TYPE_PING: u8 = 1;
pub const TURN_TYPE_PONG: u8 = 2;
pub const TURN_TYPE_BIND: u8 = 3;
pub const TURN_TYPE_BOUND: u8 = 4;
pub const TURN_TYPE_DATA: u8 = 5;
pub const TURN_TYPE_KEEPALIVE: u8 = 6;
pub const TURN_TYPE_LEAVE: u8 = 8;
pub const TURN_BIND_TIMEOUT_MS: u64 = 4600;
pub const TURN_BIND_REPEAT_MS: u64 = 900;
pub const TURN_BIND_ROUNDS: u32 = 3;
pub const TURN_BIND_RETRY_DELAY_MS: u64 = 1000;
pub const TURN_KEEPALIVE_MS: u64 = 15_000;
pub const TURN_PROBE_PACKETS: usize = 6;
pub const TURN_PROBE_DEADLINE_MS: u64 = 4000;
pub const TURN_PROBE_ROUND_TIMEOUT_MS: u64 = 800;
pub const TURN_PROBE_MAX_NODES: usize = 32;
pub const TURN_TICKET_MAX: usize = 4096;

#[derive(Clone, Debug, PartialEq)]
pub struct TurnNode {
    pub id: String,
    pub name: Option<String>,
    pub host: String,
    pub port: u16,
    pub std_turn_port: Option<u16>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TurnAllocation {
    pub session_id: String,
    pub host: String,
    pub port: u16,
    pub host_ticket: String,
    pub guest_ticket: String,
    pub expire: f64,
}

/// 对应 validTurnEndpoint
pub fn valid_turn_endpoint(host: Option<&str>, port: Option<f64>) -> bool {
    let Some(host) = host else { return false };
    if host.is_empty() || host.len() > 253 {
        return false;
    }
    match port {
        Some(value) if value.is_finite() && value == value.trunc() && value > 0.0 && value <= 65535.0 => true,
        _ => false,
    }
}

fn header(rtype: u8) -> [u8; 4] {
    [TURN_HEADER[0], TURN_HEADER[1], TURN_HEADER[2], rtype]
}

fn is_packet(p: &[u8], rtype: u8, length: usize) -> bool {
    p.len() >= length && p[0] == 0x56 && p[1] == 0x4c && p[2] == 1 && p[3] == rtype
}

fn parse_session_id(session_id: &str) -> Option<[u8; 16]> {
    let bytes = session_id.as_bytes();
    if bytes.len() != 32 {
        return None;
    }
    let mut out = [0u8; 16];
    for i in 0..16 {
        let hi = (bytes[i * 2] as char).to_digit(16)?;
        let lo = (bytes[i * 2 + 1] as char).to_digit(16)?;
        out[i] = (hi * 16 + lo) as u8;
    }
    Some(out)
}

fn resolve_host(host: &str, port: u16) -> Option<SocketAddr> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Some(SocketAddr::new(ip, port));
    }
    (host, port).to_socket_addrs().ok()?.next()
}

/// 对应 exchange()：发一次并按 repeat 补发，等一个通过 accept 的回包
fn exchange(
    socket: &UdpSocket,
    target: SocketAddr,
    packet: &[u8],
    accept: &dyn Fn(&[u8]) -> bool,
    timeout_ms: u64,
    repeat_ms: u64,
    cancelled: &AtomicBool,
) -> Result<Vec<u8>, String> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    // 原版 exchange：先无条件发一次，repeat 只决定补发节奏（含首包最多 5 次）
    if socket.send_to(packet, target).is_err() {
        return Err("中继连接已关闭".into());
    }
    let mut next_repeat = if repeat_ms > 0 { Some(Instant::now() + Duration::from_millis(repeat_ms)) } else { None };
    let mut sent = 1u32;
    let mut buf = vec![0u8; 4096];
    loop {
        if cancelled.load(Ordering::SeqCst) {
            return Err("中继连接已取消".into());
        }
        let now = Instant::now();
        if now >= deadline {
            return Err("中继节点响应超时".into());
        }
        if let Some(at) = next_repeat {
            if now >= at {
                if sent < 5 {
                    sent += 1;
                    if socket.send_to(packet, target).is_err() {
                        return Err("中继连接已关闭".into());
                    }
                }
                next_repeat = Some(at + Duration::from_millis(repeat_ms));
            }
        }
        let left = deadline.saturating_duration_since(Instant::now()).min(Duration::from_millis(50));
        let _ = socket.set_read_timeout(Some(left));
        match socket.recv_from(&mut buf) {
            Ok((size, from)) => {
                if from != target {
                    continue;
                }
                let packet_in = buf[..size].to_vec();
                if accept(&packet_in) {
                    return Ok(packet_in);
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(_) => return Err("中继连接已关闭".into()),
        }
    }
}

/// 对应 probeTurnNodes：每节点最多 6 包、4 秒窗口，取最小 RTT，离线排最后
pub fn probe_turn_nodes(nodes: &[TurnNode], cancelled: &Arc<AtomicBool>) -> Vec<(TurnNode, f64)> {
    let mut results: Vec<(TurnNode, f64)> = Vec::new();
    for node in nodes.iter().take(TURN_PROBE_MAX_NODES) {
        let Some(target) = resolve_host(&node.host, node.port) else {
            results.push((node.clone(), -1.0));
            continue;
        };
        let Ok(socket) = UdpSocket::bind("0.0.0.0:0") else {
            results.push((node.clone(), -1.0));
            continue;
        };
        let deadline = Instant::now() + Duration::from_millis(TURN_PROBE_DEADLINE_MS);
        let mut rtt = -1.0f64;
        for _ in 0..TURN_PROBE_PACKETS {
            if Instant::now() >= deadline || cancelled.load(Ordering::SeqCst) {
                break;
            }
            let mut packet = vec![0u8; 20];
            packet[0..4].copy_from_slice(&header(TURN_TYPE_PING));
            let mut nonce = [0u8; 4];
            rand::RngCore::fill_bytes(&mut rand::rng(), &mut nonce);
            packet[4..8].copy_from_slice(&nonce);
            packet[8..16].copy_from_slice(&(now_ms() as i64).to_be_bytes());
            let left = TURN_PROBE_ROUND_TIMEOUT_MS.min((deadline - Instant::now()).as_millis() as u64);
            let start = now_ms();
            let accept = |p: &[u8]| -> bool { is_packet(p, TURN_TYPE_PONG, 24) && p[4..8] == packet[4..8] };
            match exchange(&socket, target, &packet, &accept, left, 0, cancelled) {
                Ok(_) => {
                    let sample = now_ms() - start;
                    rtt = if rtt < 0.0 { sample } else { rtt.min(sample) };
                }
                Err(_) => {
                    if cancelled.load(Ordering::SeqCst) {
                        break;
                    }
                }
            }
        }
        results.push((node.clone(), rtt));
    }
    if cancelled.load(Ordering::SeqCst) {
        return Vec::new();
    }
    // 原版按 rtt<0 视为 Infinity 排序
    results.sort_by(|a, b| {
        let ka = if a.1 < 0.0 { f64::INFINITY } else { a.1 };
        let kb = if b.1 < 0.0 { f64::INFINITY } else { b.1 };
        ka.partial_cmp(&kb).unwrap_or(std::cmp::Ordering::Equal)
    });
    results
}

/// 对应 TurnCodec：24 字节头 = MAGIC(4) + session(16) + role + peerRole + u16 长度
#[derive(Debug)]
pub struct TurnCodec {
    pub session: [u8; 16],
    pub role: u8,
}

impl TurnCodec {
    pub fn new(session_id: &str, role: u8) -> Result<Self, String> {
        match parse_session_id(session_id) {
            Some(session) => Ok(TurnCodec { session, role }),
            None => Err("中继会话标识无效".into()),
        }
    }
}

impl RudpCodec for TurnCodec {
    fn encode(&self, frame: &[u8]) -> Vec<u8> {
        let mut out = vec![0u8; 24 + frame.len()];
        out[0..4].copy_from_slice(&header(TURN_TYPE_DATA));
        out[4..20].copy_from_slice(&self.session);
        out[20] = self.role;
        out[21] = 3 - self.role;
        out[22..24].copy_from_slice(&(frame.len() as u16).to_be_bytes());
        out[24..].copy_from_slice(frame);
        out
    }

    fn decode(&self, packet: &[u8]) -> Option<Vec<u8>> {
        if !is_packet(packet, TURN_TYPE_DATA, 24)
            || packet[4..20] != self.session
            || packet[20] != 3 - self.role
            || packet[21] != self.role
            || u16::from_be_bytes([packet[22], packet[23]]) as usize != packet.len() - 24
        {
            return None;
        }
        Some(packet[24..].to_vec())
    }
}

/// 对应 TurnSession：绑定成功后交给 RUDP 用，keepalive 每 15 秒发 type6
#[derive(Debug)]
pub struct TurnSession {
    pub socket: Arc<UdpSocket>,
    pub target: RudpTarget,
    pub codec: Arc<TurnCodec>,
    role: u8,
    closed: Arc<AtomicBool>,
    bound: Arc<AtomicBool>,
    tcp: Arc<Mutex<Option<TurnTcpChannel>>>,
    keepalive: Arc<Mutex<Option<std::thread::JoinHandle<()>>>>,
}

impl TurnSession {
    fn control_packet(&self, rtype: u8) -> Vec<u8> {
        let mut p = vec![0u8; 21];
        p[0..4].copy_from_slice(&header(rtype));
        p[4..20].copy_from_slice(&self.codec.session);
        p[20] = self.role;
        p
    }

    pub fn send_control(&self, rtype: u8) {
        if self.closed.load(Ordering::SeqCst) {
            return;
        }
        let packet = self.control_packet(rtype);
        let target = SocketAddr::new(
            self.target.address.parse().unwrap_or(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)),
            self.target.port,
        );
        let _ = self.socket.send_to(&packet, target);
    }

    pub fn close(&self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        if let Some(handle) = self.keepalive.lock().unwrap().take() {
            let _ = handle;
        }
        if self.bound.load(Ordering::SeqCst) {
            self.send_control(TURN_TYPE_LEAVE);
        }
        if let Some(tcp) = self.tcp.lock().unwrap().as_ref() {
            tcp.close();
        }
    }

    pub fn start_keepalive(&self) {
        let socket = self.socket.clone();
        let target = self.target.clone();
        let packet = self.control_packet(TURN_TYPE_KEEPALIVE);
        let closed = self.closed.clone();
        let handle = std::thread::spawn(move || {
            while !closed.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(TURN_KEEPALIVE_MS));
                if closed.load(Ordering::SeqCst) {
                    return;
                }
                let addr = match target.address.parse() {
                    Ok(ip) => SocketAddr::new(ip, target.port),
                    Err(_) => return,
                };
                let _ = socket.send_to(&packet, addr);
            }
        });
        *self.keepalive.lock().unwrap() = Some(handle);
    }
}

/// 对应 TurnSession.bind：UDP 三轮 → 仍失败则换 TCP 隧道再两轮 → 只有 0 才算绑定成功
pub fn turn_session_bind(
    session_id: &str,
    host: &str,
    port: u16,
    ticket: &str,
    role: u8,
    cancelled: &Arc<AtomicBool>,
) -> Result<TurnSession, String> {
    if !valid_turn_endpoint(Some(host), Some(port as f64))
        || parse_session_id(session_id).is_none()
        || ticket.is_empty()
        || ticket.len() > TURN_TICKET_MAX
    {
        return Err("中继凭据无效".into());
    }
    let codec = Arc::new(TurnCodec::new(session_id, role)?);
    let target_addr = resolve_host(host, port).ok_or_else(|| "中继地址无法解析".to_string())?;
    let socket = Arc::new(UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?);
    let mut packet = vec![0u8; 23 + ticket.len()];
    packet[0..4].copy_from_slice(&header(TURN_TYPE_BIND));
    packet[4..20].copy_from_slice(&codec.session);
    packet[20] = role;
    packet[21..23].copy_from_slice(&(ticket.len() as u16).to_be_bytes());
    packet[23..].copy_from_slice(ticket.as_bytes());

    let session_target = RudpTarget { address: target_addr.ip().to_string(), port: target_addr.port() };
    let bound = Arc::new(AtomicBool::new(false));
    let closed = Arc::new(AtomicBool::new(false));
    let tcp_slot: Arc<Mutex<Option<TurnTcpChannel>>> = Arc::new(Mutex::new(None));

    let bind_round = |sock: &Arc<UdpSocket>, peer: SocketAddr| -> u8 {
        let session = codec.session;
        let accept = |p: &[u8]| -> bool {
            is_packet(p, TURN_TYPE_BOUND, 22) && p[4..20] == session && p[20] == role
        };
        match exchange(sock, peer, &packet, &accept, TURN_BIND_TIMEOUT_MS, TURN_BIND_REPEAT_MS, cancelled) {
            // ROLE_CONFLICT(4) 在上游语义里是"绑定成功但首包丢了"
            Ok(result) => if result[21] == 4 { 0 } else { result[21] },
            Err(_) => 5,
        }
    };

    let mut code = 5u8;
    for round in 0..TURN_BIND_ROUNDS {
        code = bind_round(&socket, target_addr);
        if code == 0 {
            break;
        }
        if code != 5 && code != 4 {
            return Err(match code {
                1 => "中继凭据无效".into(),
                2 => "中继凭据过期".into(),
                3 => "中继会话已满".into(),
                other => format!("中继绑定失败（代码 {}）", other),
            });
        }
        if round < TURN_BIND_ROUNDS - 1 {
            std::thread::sleep(Duration::from_millis(TURN_BIND_RETRY_DELAY_MS));
        }
    }
    if code == 5 {
        // 4600ms×3 轮仍无响应：换 TCP 隧道（turnTcp）
        let channel = match open_turn_tcp(host, port, &Arc::new(Mutex::new(cancelled.load(Ordering::SeqCst)))) {
            Ok(channel) => channel,
            Err(_) => {
                return Err("中继绑定失败（UDP 与 TCP 均不可用）".into());
            }
        };
        let shim = SocketAddr::new(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST), channel.shim_port);
        let tcp_socket = channel.socket.clone();
        *tcp_slot.lock().unwrap() = Some(channel);
        for round in 0..2u32 {
            code = bind_round(&tcp_socket, shim);
            if code == 0 {
                break;
            }
            if code != 4 && code != 5 {
                break;
            }
            if round == 0 {
                std::thread::sleep(Duration::from_millis(TURN_BIND_RETRY_DELAY_MS));
            }
        }
        if code == 0 {
                let session = TurnSession {
                socket: tcp_socket,
                target: RudpTarget { address: "127.0.0.1".into(), port: shim.port() },
                codec,
                role,
                closed,
                bound: bound.clone(),
                tcp: tcp_slot,
                keepalive: Arc::new(Mutex::new(None)),
            };
            bound.store(true, Ordering::SeqCst);
            session.start_keepalive();
            return Ok(session);
        }
        return Err(if code == 4 {
            "中继角色暂被占用，请稍后重试".into()
        } else {
            "中继绑定失败（UDP 与 TCP 均不可用）".into()
        });
    }
    if code != 0 {
        return Err(if code == 4 {
            "中继角色暂被占用，请稍后重试".into()
        } else {
            "中继绑定失败".into()
        });
    }
    bound.store(true, Ordering::SeqCst);
    let session = TurnSession {
        socket,
        target: session_target,
        codec,
        role,
        closed,
        bound,
        tcp: tcp_slot,
        keepalive: Arc::new(Mutex::new(None)),
    };
    session.start_keepalive();
    Ok(session)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session_hex() -> String {
        "00112233445566778899aabbccddeeff".to_string()
    }

    #[test]
    fn endpoint_validation_matches_original() {
        assert!(valid_turn_endpoint(Some("turn.example.com"), Some(3478.0)));
        assert!(!valid_turn_endpoint(Some(""), Some(3478.0)));
        assert!(!valid_turn_endpoint(None, Some(3478.0)));
        assert!(!valid_turn_endpoint(Some("h"), Some(0.0)));
        assert!(!valid_turn_endpoint(Some("h"), Some(65536.0)));
        assert!(!valid_turn_endpoint(Some("h"), Some(3478.5)));
        assert!(!valid_turn_endpoint(Some("h"), None));
        assert!(!valid_turn_endpoint(Some(&"a".repeat(254)), Some(3478.0)));
    }

    #[test]
    fn codec_header_is_24_bytes_with_mirrored_roles() {
        let codec = TurnCodec::new(&session_hex(), 1).unwrap();
        let frame = codec.encode(b"abc");
        assert_eq!(&frame[0..4], &[0x56, 0x4c, 1, 5]);
        assert_eq!(&frame[20], &1);
        assert_eq!(&frame[21], &2);
        assert_eq!(&frame[22..24], &[0, 3]);
        assert_eq!(&frame[24..], b"abc");
        // turn.ts:90-91 的 decode 要求 [3-role, role]：自己发的帧自己解不开，
        // 只有中继翻过角色之后才解得开 —— 这条必须钉住
        assert!(codec.decode(&frame).is_none(), "同角色自解必须失败");
        let peer = TurnCodec::new(&session_hex(), 2).unwrap();
        assert_eq!(peer.decode(&frame).unwrap(), b"abc", "对端角色必须能解开");
        let mut flipped = frame.clone();
        flipped[20] = 1;
        assert!(codec.decode(&flipped).is_none(), "同角色自解必须失败");
        // 长度不自洽必须拒收
        let mut lying = frame.clone();
        lying[22..24].copy_from_slice(&[0, 9]);
        assert!(codec.decode(&lying).is_none());
        assert!(TurnCodec::new("不是十六进制", 1).is_err());
        assert_eq!(TurnCodec::new("不是十六进制", 1).unwrap_err(), "中继会话标识无效");
    }

    #[test]
    fn bind_rejects_bad_credentials_before_any_socket() {
        let cancelled = Arc::new(AtomicBool::new(false));
        assert_eq!(
            turn_session_bind(&session_hex(), "", 3478, "t", 1, &cancelled).unwrap_err(),
            "中继凭据无效"
        );
        assert_eq!(
            turn_session_bind("zz", "127.0.0.1", 3478, "t", 1, &cancelled).unwrap_err(),
            "中继凭据无效"
        );
        assert_eq!(
            turn_session_bind(&session_hex(), "127.0.0.1", 3478, "", 1, &cancelled).unwrap_err(),
            "中继凭据无效"
        );
    }

    struct FakeNode {
        port: u16,
        hits: Arc<Mutex<usize>>,
    }

    /// 假中继节点：收到 BIND 就回 BOUND，第 21 字节按配置返回
    fn fake_node(reply_code: u8) -> FakeNode {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = socket.local_addr().unwrap().port();
        let hits = Arc::new(Mutex::new(0usize));
        let counter = hits.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok((size, from)) = socket.recv_from(&mut buf) {
                let req = &buf[..size];
                if !is_packet(req, TURN_TYPE_BIND, 23) {
                    continue;
                }
                *counter.lock().unwrap() += 1;
                let mut reply = vec![0u8; 22];
                reply[0..4].copy_from_slice(&header(TURN_TYPE_BOUND));
                reply[4..20].copy_from_slice(&req[4..20]);
                reply[20] = req[20];
                reply[21] = reply_code;
                let _ = socket.send_to(&reply, from);
            }
        });
        FakeNode { port, hits }
    }

    #[test]
    fn bind_succeeds_and_reports_the_relay_target() {
        let node = fake_node(0);
        let cancelled = Arc::new(AtomicBool::new(false));
        let session = turn_session_bind(&session_hex(), "127.0.0.1", node.port, "ticket-1", 1, &cancelled).unwrap();
        assert_eq!(session.target.address, "127.0.0.1");
        assert_eq!(session.target.port, node.port);
        assert!(*node.hits.lock().unwrap() >= 1);
        session.close();
    }

    #[test]
    fn role_conflict_is_treated_as_bound() {
        // 上游语义：code 4 = 绑定成功但首个回包丢了
        let node = fake_node(4);
        let cancelled = Arc::new(AtomicBool::new(false));
        let session = turn_session_bind(&session_hex(), "127.0.0.1", node.port, "ticket-2", 2, &cancelled);
        assert!(session.is_ok(), "ROLE_CONFLICT 应当视为绑定成功");
        session.unwrap().close();
    }

    #[test]
    fn explicit_failure_codes_map_to_the_original_texts() {
        let cancelled = Arc::new(AtomicBool::new(false));
        for (code, text) in [(1u8, "中继凭据无效"), (2, "中继凭据过期"), (3, "中继会话已满")] {
            let node = fake_node(code);
            let err = turn_session_bind(&session_hex(), "127.0.0.1", node.port, "t", 1, &cancelled).unwrap_err();
            assert_eq!(err, text, "code={}", code);
        }
    }

    #[test]
    fn silent_node_falls_back_to_tcp_then_reports_unavailable() {
        // 没有任何 UDP 应答：UDP 三轮 + TCP 隧道也连不上 → 原版的合并文案
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let cancelled = Arc::new(AtomicBool::new(false));
        let err = turn_session_bind(&session_hex(), "127.0.0.1", port, "t", 1, &cancelled).unwrap_err();
        assert_eq!(err, "中继绑定失败（UDP 与 TCP 均不可用）");
    }

    fn node(id: &str, port: u16) -> TurnNode {
        TurnNode { id: id.into(), name: None, host: "127.0.0.1".into(), port, std_turn_port: None }
    }

    #[test]
    fn exchange_roundtrip_with_repeat_zero() {
        // 对照组：直接走 exchange()，把 probe 的循环排除在外
        let live = UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = live.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let mut buf = [0u8; 64];
            while let Ok((size, from)) = live.recv_from(&mut buf) {
                let req = &buf[..size];
                if !is_packet(req, TURN_TYPE_PING, 20) {
                    continue;
                }
                let mut pong = vec![0u8; 24];
                pong[0..4].copy_from_slice(&header(TURN_TYPE_PONG));
                pong[4..8].copy_from_slice(&req[4..8]);
                let _ = live.send_to(&pong, from);
            }
        });
        let probe = UdpSocket::bind("0.0.0.0:0").unwrap();
        let target: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
        let mut packet = vec![0u8; 20];
        packet[0..4].copy_from_slice(&header(TURN_TYPE_PING));
        packet[4..8].copy_from_slice(&[1, 2, 3, 4]);
        let cancelled = AtomicBool::new(false);
        let accept = |p: &[u8]| -> bool { is_packet(p, TURN_TYPE_PONG, 24) && p[4..8] == [1, 2, 3, 4] };
        let got = exchange(&probe, target, &packet, &accept, 1500, 0, &cancelled);
        assert!(got.is_ok(), "exchange(repeat=0) 失败：{:?}", got.err());
    }

    #[test]
    fn probe_measures_a_live_node() {
        let live = UdpSocket::bind("127.0.0.1:0").unwrap();
        let live_port = live.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let mut buf = [0u8; 64];
            while let Ok((size, from)) = live.recv_from(&mut buf) {
                let req = &buf[..size];
                if !is_packet(req, TURN_TYPE_PING, 20) {
                    continue;
                }
                let mut pong = vec![0u8; 24];
                pong[0..4].copy_from_slice(&header(TURN_TYPE_PONG));
                pong[4..8].copy_from_slice(&req[4..8]);
                let _ = live.send_to(&pong, from);
            }
        });
        let cancelled = Arc::new(AtomicBool::new(false));
        let probed = probe_turn_nodes(&[node("live", live_port)], &cancelled);
        assert_eq!(probed.len(), 1);
        assert!(probed[0].1 >= 0.0, "在线节点必须测出 RTT，实测 {}", probed[0].1);
    }

    #[test]
    fn probe_marks_a_silent_node_offline() {
        let listener = UdpSocket::bind("127.0.0.1:0").unwrap();
        let dead_port = listener.local_addr().unwrap().port();
        drop(listener);
        let cancelled = Arc::new(AtomicBool::new(false));
        let probed = probe_turn_nodes(&[node("dead", dead_port)], &cancelled);
        assert_eq!(probed[0].1, -1.0, "无响应节点必须记 -1");
    }

    #[test]
    fn offline_nodes_sort_last() {
        // 排序口径：rtt<0 视作 Infinity 排最后，其余升序
        let mut results = vec![("dead".to_string(), -1.0f64), ("fast".to_string(), 12.0), ("slow".to_string(), 80.0)];
        results.sort_by(|a, b| {
            let ka = if a.1 < 0.0 { f64::INFINITY } else { a.1 };
            let kb = if b.1 < 0.0 { f64::INFINITY } else { b.1 };
            ka.partial_cmp(&kb).unwrap_or(std::cmp::Ordering::Equal)
        });
        assert_eq!(results.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), vec!["fast", "slow", "dead"]);
    }
}
