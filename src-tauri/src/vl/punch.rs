//! UDP 打洞（对应原版 voxlink/punch.ts 181 行，LGPL-3.0-only）
//! 线上语义来自 UdpHolePuncher.java, VoxLink 924845e。

use crate::vl::punchauth::{sign_punch_frame, verify_punch_frame};
use crate::vl::punchpolicy::{from_profile, PunchParams, PunchResult};
use crate::vl::punchprofiles::{PunchProfile, DEFAULT};
use std::collections::HashSet;
use std::net::{IpAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub type Rng = dyn Fn() -> f64 + Send + Sync;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PunchAddress {
    pub address: String,
    pub port: u16,
}

/// UdpHolePuncher.java: MAGIC + CONTROL_PLAIN_LEN
pub fn punch_build_control(rtype: u8, nonce: u16) -> [u8; 5] {
    let mut b = [86u8, 76, rtype, 0, 0];
    b[3..5].copy_from_slice(&nonce.to_be_bytes());
    b
}

pub fn punch_parse_control(b: &[u8]) -> Option<(u8, u16)> {
    if b.len() >= 5 && b[0] == 86 && b[1] == 76 && (b[2] == 1 || b[2] == 2) {
        Some((b[2], u16::from_be_bytes([b[3], b[4]])))
    } else {
        None
    }
}

pub fn punch_random_nonce() -> u16 {
    let mut bytes = [0u8; 2];
    rand::RngCore::fill_bytes(&mut rand::rng(), &mut bytes);
    u16::from_be_bytes(bytes)
}

fn is_ipv4_text(value: &str) -> bool {
    value.parse::<IpAddr>().map(|ip| ip.is_ipv4()).unwrap_or(false)
}

fn first_two_octets(value: &str) -> Option<String> {
    let parts: Vec<&str> = value.split('.').collect();
    if parts.len() != 4 {
        return None;
    }
    Some(format!("{}.{}", parts[0], parts[1]))
}

/// UdpHolePuncher.java acceptAddress：同 IP 直接过；仅未鉴权的控制包允许 /16 CGNAT 漂移
pub fn punch_accept_source(expected: Option<&PunchAddress>, actual: Option<&PunchAddress>, authenticated: bool) -> bool {
    let (Some(expected), Some(actual)) = (expected, actual) else { return false };
    if expected.address == actual.address {
        return true;
    }
    if authenticated {
        return false;
    }
    if !is_ipv4_text(&expected.address) || !is_ipv4_text(&actual.address) {
        return false;
    }
    first_two_octets(&expected.address) == first_two_octets(&actual.address)
}

pub struct PpsLimiter {
    max_pps: f64,
    start: f64,
    sent: u64,
}

impl PpsLimiter {
    pub fn new(max_pps: f64) -> Self {
        PpsLimiter { max_pps, start: elapsed_ms(), sent: 0 }
    }

    /// UdpHolePuncher.java beforeSend：单调预算，超发就补睡
    pub fn before_send_delay(&mut self) -> f64 {
        self.sent += 1;
        let allowed = ((elapsed_ms() - self.start) * self.max_pps.max(1.0) / 1000.0).floor() as i64;
        if self.sent as i64 > allowed {
            (self.sent as i64 - allowed) as f64 * 1000.0 / self.max_pps.max(1.0)
        } else {
            0.0
        }
    }
}

fn elapsed_ms() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    // performance.now() 在原版里是单调时钟，这里用同一进程内的毫秒差
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64() * 1000.0).unwrap_or_default()
}

pub fn shuffle(values: &mut Vec<i64>, random: &Rng) {
    let mut i = values.len();
    while i > 0 {
        i -= 1;
        let j = (random() * (i as f64 + 1.0)).floor() as usize;
        if j < values.len() && j != i {
            values.swap(i, j);
        }
    }
}

