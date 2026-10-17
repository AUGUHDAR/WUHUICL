//! 可靠 UDP 传输（对应原版 voxlink/rudp.ts 209 行，LGPL-3.0-only）
//! 线上格式对齐 VoxLink ReliableUdpTransport.java @6b11d93。

use crate::vl::punchauth::{sign_punch_frame, verify_punch_frame};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const RUDP_TYPE_PUNCH: u8 = 1;
pub const RUDP_TYPE_PUNCH_ACK: u8 = 2;
pub const RUDP_TYPE_DATA: u8 = 3;
pub const RUDP_TYPE_ACK: u8 = 4;
pub const RUDP_TYPE_DISCONNECT: u8 = 7;
pub const RUDP_TYPE_KEEPALIVE: u8 = 8;
pub const RUDP_TYPE_FEC_XOR: u8 = 9;
pub const RUDP_TYPE_RESTART: u8 = 10;
pub const RUDP_TYPE_VOICE: u8 = 11;

const WINDOW_PACKETS: usize = 64;
const REORDER_WINDOW: u32 = 512;
const CACHE_BYTE_LIMIT: usize = 8 * 1024 * 1024;
const INCOMING_LIMIT: usize = 512;
const FEC_GROUP_LIMIT: usize = 128;
const FEC_GROUP_TTL: f64 = 24000.0;
const CHUNK_PLAIN: usize = 1400;
const CHUNK_CODECCED: usize = 1374;
const IDLE_TIMEOUT: f64 = 60000.0;
const PACKET_TIMEOUT: f64 = 24000.0;
const KEEPALIVE_MS: f64 = 1000.0;
const DRIFT_SILENT_MS: f64 = 6000.0;
const DRIFT_CONFIRM_MS: f64 = 5000.0;
const PROMOTE_GRACE_MS: f64 = 10000.0;
const DUP_ACK_MIN_INTERVAL: f64 = 50.0;

pub fn now_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or_default()
}

pub fn seq_diff(a: u32, b: u32) -> u32 {
    a.wrapping_sub(b)
}

pub fn seq_after(a: u32, b: u32) -> bool {
    let d = seq_diff(a, b);
    d > 0 && d < 0x8000_0000
}