/// UdpHolePuncher.java sendControlMultiPort：大范围走随机窗口扫描（保留中心端口重复）
pub fn control_ports(center: i64, range: i64, window: i64, random: &Rng) -> Vec<i64> {
    let mut ports = vec![center];
    if range > 20 {
        let lo = (1i64).max(center - range);
        let hi = 65535i64.min(center + range);
        let max = window.min(hi - lo);
        let mut chosen: Vec<i64> = vec![center];
        let mut seen: HashSet<i64> = HashSet::new();
        seen.insert(center);
        let mut guard = 0u64;
        while (chosen.len() as i64) < max + 1 {
            guard += 1;
            if guard > 200_000 {
                break;
            }
            let candidate = lo + (random() * (hi - lo + 1) as f64).floor() as i64;
            if seen.insert(candidate) {
                chosen.push(candidate);
            }
        }
        shuffle(&mut chosen, random);
        ports.extend(chosen);
    } else {
        let mut offset = 1i64;
        while offset <= range {
            if center - offset > 0 {
                ports.push(center - offset);
            }
            if center + offset <= 65535 {
                ports.push(center + offset);
            }
            offset += 1;
        }
    }
    ports
}

#[derive(Clone, Debug, PartialEq)]
pub struct PunchFailure {
    pub result: PunchResult,
    pub message: String,
}

#[derive(Clone)]
pub enum PunchMode {
    Prediction,
    Ports,
    Group,
}

pub struct PuncherOptions {
    pub timeout_ms: Option<f64>,
    pub auth_key: Option<[u8; 32]>,
    pub profile: Option<PunchProfile>,
    pub params: Option<PunchParams>,
    pub mode: PunchMode,
    pub range: Option<i64>,
    pub fixed_range: bool,
    pub skip_firewall: bool,
    pub sweep_spread: Option<i64>,
}

impl Default for PuncherOptions {
    fn default() -> Self {
        PuncherOptions {
            timeout_ms: None,
            auth_key: None,
            profile: None,
            params: None,
            mode: PunchMode::Prediction,
            range: None,
            fixed_range: false,
            skip_firewall: false,
            sweep_spread: None,
        }
    }
}

struct State {
    active: bool,
    settled: bool,
    remote: Option<PunchAddress>,
    ports: Vec<i64>,
    stats: PunchResult,
    started_at: f64,
    spray_table: Option<Vec<i64>>,
    spray_cursor: usize,
}

struct Inner {
    sockets: Vec<Arc<UdpSocket>>,
    conn: Arc<UdpSocket>,
    nonce: u16,
    profile: PunchProfile,
    params: PunchParams,
    timeout_ms: f64,
    options: PuncherOptions,
    state: Mutex<State>,
    cancel: AtomicBool,
    result: Mutex<Option<std::sync::mpsc::Sender<Result<PunchAddress, PunchFailure>>>>,
    waiter: Mutex<Option<std::sync::mpsc::Receiver<Result<PunchAddress, PunchFailure>>>>,
    answer: Mutex<Option<Result<PunchAddress, PunchFailure>>>,
}

pub struct Puncher {
    inner: Arc<Inner>,
}

fn mode_is_group(mode: &PunchMode) -> bool {
    matches!(mode, PunchMode::Group)
}

impl Puncher {
    pub fn new(conn: Arc<UdpSocket>, sockets: Vec<Arc<UdpSocket>>, options: PuncherOptions) -> Self {
        let profile = options.profile.clone().unwrap_or_else(|| DEFAULT.clone());
        let params = options.params.clone().unwrap_or_else(|| from_profile(&profile));
        let timeout_ms = options.timeout_ms.unwrap_or(params.timeout_ms);
        let sockets_tried = if sockets.is_empty() { 1 } else { sockets.len() };
        let range = options.range.unwrap_or(0);
        let stats = PunchResult {
            sockets_tried: sockets_tried as f64,
            sockets_received_punch: 0.0,
            sockets_received_ack: 0.0,
            prediction_delta: 0.0,
            elapsed_ms: 0.0,
            firewall_detected: false,
            port_prediction_active: mode_is_group(&options.mode)
                && params.easy_sym_bomb
                || (!matches!(options.mode, PunchMode::Ports) && range > 0),
            success: false,
        };
        // 对应 this.result = new Promise(...)：命中/失败/取消都从这里出
        let (tx, rx) = std::sync::mpsc::channel();
        Puncher {
            inner: Arc::new(Inner {
                sockets: if sockets.is_empty() { vec![conn.clone()] } else { sockets },
                conn,
                nonce: punch_random_nonce(),
                profile,
                params,
                timeout_ms,
                options,
                state: Mutex::new(State {
                    active: false,
                    settled: false,
                    remote: None,
                    ports: Vec::new(),
                    stats,
                    started_at: 0.0,
                    spray_table: None,
                    spray_cursor: 0,
                }),
                cancel: AtomicBool::new(false),
                result: Mutex::new(Some(tx)),
                waiter: Mutex::new(Some(rx)),
                answer: Mutex::new(None),
            }),
        }
    }

    /// 对应原版 wait()：命中回 PunchAddress，未命中/取消回 PunchFailure。
    /// Promise 可以被 await 多次，这里用 answer 缓存首次结果。
    pub fn wait(&self) -> Result<PunchAddress, PunchFailure> {
        if let Some(answer) = self.inner.answer.lock().unwrap().clone() {
            return answer;
        }
        let answer = match self.inner.waiter.lock().unwrap().take() {
            Some(rx) => rx.recv().unwrap_or_else(|_| Err(PunchFailure { result: self.stats(), message: "打洞已取消".into() })),
            None => Err(PunchFailure { result: self.stats(), message: "打洞已取消".into() }),
        };
        *self.inner.answer.lock().unwrap() = Some(answer.clone());
        answer
    }

    pub fn nonce(&self) -> u16 {
        self.inner.nonce
    }

    pub fn set_target(&self, address: Option<PunchAddress>) {
        self.inner.state.lock().unwrap().remote = address;
    }

    pub fn target(&self) -> Option<PunchAddress> {
        self.inner.state.lock().unwrap().remote.clone()
    }

    pub fn set_predicted_ports(&self, ports: Vec<i64>) {
        let mut seen = HashSet::new();
        self.inner.state.lock().unwrap().ports = ports
            .into_iter()
            .filter(|p| seen.insert(*p) && *p > 0 && *p <= 65535)
            .collect();
    }

    pub fn state_ports(&self) -> Vec<i64> {
        self.inner.state.lock().unwrap().ports.clone()
    }

    pub fn stats(&self) -> PunchResult {
        self.inner.state.lock().unwrap().stats.clone()
    }

    /// 对应原版 start()：挂监听 + 超时看门狗 + 发送循环
    pub fn start(&self) {
        {
            let mut st = self.inner.state.lock().unwrap();
            if st.active || st.settled {
                return;
            }
            st.active = true;
            st.started_at = elapsed_ms();
        }
        for index in 0..self.inner.sockets.len() {
            let inner = self.inner.clone();
            let socket = self.inner.sockets[index].clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 2048];
                loop {
                    if inner.cancel.load(Ordering::SeqCst) {
                        return;
                    }
                    let _ = socket.set_read_timeout(Some(Duration::from_millis(50)));
                    match socket.recv_from(&mut buf) {
                        Ok((size, from)) => {
                            message(&inner, &socket, &buf[..size], from);
                        }
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => {}
                        Err(_) => return,
                    }
                }
            });
        }
        let inner = self.inner.clone();
        let extra = if matches!(self.inner.options.mode, PunchMode::Ports) {
            self.inner.profile.send.extra_wait_long_ms
        } else {
            self.inner.profile.send.extra_wait_ms
        } as f64;
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis((inner.timeout_ms + extra) as u64));
            fail(&inner, false);
        });
        let inner = self.inner.clone();
        std::thread::spawn(move || {
            send_loop(&inner);
        });
    }

    pub fn stop(&self) {
        cleanup(&self.inner);
        let mut st = self.inner.state.lock().unwrap();
        if !st.settled {
            st.settled = true;
            let stats = st.stats.clone();
            drop(st);
            if let Some(tx) = self.inner.result.lock().unwrap().take() {
                let _ = tx.send(Err(PunchFailure { result: stats, message: "打洞已取消".into() }));
            }
        }
    }
}

fn cleanup(inner: &Arc<Inner>) {
    inner.state.lock().unwrap().active = false;
    inner.cancel.store(true, Ordering::SeqCst);
}

fn send_pkt(socket: &Arc<UdpSocket>, packet: &[u8], remote: &PunchAddress) -> bool {
    // UdpHolePuncher.java sendPkt：三连发清掉 Windows 的 ICMP 锁存
    for _ in 0..3 {
        if let Ok(addr) = remote.address.parse::<IpAddr>() {
            if socket.send_to(packet, (addr, remote.port)).is_ok() {
                return true;
            }
        } else if socket.send_to(packet, (remote.address.as_str(), remote.port)).is_ok() {
            return true;
        }
    }
    false
}