#[derive(Clone, Debug, PartialEq)]
pub struct RudpFrame {
    pub rtype: u8,
    pub seq: u32,
    pub ack: u32,
    pub payload: Vec<u8>,
    pub fec_count: u8,
    pub fec_lengths: Vec<u16>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RudpTarget {
    pub address: String,
    pub port: u16,
}

pub trait RudpCodec: Send + Sync {
    fn encode(&self, frame: &[u8]) -> Vec<u8>;
    fn decode(&self, packet: &[u8]) -> Option<Vec<u8>>;
}

#[derive(Clone, Debug)]
pub enum RudpEvent {
    Closed(String),
    Restart,
    Voice(Vec<u8>),
    AuthDowngrade,
}

pub fn rudp_encode(f: &RudpFrame) -> Vec<u8> {
    let count = if f.rtype == RUDP_TYPE_FEC_XOR {
        f.fec_lengths.len()
    } else {
        0
    };
    let offset = if f.rtype == RUDP_TYPE_FEC_XOR {
        14 + count * 2
    } else if f.rtype == RUDP_TYPE_DATA {
        13
    } else {
        11
    };
    let mut packet = vec![0u8; offset + f.payload.len()];
    packet[0] = 86;
    packet[1] = 76;
    packet[2] = f.rtype;
    packet[3..7].copy_from_slice(&f.seq.to_be_bytes());
    packet[7..11].copy_from_slice(&f.ack.to_be_bytes());
    if f.rtype == RUDP_TYPE_DATA || f.rtype == RUDP_TYPE_FEC_XOR {
        packet[11..13].copy_from_slice(&(f.payload.len() as u16).to_be_bytes());
    }
    if f.rtype == RUDP_TYPE_FEC_XOR {
        packet[13] = count as u8;
        for (i, size) in f.fec_lengths.iter().enumerate() {
            packet[14 + i * 2..16 + i * 2].copy_from_slice(&size.to_be_bytes());
        }
    }
    packet[offset..].copy_from_slice(&f.payload);
    packet
}

pub fn rudp_decode(packet: &[u8]) -> Option<RudpFrame> {
    if packet.len() < 11 || packet[0] != 86 || packet[1] != 76 {
        return None;
    }
    let rtype = packet[2];
    let mut offset = 11usize;
    let mut length = packet.len() - 11;
    let mut count = 0u8;
    let mut lengths: Vec<u16> = Vec::new();
    if rtype == RUDP_TYPE_DATA || rtype == RUDP_TYPE_FEC_XOR {
        if packet.len() < 13 {
            return None;
        }
        length = u16::from_be_bytes([packet[11], packet[12]]) as usize;
        offset = 13;
    }
    if rtype == RUDP_TYPE_FEC_XOR {
        if packet.len() < 14 {
            return None;
        }
        count = packet[13];
        if count < 1 || count > 20 {
            return None;
        }
        offset = 14 + count as usize * 2;
        if packet.len() < offset {
            return None;
        }
        for i in 0..count as usize {
            lengths.push(u16::from_be_bytes([packet[14 + i * 2], packet[15 + i * 2]]));
        }
    }
    if length > 1400 || packet.len() != offset + length {
        return None;
    }
    Some(RudpFrame {
        rtype,
        seq: u32::from_be_bytes([packet[3], packet[4], packet[5], packet[6]]),
        ack: u32::from_be_bytes([packet[7], packet[8], packet[9], packet[10]]),
        payload: packet[offset..].to_vec(),
        fec_count: count,
        fec_lengths: lengths,
    })
}

struct PendingPacket {
    data: Vec<u8>,
    sent: f64,
    created: f64,
    retries: u32,
}

struct FecGroup {
    data: Vec<(u32, Vec<u8>)>,
    parity: Option<Vec<u8>>,
    lengths: Option<Vec<u16>>,
    updated: f64,
}

struct Path {
    socket: Arc<UdpSocket>,
    remote: RudpTarget,
    codec: Option<Arc<dyn RudpCodec>>,
    owns: bool,
    last_rx: f64,
    rx: u64,
    last_current: f64,
    pending_port: i32,
    pending_at: f64,
    attached: bool,
}

struct State {
    running: bool,
    ended: bool,
    last_rx: f64,
    last_ping: f64,
    next_send: u32,
    next_read: u32,
    /// 原版是 Map，重传遍历按插入序，这里用 Vec 保序
    pending: Vec<(u32, PendingPacket)>,
    fec_send: Vec<Vec<u8>>,
    fec_receive: Vec<(u32, FecGroup)>,
    srtt: f64,
    rttvar: f64,
    rto: f64,
    last_ack: i64,
    duplicate_acks: u32,
    reordered: HashMap<u32, Vec<u8>>,
    incoming: Vec<Vec<u8>>,
    queued_bytes: usize,
    auth_key: Option<[u8; 32]>,
    allow_turn_auth_downgrade: bool,
    auth_drops: u32,
    paths: Vec<Path>,
    primary: Option<usize>,
    secondary: Option<usize>,
    grace_until: f64,
}

struct Outbound {
    socket: Arc<UdpSocket>,
    addr: SocketAddr,
    bytes: Vec<u8>,
}

fn flush(outs: Vec<Outbound>) {
    for o in outs {
        let _ = o.socket.send_to(&o.bytes, o.addr);
    }
}

fn target_addr(remote: &RudpTarget) -> Option<SocketAddr> {
    let ip: IpAddr = remote.address.parse().ok()?;
    Some(SocketAddr::new(ip, remote.port))
}

/// 锁内只构造报文，发送一律放到锁外，避免与对端回包处理互相等锁
fn build_send(
    st: &mut State,
    outs: &mut Vec<Outbound>,
    rtype: u8,
    payload: Vec<u8>,
    seq: u32,
    lengths: Vec<u16>,
) {
    if st.ended || st.primary.is_none() {
        return;
    }
    let frame = rudp_encode(&RudpFrame {
        rtype,
        seq,
        ack: if rtype == RUDP_TYPE_FEC_XOR {
            0
        } else {
            st.next_read
        },
        payload,
        fec_count: lengths.len() as u8,
        fec_lengths: lengths,
    });
    let packet = sign_punch_frame(&frame, st.auth_key.as_ref());
    let now = now_ms();
    let grace = st.grace_until;
    let want_secondary = st
        .secondary
        .filter(|_| now < grace || rtype == RUDP_TYPE_KEEPALIVE);
    let primary = st.primary.unwrap();
    let mut indices = vec![primary];
    if let Some(s) = want_secondary {
        indices.push(s);
    }
    for idx in indices {
        let path = &st.paths[idx];
        let wire = match &path.codec {
            Some(c) => c.encode(&packet),
            None => packet.clone(),
        };
        if let Some(addr) = target_addr(&path.remote) {
            outs.push(Outbound {
                socket: path.socket.clone(),
                addr,
                bytes: wire,
            });
        }
    }
}

fn send_one(st: &mut State, outs: &mut Vec<Outbound>, idx: usize, bytes: Vec<u8>) {
    let path = &st.paths[idx];
    let wire = match &path.codec {
        Some(c) => c.encode(&bytes),
        None => bytes.clone(),
    };
    if let Some(addr) = target_addr(&path.remote) {
        outs.push(Outbound {
            socket: path.socket.clone(),
            addr,
            bytes: wire,
        });
    }
}

#[derive(Default)]
struct Wake {
    gen: Mutex<u64>,
    cv: Condvar,
}

impl Wake {
    fn bump(&self) {
        *self.gen.lock().unwrap() += 1;
        self.cv.notify_all();
    }

    fn wait_while(&self, mut busy: impl FnMut() -> bool) {
        let mut g = self.gen.lock().unwrap();
        while busy() {
            let (next, _) = self.cv.wait_timeout(g, Duration::from_millis(20)).unwrap();
            g = next;
        }
    }
}

struct Inner {
    st: Mutex<State>,
    wake: Wake,
    writer_lock: Mutex<()>,
    reader_lock: Mutex<()>,
    events: Mutex<Option<Sender<RudpEvent>>>,
    on_closed: Mutex<Option<Box<dyn FnMut(String) + Send + 'static>>>,
    rx_tx: Mutex<Option<Sender<(usize, Vec<u8>, SocketAddr)>>>,
}

pub struct RudpConn {
    inner: Arc<Inner>,
    loop_thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

fn emit(inner: &Arc<Inner>, ev: RudpEvent) {
    if let Some(tx) = inner.events.lock().unwrap().as_ref() {
        let _ = tx.send(ev);
    }
}

fn raise_recv_buffer(socket: &UdpSocket) {
    // 64 包窗口 + 校验会顶爆 Windows 默认 64KiB UDP 队列；std 没有取/设缓冲的 API
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawSocket;
        use windows_sys::Win32::Networking::WinSock::{setsockopt, SOL_SOCKET, SO_RCVBUF};
        const TARGET: u32 = 256 * 1024;
        let mut want: u32 = TARGET;
        unsafe {
            setsockopt(
                socket.as_raw_socket() as usize,
                SOL_SOCKET,
                SO_RCVBUF,
                &mut want as *mut u32 as windows_sys::core::PCSTR,
                4,
            );
        }
    }
    #[cfg(not(windows))]
    {
        if socket
            .recv_buffer_size()
            .map(|v| v < 256 * 1024)
            .unwrap_or(false)
        {
            let _ = socket.set_recv_buffer_size(256 * 1024);
        }
    }
}

impl RudpConn {
    pub fn new(
        socket: Arc<UdpSocket>,
        remote: Option<RudpTarget>,
        codec: Option<Arc<dyn RudpCodec>>,
        auth_key: Option<[u8; 32]>,
        owns_socket: bool,
        allow_turn_auth_downgrade: bool,
    ) -> Self {
        let mut paths = Vec::new();
        let primary = remote.map(|r| {
            paths.push(Path {
                socket,
                remote: r,
                codec,
                owns: owns_socket,
                last_rx: 0.0,
                rx: 0,
                last_current: 0.0,
                pending_port: -1,
                pending_at: 0.0,
                attached: false,
            });
            paths.len() - 1
        });
        let inner = Arc::new(Inner {
            st: Mutex::new(State {
                running: false,
                ended: false,
                last_rx: now_ms(),
                last_ping: 0.0,
                next_send: 0,
                next_read: 0,
                pending: Vec::new(),
                fec_send: Vec::new(),
                fec_receive: Vec::new(),
                srtt: -1.0,
                rttvar: 0.0,
                rto: 200.0,
                last_ack: -1,
                duplicate_acks: 0,
                reordered: HashMap::new(),
                incoming: Vec::new(),
                queued_bytes: 0,
                auth_key,
                allow_turn_auth_downgrade,
                auth_drops: 0,
                paths,
                primary,
                secondary: None,
                grace_until: 0.0,
            }),
            wake: Wake::default(),
            writer_lock: Mutex::new(()),
            reader_lock: Mutex::new(()),
            events: Mutex::new(None),
            on_closed: Mutex::new(None),
            rx_tx: Mutex::new(None),
        });
        RudpConn {
            inner,
            loop_thread: Mutex::new(None),
        }
    }

    pub fn events(&self) -> Receiver<RudpEvent> {
        let (tx, rx) = mpsc::channel();
        *self.inner.events.lock().unwrap() = Some(tx);
        rx
    }

    pub fn set_on_closed(&self, f: impl FnMut(String) + Send + 'static) {
        *self.inner.on_closed.lock().unwrap() = Some(Box::new(f));
    }

    pub fn is_connected(&self) -> bool {
        let st = self.inner.st.lock().unwrap();
        st.running && !st.ended
    }

    pub fn get_remote(&self) -> Option<RudpTarget> {
        let st = self.inner.st.lock().unwrap();
        st.primary.map(|i| st.paths[i].remote.clone())
    }

    pub fn add_secondary_path(
        &self,
        socket: Arc<UdpSocket>,
        remote: RudpTarget,
        codec: Option<Arc<dyn RudpCodec>>,
    ) -> bool {
        let mut st = self.inner.st.lock().unwrap();
        if st.ended || st.secondary.is_some() {
            return false;
        }
        st.paths.push(Path {
            socket,
            remote,
            codec,
            owns: true,
            last_rx: 0.0,
            rx: 0,
            last_current: 0.0,
            pending_port: -1,
            pending_at: 0.0,
            attached: false,
        });
        let idx = st.paths.len() - 1;
        st.secondary = Some(idx);
        if st.running {
            drop(st);
            self.attach(idx);
        }
        true
    }

    pub fn secondary_health(&self) -> Option<(u64, f64)> {
        let st = self.inner.st.lock().unwrap();
        st.secondary.map(|i| (st.paths[i].rx, st.paths[i].last_rx))
    }

    pub fn promote_secondary_path(&self) -> bool {
        let mut st = self.inner.st.lock().unwrap();
        if st.secondary.is_none() || st.primary.is_none() {
            return false;
        }
        let sec = st.secondary.unwrap();
        let pri = st.primary.unwrap();
        st.primary = Some(sec);
        st.secondary = Some(pri);
        st.grace_until = now_ms() + PROMOTE_GRACE_MS;
        true
    }

    pub fn drop_secondary_path(&self) {
        let path = {
            let mut st = self.inner.st.lock().unwrap();
            let Some(i) = st.secondary else { return };
            st.secondary = None;
            st.grace_until = 0.0;
            st.paths[i].attached = false;
            (st.paths[i].owns, Some(st.paths[i].socket.clone()))
        };
        if let (true, Some(socket)) = path {
            drop(socket);
        }
    }

    fn attach(&self, idx: usize) {
        {
            let mut st = self.inner.st.lock().unwrap();
            if st.paths[idx].attached || st.ended {
                return;
            }
            st.paths[idx].attached = true;
            raise_recv_buffer(&st.paths[idx].socket);
        }
        let (tx, rx) = {
            let mut guard = self.inner.rx_tx.lock().unwrap();
            if guard.is_none() {
                let (tx, rx) = mpsc::channel();
                *guard = Some(tx);
                let inner = self.inner.clone();
                std::thread::spawn(move || pump(inner, rx));
            }
            (
                guard.clone(),
                None::<Receiver<(usize, Vec<u8>, SocketAddr)>>,
            )
        };
        let Some(tx) = tx else { return };
        let socket = self.inner.st.lock().unwrap().paths[idx].socket.clone();
        let mut buf = vec![0u8; 2048];
        std::thread::spawn(move || loop {
            let _ = socket.set_read_timeout(Some(Duration::from_millis(50)));
            match socket.recv_from(&mut buf) {
                Ok((size, from)) => {
                    if tx.send((idx, buf[..size].to_vec(), from)).is_err() {
                        return;
                    }
                }
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut => {}
                Err(_) => return,
            }
        });
        let _ = rx;
    }