fn message(inner: &Arc<Inner>, socket: &Arc<UdpSocket>, packet: &[u8], from: std::net::SocketAddr) {
    let mut st = inner.state.lock().unwrap();
    if !st.active || st.settled {
        return;
    }
    let verified = match verify_punch_frame(packet, inner.options.auth_key.as_ref()) {
        Some(v) => v,
        None => return,
    };
    let frame = match punch_parse_control(&verified) {
        Some(f) => f,
        None => return,
    };
    let actual = PunchAddress { address: from.ip().to_string(), port: from.port() };
    if !punch_accept_source(st.remote.as_ref(), Some(&actual), inner.options.auth_key.is_some()) {
        return;
    }
    if frame.0 == 1 {
        st.stats.sockets_received_punch += 1.0;
    } else {
        st.stats.sockets_received_ack += 1.0;
    }
    st.remote = Some(actual.clone());
    st.stats.success = true;
    st.stats.elapsed_ms = elapsed_ms() - st.started_at;
    st.settled = true;
    let stats = st.stats.clone();
    let nonce = inner.nonce;
    let key = inner.options.auth_key;
    drop(st);
    let ack = sign_punch_frame(&punch_build_control(2, nonce), key.as_ref());
    send_pkt(socket, &ack, &actual);
    inner.cancel.store(true, Ordering::SeqCst);
    inner.state.lock().unwrap().active = false;
    if let Some(tx) = inner.result.lock().unwrap().take() {
        let _ = tx.send(Ok(actual));
    }
    let _ = stats;
}

fn fail(inner: &Arc<Inner>, firewall: bool) {
    let mut st = inner.state.lock().unwrap();
    if st.settled {
        return;
    }
    st.stats.elapsed_ms = elapsed_ms() - st.started_at;
    st.stats.firewall_detected = firewall;
    st.settled = true;
    let stats = st.stats.clone();
    drop(st);
    inner.cancel.store(true, Ordering::SeqCst);
    st = inner.state.lock().unwrap();
    st.active = false;
    drop(st);
    if let Some(tx) = inner.result.lock().unwrap().take() {
        let message = if stats.firewall_detected { "本轮未收到 UDP 回包" } else { "本轮打洞未命中" };
        let _ = tx.send(Err(PunchFailure { result: stats, message: message.into() }));
    }
}