    pub fn start(&self) {
        {
            let mut st = self.inner.st.lock().unwrap();
            if st.running || st.ended {
                return;
            }
            st.running = true;
            st.last_rx = now_ms();
        }
        let primary = self.inner.st.lock().unwrap().primary;
        if let Some(i) = primary {
            self.attach(i);
        }
        let inner = self.inner.clone();
        let handle = std::thread::spawn(move || pump_loop(inner));
        *self.loop_thread.lock().unwrap() = Some(handle);
        let mut outs = Vec::new();
        let mut st = self.inner.st.lock().unwrap();
        build_send(
            &mut st,
            &mut outs,
            RUDP_TYPE_KEEPALIVE,
            Vec::new(),
            0,
            Vec::new(),
        );
        drop(st);
        flush(outs);
    }

    pub fn write(&self, data: &[u8]) -> Result<usize, String> {
        let _guard = self.inner.writer_lock.lock().unwrap();
        let copy = data.to_vec();
        let mut offset = 0usize;
        while offset < copy.len() {
            self.inner.wake.wait_while(|| {
                let st = self.inner.st.lock().unwrap();
                st.pending.len() >= WINDOW_PACKETS && !st.ended
            });
            let (seq, outs) = {
                let mut st = self.inner.st.lock().unwrap();
                if st.ended {
                    return Err("连接已关闭".into());
                }
                let limit = if st
                    .primary
                    .map(|i| st.paths[i].codec.is_some())
                    .unwrap_or(false)
                {
                    CHUNK_CODECCED
                } else {
                    CHUNK_PLAIN
                };
                let end = (offset + limit).min(copy.len());
                let part = copy[offset..end].to_vec();
                offset = end;
                let seq = st.next_send;
                st.next_send = seq.wrapping_add(1);
                let now = now_ms();
                st.pending.push((
                    seq,
                    PendingPacket {
                        data: part.clone(),
                        sent: now,
                        created: now,
                        retries: 0,
                    },
                ));
                let mut outs = Vec::new();
                build_send(
                    &mut st,
                    &mut outs,
                    RUDP_TYPE_DATA,
                    part.clone(),
                    seq,
                    Vec::new(),
                );
                // 原版 sendFec：攒满 4 片就发一条 XOR 校验
                st.fec_send.push(part);
                if st.fec_send.len() == 4 {
                    let lengths: Vec<u16> = st.fec_send.iter().map(|p| p.len() as u16).collect();
                    let max_len = lengths.iter().copied().max().unwrap_or(0) as usize;
                    let mut parity = vec![0u8; max_len];
                    for piece in &st.fec_send {
                        for (i, byte) in piece.iter().enumerate() {
                            parity[i] ^= *byte;
                        }
                    }
                    st.fec_send.clear();
                    build_send(
                        &mut st,
                        &mut outs,
                        RUDP_TYPE_FEC_XOR,
                        parity,
                        seq / 4,
                        lengths,
                    );
                }
                (seq, outs)
            };
            let _ = seq;
            flush(outs);
        }
        Ok(copy.len())
    }

    pub fn read(&self, target: &mut [u8]) -> Result<usize, String> {
        let _guard = self.inner.reader_lock.lock().unwrap();
        self.inner.wake.wait_while(|| {
            let st = self.inner.st.lock().unwrap();
            st.incoming.is_empty() && !st.ended
        });
        let size = {
            let mut st = self.inner.st.lock().unwrap();
            if st.incoming.is_empty() {
                return Ok(0);
            }
            let head = st.incoming[0].clone();
            let size = target.len().min(head.len());
            target[..size].copy_from_slice(&head[..size]);
            st.queued_bytes = st.queued_bytes.saturating_sub(size);
            if size == head.len() {
                st.incoming.remove(0);
            } else {
                st.incoming[0] = head[size..].to_vec();
            }
            drain_received(&mut st);
            size
        };
        let mut outs = Vec::new();
        let mut st = self.inner.st.lock().unwrap();
        build_send(&mut st, &mut outs, RUDP_TYPE_ACK, Vec::new(), 0, Vec::new());
        drop(st);
        flush(outs);
        Ok(size)
    }

    pub fn close(&self) {
        self.close_with("连接已关闭");
    }

    pub fn close_with(&self, reason: &str) {
        close_now(&self.inner, reason);
    }
}

fn drain_received(st: &mut State) {
    while st.incoming.len() < INCOMING_LIMIT {
        let Some(data) = st.reordered.remove(&st.next_read) else {
            break;
        };
        st.next_read = st.next_read.wrapping_add(1);
        st.queued_bytes += data.len();
        st.incoming.push(data);
    }
}

fn store_received(st: &mut State, seq: u32, data: &[u8]) -> bool {
    if seq_diff(seq, st.next_read) >= REORDER_WINDOW
        || st.reordered.contains_key(&seq)
        || st.queued_bytes + data.len() >= CACHE_BYTE_LIMIT
    {
        return false;
    }
    st.reordered.insert(seq, data.to_vec());
    drain_received(st);
    true
}

fn fec_group(st: &mut State, id: u32, now: f64) -> usize {
    if let Some(pos) = st.fec_receive.iter().position(|(k, _)| *k == id) {
        st.fec_receive[pos].1.updated = now;
        return pos;
    }
    st.fec_receive
        .retain(|(_, v)| now - v.updated <= FEC_GROUP_TTL);
    while st.fec_receive.len() > FEC_GROUP_LIMIT {
        st.fec_receive.remove(0);
    }
    st.fec_receive.push((
        id,
        FecGroup {
            data: Vec::new(),
            parity: None,
            lengths: None,
            updated: now,
        },
    ));
    st.fec_receive.len() - 1
}

fn recover_fec(inner: &Arc<Inner>, id: u32) {
    let mut st = inner.st.lock().unwrap();
    let Some(gi) = st.fec_receive.iter().position(|(k, _)| *k == id) else {
        return;
    };
    let group = &st.fec_receive[gi].1;
    let (Some(parity), Some(lengths)) = (group.parity.clone(), group.lengths.clone()) else {
        return;
    };
    if group.data.len() != 3 {
        return;
    }
    let data = group.data.clone();
    let mut missing = -1i32;
    for i in 0..4u32 {
        if !data
            .iter()
            .any(|(s, _)| *s == id.wrapping_mul(4).wrapping_add(i))
        {
            missing = i as i32;
        }
    }
    if missing < 0 {
        return;
    }
    let seq = id.wrapping_mul(4).wrapping_add(missing as u32);
    if seq_diff(seq, st.next_read) >= REORDER_WINDOW {
        return;
    }
    let mut recovered = parity.clone();
    for (s, chunk) in &data {
        let idx = (s.wrapping_rem(4)) as usize;
        if chunk.len() != lengths[idx] as usize {
            return;
        }
        for (i, byte) in chunk.iter().enumerate() {
            recovered[i] ^= *byte;
        }
    }
    let cut = (lengths[missing as usize] as usize).min(recovered.len());
    if store_received(&mut st, seq, &recovered[..cut]) {
        if let Some(pos) = st.fec_receive.iter().position(|(k, _)| *k == id) {
            st.fec_receive.remove(pos);
        }
        let mut outs = Vec::new();
        build_send(&mut st, &mut outs, RUDP_TYPE_ACK, Vec::new(), 0, Vec::new());
        drop(st);
        flush(outs);
    }
}

fn process_ack(st: &mut State, ack: u32, rtype: u8, now: f64, outs: &mut Vec<Outbound>) {
    if seq_after(ack, st.next_send) {
        return;
    }
    let mut done: Vec<u32> = Vec::new();
    for (seq, entry) in st.pending.iter() {
        if !seq_after(ack, *seq) {
            continue;
        }
        // Karn 规则：重传过的包不能当 RTT 样本
        if entry.retries == 0 {
            let sample = (now - entry.sent).max(1.0);
            if st.srtt < 0.0 {
                st.srtt = sample;
                st.rttvar = sample / 2.0;
            } else {
                st.rttvar = (3.0 * st.rttvar + (st.srtt - sample).abs()) / 4.0;
                st.srtt = (7.0 * st.srtt + sample) / 8.0;
            }
            st.rto = 100.0f64.max(800.0f64.min(st.srtt + 10.0f64.max(4.0 * st.rttvar)));
        }
        done.push(*seq);
    }
    if !done.is_empty() {
        st.pending.retain(|(seq, _)| !done.contains(seq));
    }
    let held = st.pending.iter().any(|(seq, _)| *seq == ack);
    if rtype == RUDP_TYPE_ACK && ack as i64 == st.last_ack && held {
        st.duplicate_acks += 1;
        if st.duplicate_acks >= 3 {
            st.duplicate_acks = 0;
            if let Some((_, entry)) = st.pending.iter_mut().find(|(seq, _)| *seq == ack) {
                if now - entry.sent >= DUP_ACK_MIN_INTERVAL {
                    entry.sent = now;
                    entry.retries += 1;
                    let data = entry.data.clone();
                    build_send(st, outs, RUDP_TYPE_DATA, data, ack, Vec::new());
                }
            }
        }
    } else if rtype == RUDP_TYPE_ACK {
        st.last_ack = ack as i64;
        st.duplicate_acks = 0;
    }
}

fn close_now(inner: &Arc<Inner>, reason: &str) {
    let mut st = inner.st.lock().unwrap();
    if st.ended {
        return;
    }
    let frame = rudp_encode(&RudpFrame {
        rtype: RUDP_TYPE_DISCONNECT,
        seq: 0,
        ack: st.next_read,
        payload: Vec::new(),
        fec_count: 0,
        fec_lengths: Vec::new(),
    });
    let packet = sign_punch_frame(&frame, st.auth_key.as_ref());
    let mut outs = Vec::new();
    if let Some(pi) = st.primary {
        let path = &st.paths[pi];
        let wire = match &path.codec {
            Some(c) => c.encode(&packet),
            None => packet.clone(),
        };
        if let Some(addr) = target_addr(&path.remote) {
            outs.push(Outbound {
                socket: path.socket.clone(),
                addr,
                bytes: wire,
            });
        }
    }
    st.ended = true;
    st.running = false;
    st.pending.clear();
    st.reordered.clear();
    st.fec_receive.clear();
    st.fec_send.clear();
    st.incoming.clear();
    st.queued_bytes = 0;
    let reason = reason.to_string();
    drop(st);
    flush(outs);
    inner.wake.bump();
    emit(inner, RudpEvent::Closed(reason.clone()));
    let mut slot = inner.on_closed.lock().unwrap();
    if let Some(cb) = slot.as_mut() {
        cb(reason);
    }
}

fn receive(inner: &Arc<Inner>, pi: usize, packet: &[u8], from: SocketAddr) {
    let mut st = inner.st.lock().unwrap();
    if st.ended {
        return;
    }
    let (codec, remote_address, has_codec) = match st.paths.get(pi) {
        Some(p) => (p.codec.clone(), p.remote.address.clone(), p.codec.is_some()),
        None => return,
    };
    if from.ip().to_string() != remote_address
        || (has_codec && from.port() != st.paths[pi].remote.port)
    {
        return;
    }
    let decoded = match &codec {
        Some(c) => match c.decode(packet) {
            Some(v) => v,
            None => return,
        },
        None => packet.to_vec(),
    };
    let key = st.auth_key;
    let raw = match verify_punch_frame(&decoded, key.as_ref()) {
        Some(v) => v,
        None => {
            if has_codec && st.allow_turn_auth_downgrade {
                st.auth_drops += 1;
            }
            let hit = st.auth_drops >= 3 && st.allow_turn_auth_downgrade && has_codec;
            if hit {
                st.auth_key = None;
                st.last_rx = now_ms();
            }
            drop(st);
            if hit {
                emit(inner, RudpEvent::AuthDowngrade);
            }
            return;
        }
    };
    st.auth_drops = 0;
    if raw.len() < 3 || raw[0] != 86 || raw[1] != 76 {
        return;
    }
    let now = now_ms();
    if st.paths[pi].codec.is_none() {
        let path = &mut st.paths[pi];
        if from.port() == path.remote.port {
            path.last_current = now;
            path.pending_port = -1;
        } else if !(path.last_current > 0.0 && now - path.last_current < DRIFT_SILENT_MS) {
            if path.pending_port == from.port() as i32 && now - path.pending_at < DRIFT_CONFIRM_MS {
                path.remote.port = from.port();
                path.last_current = now;
                path.pending_port = -1;
            } else {
                path.pending_port = from.port() as i32;
                path.pending_at = now;
            }
        }
    }
    if st.paths[pi].remote.port != from.port() {
        return;
    }
    let mut outs: Vec<Outbound> = Vec::new();
    if raw[2] == RUDP_TYPE_PUNCH || raw[2] == RUDP_TYPE_PUNCH_ACK {
        st.last_rx = now;
        st.paths[pi].last_rx = now;
        st.paths[pi].rx += 1;
        if raw[2] == RUDP_TYPE_PUNCH {
            let signed = sign_punch_frame(&[86, 76, 2, 0, 0], st.auth_key.as_ref());
            send_one(&mut st, &mut outs, pi, signed);
        }
        drop(st);
        flush(outs);
        inner.wake.bump();
        return;
    }
    let Some(f) = rudp_decode(&raw) else { return };
    st.last_rx = now;
    st.paths[pi].last_rx = now;
    st.paths[pi].rx += 1;
    if f.rtype == RUDP_TYPE_DATA || f.rtype == RUDP_TYPE_ACK || f.rtype == RUDP_TYPE_KEEPALIVE {
        process_ack(&mut st, f.ack, f.rtype, now, &mut outs);
    }
    if f.rtype == RUDP_TYPE_DISCONNECT {
        drop(st);
        flush(outs);
        close_now(inner, "对端断开");
        return;
    }
    if f.rtype == RUDP_TYPE_RESTART {
        drop(st);
        flush(outs);
        emit(inner, RudpEvent::Restart);
        inner.wake.bump();
        return;
    }
    if f.rtype == RUDP_TYPE_VOICE {
        let payload = f.payload.clone();
        drop(st);
        flush(outs);
        emit(inner, RudpEvent::Voice(payload));
        inner.wake.bump();
        return;
    }
    let mut send_ack = false;
    if f.rtype == RUDP_TYPE_DATA {
        let ahead = seq_diff(f.seq, st.next_read);
        if ahead < REORDER_WINDOW || seq_diff(st.next_read, f.seq) <= REORDER_WINDOW {
            let id = f.seq / 4;
            let gi = fec_group(&mut st, id, now);
            st.fec_receive[gi].1.data.push((f.seq, f.payload.clone()));
            store_received(&mut st, f.seq, &f.payload);
            drop(st);
            recover_fec(inner, id);
            st = inner.st.lock().unwrap();
        }
        send_ack = true;
    }
    let fec_ok = f.rtype == RUDP_TYPE_FEC_XOR
        && f.fec_count == 4
        && f.fec_lengths
            .iter()
            .all(|size| *size > 0 && *size as usize <= f.payload.len())
        && f.fec_lengths.iter().copied().max().unwrap_or(0) as usize == f.payload.len();
    if fec_ok {
        let first = f.seq.wrapping_mul(4);
        if seq_diff(first, st.next_read) < REORDER_WINDOW
            || seq_diff(st.next_read, first) <= REORDER_WINDOW
        {
            let gi = fec_group(&mut st, f.seq, now);
            st.fec_receive[gi].1.parity = Some(f.payload.clone());
            st.fec_receive[gi].1.lengths = Some(f.fec_lengths.clone());
            drop(st);
            recover_fec(inner, f.seq);
            inner.wake.bump();
            return;
        }
    }
    if send_ack {
        build_send(&mut st, &mut outs, RUDP_TYPE_ACK, Vec::new(), 0, Vec::new());
    }
    drop(st);
    flush(outs);
    inner.wake.bump();
}

/// 每块路径一个读线程，收到的包送进这里统一处理（对应 Node 的 message 事件）
fn pump(inner: Arc<Inner>, rx: Receiver<(usize, Vec<u8>, SocketAddr)>) {
    while let Ok((pi, bytes, from)) = rx.recv() {
        receive(&inner, pi, &bytes, from);
    }
}

/// 对应原版 setInterval(50ms)：重传、超时、保活
fn pump_loop(inner: Arc<Inner>) {
    loop {
        std::thread::sleep(Duration::from_millis(50));
        let now = now_ms();
        let mut outs = Vec::new();
        let mut close_reason: Option<String> = None;
        {
            let mut st = inner.st.lock().unwrap();
            if st.ended || !st.running {
                return;
            }
            if now - st.last_rx > IDLE_TIMEOUT {
                close_reason = Some("对端连接超时".into());
            } else {
                let rto = st.rto;
                let mut resend: Vec<(u32, Vec<u8>)> = Vec::new();
                for (seq, entry) in st.pending.iter_mut() {
                    if now - entry.created > PACKET_TIMEOUT {
                        close_reason = Some("可靠传输重试超时".into());
                        break;
                    }
                    let backoff = rto + (entry.retries.min(3) as f64) * 250.0;
                    if now - entry.sent >= backoff {
                        entry.sent = now;
                        entry.retries += 1;
                        resend.push((*seq, entry.data.clone()));
                    }
                }
                for (seq, data) in resend {
                    build_send(&mut st, &mut outs, RUDP_TYPE_DATA, data, seq, Vec::new());
                }
                if now - st.last_ping >= KEEPALIVE_MS {
                    st.last_ping = now;
                    build_send(
                        &mut st,
                        &mut outs,
                        RUDP_TYPE_KEEPALIVE,
                        Vec::new(),
                        0,
                        Vec::new(),
                    );
                }
            }
        }
        flush(outs);
        if let Some(reason) = close_reason {
            close_now(&inner, &reason);
            return;
        }
    }
}

impl Drop for RudpConn {
    fn drop(&mut self) {
        self.inner.st.lock().unwrap().ended = true;
        self.inner.wake.bump();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(rtype: u8, seq: u32, ack: u32, payload: &[u8], lengths: Vec<u16>) -> RudpFrame {
        RudpFrame {
            rtype,
            seq,
            ack,
            payload: payload.to_vec(),
            fec_count: lengths.len() as u8,
            fec_lengths: lengths,
        }
    }

    #[test]
    fn header_layout_is_byte_exact() {
        let p = rudp_encode(&frame(RUDP_TYPE_ACK, 7, 3, &[], vec![]));
        assert_eq!(p, vec![86, 76, 4, 0, 0, 0, 7, 0, 0, 0, 3]);
        let d = rudp_encode(&frame(RUDP_TYPE_DATA, 1, 0, b"hi", vec![]));
        assert_eq!(
            &d[..13],
            [86, 76, 3, 0, 0, 0, 1, 0, 0, 0, 0, 0, 2].as_slice()
        );
        assert_eq!(&d[13..], b"hi");
    }

    #[test]
    fn fec_header_carries_count_and_lengths() {
        let p = rudp_encode(&frame(
            RUDP_TYPE_FEC_XOR,
            2,
            9,
            &[1, 2, 3],
            vec![9, 8, 7, 6],
        ));
        assert_eq!(p[11..13], [0, 3]);
        assert_eq!(p[13], 4);
        assert_eq!(&p[14..22], [0, 9, 0, 8, 0, 7, 0, 6].as_slice());
        assert_eq!(&p[22..], [1, 2, 3]);
        // 编码器原样写调用方给的 ack；"FEC 的 ack 恒为 0" 是 send() 侧的规则（见 build_send）
        assert_eq!(u32::from_be_bytes([p[7], p[8], p[9], p[10]]), 9);
    }

    #[test]
    fn decode_rejects_malformed() {
        assert!(rudp_decode(&[86, 76, 3]).is_none());
        assert!(rudp_decode(&[0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 0]).is_none());
        let mut bad = rudp_encode(&frame(RUDP_TYPE_DATA, 1, 0, b"abc", vec![]));
        bad.push(0);
        assert!(rudp_decode(&bad).is_none());
        let big = rudp_encode(&frame(RUDP_TYPE_DATA, 1, 0, &vec![7u8; 1401], vec![]));
        assert!(rudp_decode(&big).is_none());
        let zero_count = {
            let mut p = vec![86u8, 76, 9, 0, 0, 0, 1, 0, 0, 0, 0, 0, 4, 0];
            p.extend_from_slice(&[0, 0, 0, 0]);
            p
        };
        assert!(rudp_decode(&zero_count).is_none());
    }

    #[test]
    fn roundtrip_matches_every_type() {
        for t in [
            RUDP_TYPE_PUNCH,
            RUDP_TYPE_DATA,
            RUDP_TYPE_ACK,
            RUDP_TYPE_KEEPALIVE,
            RUDP_TYPE_RESTART,
            RUDP_TYPE_VOICE,
        ] {
            let f = frame(t, 42, 41, b"xyz", vec![]);
            let back = rudp_decode(&rudp_encode(&f)).unwrap();
            assert_eq!(back.rtype, t);
            assert_eq!(back.seq, 42);
            assert_eq!(back.ack, if t == RUDP_TYPE_FEC_XOR { 0 } else { 41 });
            assert_eq!(back.payload, b"xyz");
        }
    }

    #[test]
    fn seq_math_wraps() {
        assert_eq!(seq_diff(0, 1), 0xFFFF_FFFF);
        assert!(!seq_after(0, 1));
        assert!(seq_after(1, 0));
        assert!(!seq_after(5, 5));
        assert!(seq_after(0x7FFF_FFFF, 0));
        assert!(!seq_after(0x8000_0000, 0));
    }

    fn bare_state() -> State {
        State {
            running: true,
            ended: false,
            last_rx: 0.0,
            last_ping: 0.0,
            next_send: 2,
            next_read: 0,
            pending: vec![(
                1,
                PendingPacket {
                    data: vec![1],
                    sent: 0.0,
                    created: 0.0,
                    retries: 0,
                },
            )],
            fec_send: vec![],
            fec_receive: vec![],
            srtt: -1.0,
            rttvar: 0.0,
            rto: 200.0,
            last_ack: -1,
            duplicate_acks: 0,
            reordered: HashMap::new(),
            incoming: vec![],
            queued_bytes: 0,
            auth_key: None,
            allow_turn_auth_downgrade: false,
            auth_drops: 0,
            paths: vec![],
            primary: None,
            secondary: None,
            grace_until: 0.0,
        }
    }

    #[test]
    fn rto_clamps_to_100_800() {
        // 下界：单样本 RTT=5000 时 srtt+4·rttvar 远大于 800，必须先 min(800) 再 max(100)
        let mut st = bare_state();
        process_ack(&mut st, 2, RUDP_TYPE_ACK, 5000.0, &mut Vec::new());
        assert_eq!(st.rto, 800.0);
        // 上界：样本极小时钳到 100
        let mut st2 = bare_state();
        st2.srtt = 10.0;
        st2.rttvar = 0.0;
        st2.pending[0].1.sent = 4999.0;
        process_ack(&mut st2, 2, RUDP_TYPE_ACK, 5000.0, &mut Vec::new());
        assert_eq!(st2.rto, 100.0);
    }

    #[test]
    fn reorder_window_bounds_match_original() {
        let mut st = State {
            running: true,
            ended: false,
            last_rx: 0.0,
            last_ping: 0.0,
            next_send: 0,
            next_read: 0,
            pending: vec![],
            fec_send: vec![],
            fec_receive: vec![],
            srtt: -1.0,
            rttvar: 0.0,
            rto: 200.0,
            last_ack: -1,
            duplicate_acks: 0,
            reordered: HashMap::new(),
            incoming: vec![],
            queued_bytes: 0,
            auth_key: None,
            allow_turn_auth_downgrade: false,
            auth_drops: 0,
            paths: vec![],
            primary: None,
            secondary: None,
            grace_until: 0.0,
        };
        assert!(store_received(&mut st, 511, b"a"));
        assert!(!store_received(&mut st, 512, b"b"));
        assert!(st.reordered.contains_key(&511));
    }

    fn pair(key: Option<[u8; 32]>) -> (Arc<UdpSocket>, Arc<UdpSocket>, RudpConn, RudpConn) {
        let a = Arc::new(UdpSocket::bind("127.0.0.1:0").unwrap());
        let b = Arc::new(UdpSocket::bind("127.0.0.1:0").unwrap());
        let ap = a.local_addr().unwrap();
        let bp = b.local_addr().unwrap();
        let conn_a = RudpConn::new(
            a.clone(),
            Some(RudpTarget {
                address: ap.ip().to_string(),
                port: bp.port(),
            }),
            None,
            key,
            false,
            false,
        );
        let conn_b = RudpConn::new(
            b.clone(),
            Some(RudpTarget {
                address: bp.ip().to_string(),
                port: ap.port(),
            }),
            None,
            key,
            false,
            false,
        );
        (a, b, conn_a, conn_b)
    }

    fn read_until(conn: &RudpConn, want: usize, secs: u64) -> Vec<u8> {
        let mut got = Vec::new();
        let deadline = std::time::Instant::now() + Duration::from_secs(secs);
        while got.len() < want && std::time::Instant::now() < deadline {
            let mut chunk = [0u8; 1024];
            match conn.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => got.extend_from_slice(&chunk[..n]),
                Err(_) => break,
            }
        }
        got
    }