fn sleep_cancelable(inner: &Arc<Inner>, ms: f64) {
    let deadline = Instant::now() + Duration::from_millis(ms.max(0.0) as u64);
    while Instant::now() < deadline {
        if inner.cancel.load(Ordering::SeqCst) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn send_control(inner: &Arc<Inner>, socket: &Arc<UdpSocket>, port: i64, limiter: Option<&mut PpsLimiter>) {
    let remote = {
        let st = inner.state.lock().unwrap();
        if !st.active {
            return;
        }
        match &st.remote {
            Some(r) => PunchAddress { address: r.address.clone(), port: r.port },
            None => return,
        }
    };
    if port < 1 || port > 65535 {
        return;
    }
    if let Some(limiter) = limiter {
        let wait = limiter.before_send_delay();
        if wait > 0.0 {
            sleep_cancelable(inner, wait);
        }
    }
    let st = inner.state.lock().unwrap();
    if !st.active {
        return;
    }
    let nonce = inner.nonce;
    let key = inner.options.auth_key;
    drop(st);
    let frame = sign_punch_frame(&punch_build_control(1, nonce), key.as_ref());
    let target = PunchAddress { address: remote.address, port: port as u16 };
    send_pkt(socket, &frame, &target);
}

fn spray(inner: &Arc<Inner>, round: i64, limiter: &mut PpsLimiter) {
    let p = inner.params.clone();
    let random: Box<Rng> = Box::new(|| rand::Rng::random::<f64>(&mut rand::rng()));
    {
        let mut st = inner.state.lock().unwrap();
        if st.spray_table.is_none() {
            let mut table: Vec<i64> = (1..=65535i64).collect();
            shuffle(&mut table, &*random);
            st.spray_table = Some(table);
        }
    }
    let remote_port = {
        let st = inner.state.lock().unwrap();
        st.remote.as_ref().map(|r| r.port as i64)
    };
    if let Some(port) = remote_port {
        for _ in 0..p.spray_packets_per_port as i64 {
            if !inner.state.lock().unwrap().active {
                return;
            }
            send_control(inner, &inner.conn, port, Some(limiter));
        }
    }
    let base = p.spray_port_count_min + (random() * (p.spray_port_count_max - p.spray_port_count_min + 1.0).max(1.0)).floor();
    let count = if round > 2 {
        (base * p.spray_decay_numerator / round as f64).trunc().max(p.spray_decay_floor)
    } else {
        base
    };
    let mut i = 0i64;
    while i < count as i64 {
        if !inner.state.lock().unwrap().active {
            return;
        }
        let port = {
            let mut st = inner.state.lock().unwrap();
            let length = st.spray_table.as_ref().expect("spray 表已建").len();
            let index = st.spray_cursor % length;
            st.spray_cursor += 1;
            st.spray_table.as_ref().expect("spray 表已建")[index]
        };
        let socket = inner.sockets[(i as usize) % inner.sockets.len()].clone();
        for _ in 0..p.spray_packets_per_port as i64 {
            if !inner.state.lock().unwrap().active {
                return;
            }
            send_control(inner, &socket, port, Some(limiter));
        }
        sleep_cancelable(inner, p.spray_port_interval_ms.max(0.0));
        i += 1;
    }
}

fn send_loop(inner: &Arc<Inner>) {
    let p = inner.params.clone();
    let s = inner.profile.send.clone();
    let profile = inner.profile.clone();
    let group = mode_is_group(&inner.options.mode);
    let interval = if group && p.easy_sym_bomb {
        p.send_interval.min(1.0f64.max(p.bomb_round_interval_ms))
    } else {
        p.send_interval
    };
    let max_cycles = (inner.timeout_ms / interval.max(1.0)).trunc() as i64;
    let mut limiter = PpsLimiter::new(profile.sym.max_pps as f64);
    let random: Box<Rng> = Box::new(|| rand::Rng::random::<f64>(&mut rand::rng()));
    let mut cycle = 0i64;
    while cycle < max_cycles {
        if !inner.state.lock().unwrap().active {
            return;
        }
        let remote = inner.state.lock().unwrap().remote.clone();
        let Some(remote) = remote else {
            sleep_cancelable(inner, interval);
            cycle += 1;
            continue;
        };
        if group {
            let sockets_len = inner.sockets.len();
            if sockets_len > 1
                && !inner.options.skip_firewall
                && cycle >= profile.firewall_detect_cycles as i64
                && inner.state.lock().unwrap().stats.sockets_received_punch == 0.0
            {
                fail(inner, true);
                return;
            }
            if p.hard_sym_spray {
                spray(inner, cycle + 1, &mut limiter);
                sleep_cancelable(inner, interval);
                cycle += 1;
                continue;
            }
            if p.easy_sym_bomb {
                for socket in inner.sockets.clone() {
                    let mut off = -(p.bomb_window as i64);
                    while off <= p.bomb_window as i64 {
                        if !inner.state.lock().unwrap().active {
                            return;
                        }
                        let port = (1i64).max(65535i64.min(remote.port as i64 + off));
                        send_control(inner, &socket, port, Some(&mut limiter));
                        off += 1;
                    }
                }
            } else {
                let spread = inner.options.sweep_spread.unwrap_or(0);
                for (i, socket) in inner.sockets.clone().into_iter().enumerate() {
                    let port = remote.port as i64 + if spread > 0 { (i as i64) % (spread * 2 + 1) - spread } else { 0 };
                    for _ in 0..3 {
                        if !inner.state.lock().unwrap().active {
                            return;
                        }
                        send_control(inner, &socket, port, None);
                    }
                }
            }
            let bomb_active = p.easy_sym_bomb && (elapsed_ms() - inner.state.lock().unwrap().started_at) < p.bomb_duration_ms;
            sleep_cancelable(inner, if bomb_active { p.bomb_round_interval_ms } else { p.send_interval });
        } else if matches!(inner.options.mode, PunchMode::Ports) {
            let ports = inner.state.lock().unwrap().ports.clone();
            for port in ports {
                if !inner.state.lock().unwrap().active {
                    return;
                }
                send_control(inner, &inner.conn, port, None);
            }
            sleep_cancelable(inner, p.send_interval);
        } else {
            let range = inner.options.range.unwrap_or(0);
            if range > 0 {
                let progressive_index = ((cycle / profile.cycles_per_range.max(1) as i64) as usize).min(profile.progressive_ranges.len() - 1);
                let progressive = profile.progressive_ranges[progressive_index] as i64;
                let current_range = if inner.options.fixed_range { range } else { (if range == 0 { p.port_range as i64 } else { range }).min(progressive) };
                let ports = control_ports(remote.port as i64, current_range, s.sweep_window_size as i64, &*random);
                let passes = if p.send_min_rounds > 0.0 { p.send_min_rounds as i64 } else { s.min_rounds as i64 };
                let reps = if p.send_min_pass > 0.0 { p.send_min_pass as i64 } else { s.min_pass as i64 };
                let mut pass = 0i64;
                while pass < passes {
                    for i in 0..ports.len() {
                        if !inner.state.lock().unwrap().active {
                            return;
                        }
                        for _ in 0..reps {
                            send_control(inner, &inner.conn, ports[i], None);
                        }
                        if i + 1 < ports.len() {
                            sleep_cancelable(inner, s.sleep_short_ms as f64);
                        }
                    }
                    if pass < 2 {
                        sleep_cancelable(inner, s.sleep_long_ms as f64);
                    }
                    pass += 1;
                }
            } else {
                send_control(inner, &inner.conn, remote.port as i64, None);
            }
            sleep_cancelable(inner, p.send_interval);
        }
        cycle += 1;
    }
    fail(inner, false);
}

/// 对应 punchListen：优先绑指定端口，失败回落随机端口
pub fn punch_listen(preferred_port: i64) -> Arc<UdpSocket> {
    if (1..=65535).contains(&preferred_port) {
        if let Ok(socket) = UdpSocket::bind(("0.0.0.0", preferred_port as u16)) {
            return Arc::new(socket);
        }
    }
    Arc::new(UdpSocket::bind("0.0.0.0:0").expect("无法绑定临时 UDP 端口"))
}


#[cfg(test)]
mod tests {
    use super::*;

    fn seeded(seed: u64) -> impl Fn() -> f64 {
        let state = std::sync::atomic::AtomicU64::new(seed);
        move || {
            let x = state.fetch_update(
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
                |v| Some(v.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407)),
            ).unwrap();
            ((x >> 11) as f64) / ((1u64 << 53) as f64)
        }
    }

    fn addr(ip: &str, port: u16) -> PunchAddress {
        PunchAddress { address: ip.into(), port }
    }

    #[test]
    fn control_frame_is_five_bytes_with_big_endian_nonce() {
        let frame = punch_build_control(1, 0xABCD);
        assert_eq!(frame, [86, 76, 1, 0xAB, 0xCD]);
        assert_eq!(punch_parse_control(&frame), Some((1, 0xABCD)));
        assert_eq!(punch_parse_control(&punch_build_control(2, 7)), Some((2, 7)));
        assert_eq!(punch_parse_control(&[86, 76, 3, 0, 7]), None);
        assert_eq!(punch_parse_control(&[86, 76, 1, 0]), None);
        assert_eq!(punch_parse_control(&[0, 0, 1, 0, 7]), None);
    }

    #[test]
    fn source_accept_follows_cgnat_rule_only_when_unauthenticated() {
        assert!(punch_accept_source(Some(&addr("10.20.30.40", 1)), Some(&addr("10.20.30.40", 9)), false));
        assert!(punch_accept_source(Some(&addr("10.20.1.1", 1)), Some(&addr("10.20.99.99", 1)), false));
        assert!(!punch_accept_source(Some(&addr("10.20.1.1", 1)), Some(&addr("10.21.1.1", 1)), false));
        assert!(!punch_accept_source(Some(&addr("10.20.1.1", 1)), Some(&addr("10.20.99.99", 1)), true));
        assert!(!punch_accept_source(None, Some(&addr("1.1.1.1", 1)), false));
        assert!(!punch_accept_source(Some(&addr("1.1.1.1", 1)), None, false));
        // 非 IPv4 不能走 /16 例外
        assert!(!punch_accept_source(Some(&addr("fe80::1", 1)), Some(&addr("fe80::2", 1)), false));
    }

    #[test]
    fn small_range_scan_is_symmetric_and_clamped() {
        let rng = seeded(7);
        let ports = control_ports(25565, 3, 800, &rng);
        assert_eq!(ports, vec![25565, 25564, 25566, 25563, 25567, 25562, 25568]);
        let edge = control_ports(2, 3, 800, &rng);
        assert!(!edge.contains(&-1), "不能出现非法端口");
        assert!(!edge.contains(&0));
        assert!(edge.contains(&1) && edge.contains(&3) && edge.contains(&5));
        let top = control_ports(65535, 2, 800, &rng);
        assert!(!top.iter().any(|p| *p > 65535));
        assert!(top.contains(&65533));
    }

    #[test]
    fn wide_range_scan_keeps_duplicate_center_and_stays_in_window() {
        let rng = seeded(11);
        let ports = control_ports(25565, 100, 20, &rng);
        assert_eq!(ports[0], 25565);
        // 中心端口在集合里出现两次（原版注释明确要求保留这个重复）
        assert_eq!(ports.iter().filter(|p| **p == 25565).count(), 2);
        assert_eq!(ports.len(), 1 + 21);
        for port in &ports[1..] {
            assert!(*port >= 25465 && *port <= 25665, "越界: {}", port);
        }
        let near_edge = control_ports(10, 100, 30, &rng);
        for port in &near_edge[1..] {
            assert!(*port >= 1 && *port <= 110);
        }
    }

    #[test]
    fn shuffle_is_a_permutation() {
        let rng = seeded(3);
        let mut values: Vec<i64> = (0..50).collect();
        let before: Vec<i64> = values.iter().cloned().collect();
        shuffle(&mut values, &rng);
        assert_ne!(values, before, "应当被打乱");
        let mut sorted = values.clone();
        sorted.sort();
        assert_eq!(sorted, before);
    }

    #[test]
    fn pps_limiter_delays_when_bursting() {
        let mut limiter = PpsLimiter::new(2.0);
        let first = limiter.before_send_delay();
        let second = limiter.before_send_delay();
        let third = limiter.before_send_delay();
        assert!(first >= 0.0 && second >= 0.0 && third >= 0.0);
        assert!(third > 0.0, "2pps 下连发三包必须补睡");
    }

    #[test]
    fn listen_prefers_port_and_falls_back() {
        let socket = punch_listen(0);
        let port = socket.local_addr().unwrap().port();
        assert!(port > 0);
        let taken = punch_listen(port.max(10200) as i64);
        let first = taken.local_addr().unwrap().port();
        let again = punch_listen(first as i64);
        // 端口已被占用时必须回落到随机端口，而不是失败
        assert_ne!(again.local_addr().unwrap().port(), first);
    }

    #[test]
    fn prediction_active_matches_mode_matrix() {
        let socket = Arc::new(UdpSocket::bind("0.0.0.0:0").unwrap());
        let build = |mode: PunchMode, range: i64, bomb: bool| {
            let mut profile = DEFAULT.clone();
            let mut params = from_profile(&profile);
            params.easy_sym_bomb = bomb;
            profile.send.interval_ms = 200;
            Puncher::new(
                socket.clone(),
                vec![socket.clone()],
                PuncherOptions { mode, range: Some(range), params: Some(params), profile: Some(profile), ..Default::default() },
            )
            .stats()
            .port_prediction_active
        };
        assert!(build(PunchMode::Prediction, 50, false));
        assert!(!build(PunchMode::Prediction, 0, false));
        assert!(!build(PunchMode::Ports, 50, false));
        assert!(build(PunchMode::Group, 0, true));
        assert!(!build(PunchMode::Group, 0, false));
    }

    #[test]
    fn predicted_ports_are_deduped_and_bounded() {
        let socket = Arc::new(UdpSocket::bind("0.0.0.0:0").unwrap());
        let puncher = Puncher::new(socket.clone(), vec![], PuncherOptions::default());
        puncher.set_predicted_ports(vec![1, 1, 2, 0, 70000, 3, -5]);
        assert_eq!(puncher.state_ports(), vec![1, 2, 3]);
    }
}