    #[test]
    fn loopback_delivers_fragmented_stream_in_order() {
        let (_a, _b, conn_a, conn_b) = pair(None);
        conn_b.start();
        conn_a.start();
        let payload: Vec<u8> = (0..5000u32).map(|i| (i % 251) as u8).collect();
        std::thread::scope(|s| {
            s.spawn(|| conn_a.write(&payload));
            let got = read_until(&conn_b, 5000, 6);
            assert_eq!(got.len(), 5000, "分片重组不完整");
            assert_eq!(got, payload);
        });
    }

    #[test]
    fn signed_frames_interop_between_two_conns() {
        let key = crate::vl::punchauth::derive_punch_key("ABCDEF", "peer");
        let (_a, _b, conn_a, conn_b) = pair(key);
        conn_b.start();
        conn_a.start();
        std::thread::scope(|s| {
            s.spawn(|| conn_a.write(b"signed-payload"));
            assert_eq!(read_until(&conn_b, 14, 6), b"signed-payload");
        });
    }

    #[test]
    fn wrong_key_drops_frames() {
        let (_a, _b, conn_a, conn_b) =
            pair(crate::vl::punchauth::derive_punch_key("ABCDEF", "peer"));
        conn_b.start();
        conn_a.start();
        // 换掉 b 的密钥：同一套帧必须整批被拒
        conn_b.inner.st.lock().unwrap().auth_key =
            crate::vl::punchauth::derive_punch_key("ABCDEF", "other");
        std::thread::scope(|s| {
            s.spawn(|| conn_a.write(b"secret"));
            assert!(
                read_until(&conn_b, 6, 2).is_empty(),
                "密钥不匹配却收到了数据"
            );
        });
    }

    #[test]
    fn send_window_is_bounded() {
        // 对端不起线程：没人回 ACK，窗口才会真的塞满
        let (_a, _b, conn_a, _conn_b) = pair(None);
        conn_a.start();
        let big = vec![9u8; CHUNK_PLAIN * 80];
        std::thread::scope(|s| {
            let h = s.spawn(|| conn_a.write(&big));
            std::thread::sleep(Duration::from_millis(500));
            let pending = conn_a.inner.st.lock().unwrap().pending.len();
            assert!(pending <= WINDOW_PACKETS, "窗口超限: {}", pending);
            conn_a.close_with("收尾");
            assert!(
                h.join().unwrap().is_err(),
                "窗口满且连接关闭后 write 应报错"
            );
        });
    }
}
