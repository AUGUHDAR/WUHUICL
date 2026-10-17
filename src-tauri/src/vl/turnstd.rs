//! 标准 TURN（RFC 5766）客户端 —— 对应原版 voxlink/stdTurn.ts 134 行，LGPL-3.0-only
//! 上游 StdTurnClient.java, AUGUHDAR/VoxLink c475faa9。
//!
//! 本文件覆盖报文编解码、XOR 地址、长时密钥、ChannelData 编解码与 allocate/bind/keepalive/close；
//! `fetchTurnCredential`（stdTurn.ts:68，走 api.ts 的 HTTP 信封）在接线阶段随引擎一起补。

use crate::vl::rudp::{RudpCodec, RudpTarget};
use crate::vl::turn::valid_turn_endpoint;
use std::net::{IpAddr, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const TURN_COOKIE: u32 = 0x2112_a442;
pub const TURN_CHANNEL: u16 = 0x4000;
pub const TURN_LIFETIME: u32 = 600;
pub const TURN_TX_ROUNDS: u32 = 4;
pub const TURN_TX_RTO: u64 = 500;
pub const TURN_KEEPALIVE_MS: u64 = 240_000;

pub const ATTR_USERNAME: u16 = 6;
pub const ATTR_INTEGRITY: u16 = 8;
pub const ATTR_ERROR: u16 = 9;
pub const ATTR_CHANNEL: u16 = 12;
pub const ATTR_LIFETIME: u16 = 13;
pub const ATTR_PEER: u16 = 18;
pub const ATTR_REALM: u16 = 20;
pub const ATTR_NONCE: u16 = 21;
pub const ATTR_RELAY: u16 = 22;
pub const ATTR_TRANSPORT: u16 = 25;
pub const ATTR_SOFTWARE: u16 = 0x8022;
pub const ATTR_FINGERPRINT: u16 = 0x8028;

pub const MSG_ALLOCATE: u16 = 0x0003;
pub const MSG_REFRESH: u16 = 0x0004;
pub const MSG_CREATE_PERMISSION: u16 = 0x0008;
pub const MSG_CHANNEL_BIND: u16 = 0x0009;
pub const MSG_ALLOCATE_OK: u16 = 0x0103;
pub const MSG_ALLOCATE_ERR: u16 = 0x0113;
pub const MSG_REFRESH_OK: u16 = 0x0104;
pub const MSG_PERMISSION_OK: u16 = 0x0108;
pub const MSG_CHANNEL_BIND_OK: u16 = 0x0109;
pub const MSG_INDICATION: u16 = 0x0016;

fn u32_bytes(value: u32) -> Vec<u8> {
    value.to_be_bytes().to_vec()
}

/// 与 zlib/Java 的 CRC32 一致（反射多项式 0xEDB88320）
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in bytes {
        crc ^= *byte as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ if crc & 1 != 0 { 0xedb8_8320 } else { 0 };
        }
    }
    crc ^ 0xffff_ffff
}

fn header_bytes(msg_type: u16, length: u16, id: &[u8]) -> Vec<u8> {
    let mut b = vec![0u8; 20];
    b[0..2].copy_from_slice(&msg_type.to_be_bytes());
    b[2..4].copy_from_slice(&length.to_be_bytes());
    b[4..8].copy_from_slice(&TURN_COOKIE.to_be_bytes());
    b[8..20].copy_from_slice(&id[..12]);
    b
}

pub struct TurnMessage {
    pub msg_type: u16,
    pub id: [u8; 12],
    attrs: Vec<u8>,
}

impl TurnMessage {
    pub fn new(msg_type: u16) -> Self {
        TurnMessage {
            msg_type,
            id: random_id(),
            attrs: Vec::new(),
        }
    }

    pub fn with_id(msg_type: u16, id: [u8; 12]) -> Self {
        TurnMessage {
            msg_type,
            id,
            attrs: Vec::new(),
        }
    }

    /// 属性按 4 字节对齐补零（RFC 5389）
    pub fn put(mut self, attr: u16, value: &[u8]) -> Self {
        let mut b = vec![0u8; 4 + ((value.len() + 3) & !3)];
        b[0..2].copy_from_slice(&attr.to_be_bytes());
        b[2..4].copy_from_slice(&(value.len() as u16).to_be_bytes());
        b[4..4 + value.len()].copy_from_slice(value);
        self.attrs.extend_from_slice(&b);
        self
    }

    pub fn number(self, attr: u16, value: u32) -> Self {
        self.put(attr, &u32_bytes(value))
    }

    /// XOR-PEER-ADDRESS / XOR-RELAY-ADDRESS：地址字面量非法时给出原版同款错误
    pub fn address(self, attr: u16, target: &RudpTarget) -> Result<Self, String> {
        let encoded = encode_xor_address(target, &self.id)?;
        Ok(self.put(attr, &encoded))
    }

    /// 带密钥时追加 MESSAGE-INTEGRITY（HMAC-SHA1）与 FINGERPRINT
    pub fn build(&self, key: Option<&[u8]>) -> Vec<u8> {
        let head = header_bytes(self.msg_type, self.attrs.len() as u16, &self.id);
        let Some(key) = key else {
            let mut out = head;
            out.extend_from_slice(&self.attrs);
            return out;
        };
        let mut prefix_for_mi =
            header_bytes(self.msg_type, (self.attrs.len() + 24) as u16, &self.id);
        prefix_for_mi.extend_from_slice(&self.attrs);
        let mi = hmac_sha1(key, &prefix_for_mi);
        let mut integrity = vec![0u8; 24];
        integrity[0..2].copy_from_slice(&ATTR_INTEGRITY.to_be_bytes());
        integrity[2..4].copy_from_slice(&20u16.to_be_bytes());
        integrity[4..24].copy_from_slice(&mi[..20]);
        let mut prefix = header_bytes(self.msg_type, (self.attrs.len() + 32) as u16, &self.id);
        prefix.extend_from_slice(&self.attrs);
        prefix.extend_from_slice(&integrity);
        let mut fp = vec![0u8; 8];
        fp[0..2].copy_from_slice(&ATTR_FINGERPRINT.to_be_bytes());
        fp[2..4].copy_from_slice(&4u16.to_be_bytes());
        fp[4..8].copy_from_slice(&(crc32(&prefix) ^ 0x5354_554e).to_be_bytes());
        prefix.extend_from_slice(&fp);
        prefix
    }
}

fn random_id() -> [u8; 12] {
    let mut id = [0u8; 12];
    rand::RngCore::fill_bytes(&mut rand::rng(), &mut id);
    id
}

fn hmac_sha1(key: &[u8], data: &[u8]) -> Vec<u8> {
    use hmac::{Hmac, Mac};
    use sha1::Sha1;
    let mut mac = Hmac::<Sha1>::new_from_slice(key).expect("HMAC 接受任意长度密钥");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

#[derive(Clone, Debug)]
pub struct ParsedTurnMessage {
    pub msg_type: u16,
    pub id: [u8; 12],
    pub attrs: Vec<(u16, Vec<u8>)>,
    pub error_code: u16,
}

impl ParsedTurnMessage {
    pub fn attr(&self, attr: u16) -> Option<&Vec<u8>> {
        self.attrs.iter().find(|(k, _)| *k == attr).map(|(_, v)| v)
    }

    pub fn has(&self, attr: u16) -> bool {
        self.attrs.iter().any(|(k, _)| *k == attr)
    }
}

/// 对应 parseTurnMessage：类号必须是 0x00/0x01 段、长度 4 对齐、属性数 ≤64
pub fn parse_turn_message(b: &[u8]) -> Option<ParsedTurnMessage> {
    if b.len() < 20
        || (b[0] & 0xc0) != 0
        || u32::from_be_bytes([b[4], b[5], b[6], b[7]]) != TURN_COOKIE
    {
        return None;
    }
    let end = 20 + u16::from_be_bytes([b[2], b[3]]) as usize;
    if end > b.len() || (end - 20) % 4 != 0 {
        return None;
    }
    let mut attrs: Vec<(u16, Vec<u8>)> = Vec::new();
    let mut off = 20usize;
    let mut count = 0u32;
    while off < end {
        if off + 4 > end {
            return None;
        }
        count += 1;
        if count > 64 {
            return None;
        }
        let attr = u16::from_be_bytes([b[off], b[off + 1]]);
        let length = u16::from_be_bytes([b[off + 2], b[off + 3]]) as usize;
        off += 4;
        if off + length > end {
            return None;
        }
        attrs.push((attr, b[off..off + length].to_vec()));
        off += (length + 3) & !3;
    }
    let mut id = [0u8; 12];
    id.copy_from_slice(&b[8..20]);
    let error_code = match attrs.iter().find(|(k, _)| *k == ATTR_ERROR) {
        Some((_, v)) if v.len() >= 4 => ((v[2] & 7) as u16) * 100 + v[3] as u16,
        _ => 0,
    };
    Some(ParsedTurnMessage {
        msg_type: u16::from_be_bytes([b[0], b[1]]),
        id,
        attrs,
        error_code,
    })
}

fn ip_bytes(address: &str) -> Option<Vec<u8>> {
    match address.parse::<IpAddr>() {
        Ok(IpAddr::V4(v4)) => Some(v4.octets().to_vec()),
        Ok(IpAddr::V6(v6)) => Some(v6.octets().to_vec()),
        _ => None,
    }
}

pub fn encode_xor_address(target: &RudpTarget, id: &[u8]) -> Result<Vec<u8>, String> {
    let Some(ip) = ip_bytes(&target.address) else {
        return Err("无效的 TURN 地址".into());
    };
    let mut b = vec![0u8; 4 + ip.len()];
    let mask = [u32_bytes(TURN_COOKIE), id[..12].to_vec()].concat();
    b[1] = if ip.len() == 4 { 1 } else { 2 };
    b[2..4].copy_from_slice(&(target.port ^ (TURN_COOKIE >> 16) as u16).to_be_bytes());
    for (i, byte) in ip.iter().enumerate() {
        b[4 + i] = *byte ^ mask[i];
    }
    Ok(b)
}

pub fn decode_xor_address(b: Option<&Vec<u8>>, id: &[u8]) -> Option<RudpTarget> {
    let b = b?;
    if b.is_empty() || b[0] != 0 {
        return None;
    }
    let ipv6 = b[1] == 2;
    if !((b[1] == 1 && b.len() >= 8) || (ipv6 && b.len() >= 20)) {
        return None;
    }
    let mask = [u32_bytes(TURN_COOKIE), id[..12].to_vec()].concat();
    let bytes: Vec<u8> = (0..if ipv6 { 16 } else { 4 })
        .map(|i| b[4 + i] ^ mask[i])
        .collect();
    let address = if ipv6 {
        let mut octets = [0u8; 16];
        octets.copy_from_slice(&bytes);
        IpAddr::from(octets).to_string()
    } else {
        IpAddr::from(<[u8; 4]>::try_from(bytes.as_slice()).ok()?).to_string()
    };
    Some(RudpTarget {
        address,
        port: u16::from_be_bytes([b[2], b[3]]) ^ (TURN_COOKIE >> 16) as u16,
    })
}

/// 长时凭证密钥：md5(username:realm:password)
pub fn turn_long_term_key(username: &str, realm: &str, password: &str) -> Vec<u8> {
    use md5::{Digest, Md5};
    let mut hash = Md5::new();
    hash.update(format!("{}:{}:{}", username, realm, password).as_bytes());
    hash.finalize().to_vec()
}

#[derive(Clone, Debug)]
pub struct TurnCredential {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub expire: f64,
}

/// ChannelData：u16 通道号 + u16 长度
#[derive(Clone, Debug)]
pub struct StdTurnCodec {
    pub channel: u16,
}

impl Default for StdTurnCodec {
    fn default() -> Self {
        StdTurnCodec {
            channel: TURN_CHANNEL,
        }
    }
}

impl RudpCodec for StdTurnCodec {
    fn encode(&self, frame: &[u8]) -> Vec<u8> {
        let mut out = vec![0u8; 4 + frame.len()];
        out[0..2].copy_from_slice(&self.channel.to_be_bytes());
        out[2..4].copy_from_slice(&(frame.len() as u16).to_be_bytes());
        out[4..].copy_from_slice(frame);
        out
    }

    fn decode(&self, packet: &[u8]) -> Option<Vec<u8>> {
        if packet.len() < 4
            || u16::from_be_bytes([packet[0], packet[1]]) != self.channel
            || self.channel < 0x4000
            || self.channel > 0x7fff
        {
            return None;
        }
        let length = u16::from_be_bytes([packet[2], packet[3]]) as usize;
        if length + 4 <= packet.len() {
            Some(packet[4..4 + length].to_vec())
        } else {
            None
        }
    }
}

/// 对应 quiet()：会话已收尾就不再发；发出去时带当前密钥签名
fn quiet(state: &StdTurnState, request: &TurnMessage) {
    if state.ended.load(Ordering::SeqCst) {
        return;
    }
    let key = state.key.lock().unwrap().clone();
    let packet = request.build(key.as_deref());
    if let Ok(list) = (state.target.address.as_str(), state.target.port).to_socket_addrs() {
        if let Some(target) = list.into_iter().next() {
            let _ = state.socket.send_to(&packet, target);
        }
    }
}

/// 会话内需要跨线程共享的部分（keepalive 线程要能读到 close() 后的状态）
#[derive(Debug)]
pub struct StdTurnState {
    socket: Arc<UdpSocket>,
    target: RudpTarget,
    username: String,
    key: Mutex<Option<Vec<u8>>>,
    realm: Mutex<String>,
    nonce: Mutex<String>,
    peer: Mutex<Option<RudpTarget>>,
    ended: AtomicBool,
}

/// 对应 StdTurnSession：握手期持有 socket，分配成功后原样交给 RUDP
#[derive(Debug)]
pub struct StdTurnSession {
    state: Arc<StdTurnState>,
    pub relay: RudpTarget,
    pub lifetime: u32,
    pub codec: StdTurnCodec,
}

impl std::ops::Deref for StdTurnSession {
    type Target = StdTurnState;
    fn deref(&self) -> &StdTurnState {
        &self.state
    }
}

/// 对应 stdTurn.ts:68-74 的 `socket`：分配完成后由 RUDP 接管同一个数据报套接字
impl StdTurnSession {
    pub fn socket(&self) -> Arc<UdpSocket> {
        self.state.socket.clone()
    }

    pub fn peer(&self) -> Option<RudpTarget> {
        self.state.peer.lock().unwrap().clone()
    }

    pub fn is_ended(&self) -> bool {
        self.state.ended.load(Ordering::SeqCst)
    }
}

impl StdTurnSession {
    fn auth(&self, msg_type: u16) -> TurnMessage {
        TurnMessage::new(msg_type)
            .put(ATTR_USERNAME, self.username.as_bytes())
            .put(ATTR_REALM, self.realm.lock().unwrap().as_bytes())
            .put(ATTR_NONCE, self.nonce.lock().unwrap().as_bytes())
    }

    /// 对应 transact()：Java 侧收超时 200ms，故每轮 min(rto,200,剩余)，rto 每轮 ×3。
    /// 非对端报文与 id 不匹配的报文只是忽略，不消耗重传轮次（原版是事件监听，靠定时器换轮）。
    fn transact(
        &self,
        request: &TurnMessage,
        budget_ms: u64,
        cancelled: &AtomicBool,
    ) -> Result<ParsedTurnMessage, String> {
        let key = self.key.lock().unwrap().clone();
        let packet = request.build(key.as_deref());
        let target = match (self.target.address.as_str(), self.target.port).to_socket_addrs() {
            Ok(list) => list.collect::<Vec<_>>().into_iter().next(),
            Err(_) => None,
        };
        let Some(target) = target else {
            return Err("标准 TURN 地址无效".into());
        };
        let deadline = Instant::now() + Duration::from_millis(budget_ms);
        let mut round = 0u32;
        let mut rto = TURN_TX_RTO;
        let mut buf = vec![0u8; 2048];
        loop {
            if self.ended.load(Ordering::SeqCst) || cancelled.load(Ordering::SeqCst) {
                return Err("标准 TURN 操作已取消".into());
            }
            if round >= TURN_TX_ROUNDS || Instant::now() >= deadline {
                return Err("标准 TURN 节点响应超时".into());
            }
            round += 1;
            if self.socket.send_to(&packet, target).is_err() {
                return Err("标准 TURN 发送失败".into());
            }
            let window = rto.min(200).max(1);
            rto = rto.saturating_mul(3);
            let round_end = Instant::now() + Duration::from_millis(window);
            loop {
                let now = Instant::now();
                let stop = round_end.min(deadline);
                if now >= stop {
                    break;
                }
                let _ = self.socket.set_read_timeout(Some(stop - now));
                match self.socket.recv_from(&mut buf) {
                    Ok((size, from)) => {
                        if from != target {
                            continue;
                        }
                        if let Some(parsed) = parse_turn_message(&buf[..size]) {
                            if parsed.id == request.id {
                                return Ok(parsed);
                            }
                        }
                    }
                    Err(ref e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            || e.kind() == std::io::ErrorKind::TimedOut => {}
                    Err(_) => return Err("标准 TURN 连接已关闭".into()),
                }
            }
        }
    }

    /// 对应 allocate()：先无凭证拿 401 挑战，再带 HMAC 重新 Allocate
    pub fn allocate(
        cred: &TurnCredential,
        cancelled: &Arc<AtomicBool>,
        timeout_ms: u64,
    ) -> Result<StdTurnSession, String> {
        let target = match (cred.host.as_str(), cred.port).to_socket_addrs() {
            Ok(list) => list.collect::<Vec<_>>().into_iter().next(),
            Err(_) => None,
        };
        let Some(target) = target else {
            return Err("标准 TURN 地址无法解析".into());
        };
        let socket = Arc::new(UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?);
        let mut session = StdTurnSession {
            state: Arc::new(StdTurnState {
                socket,
                target: RudpTarget {
                    address: target.ip().to_string(),
                    port: target.port(),
                },
                username: cred.username.clone(),
                key: Mutex::new(None),
                realm: Mutex::new(String::new()),
                nonce: Mutex::new(String::new()),
                peer: Mutex::new(None),
                ended: AtomicBool::new(false),
            }),
            relay: RudpTarget {
                address: String::new(),
                port: 0,
            },
            lifetime: TURN_LIFETIME,
            codec: StdTurnCodec::default(),
        };
        let challenge = {
            let mut got: Option<ParsedTurnMessage> = None;
            for _ in 0..3 {
                let request = TurnMessage::new(MSG_ALLOCATE)
                    .number(ATTR_TRANSPORT, 17 << 24)
                    .put(ATTR_SOFTWARE, b"voxlink");
                match session.transact(&request, timeout_ms.min(3000), cancelled) {
                    Ok(parsed) => {
                        got = Some(parsed);
                        break;
                    }
                    Err(_) => {
                        if cancelled.load(Ordering::SeqCst) {
                            return Err("标准 TURN 操作已取消".into());
                        }
                    }
                }
            }
            got
        };
        let Some(challenge) = challenge else {
            return Err("标准 TURN 未返回有效鉴权挑战".into());
        };
        if challenge.msg_type != MSG_ALLOCATE_ERR
            || challenge.error_code != 401
            || !challenge.has(ATTR_REALM)
            || !challenge.has(ATTR_NONCE)
        {
            return Err("标准 TURN 未返回有效鉴权挑战".into());
        }
        *session.realm.lock().unwrap() =
            String::from_utf8_lossy(challenge.attr(ATTR_REALM).unwrap()).to_string();
        *session.nonce.lock().unwrap() =
            String::from_utf8_lossy(challenge.attr(ATTR_NONCE).unwrap()).to_string();
        // 长时密钥固定为 MD5(username:realm:password)（stdTurn.ts:62）
        *session.key.lock().unwrap() = Some(turn_long_term_key(
            &cred.username,
            &session.realm.lock().unwrap(),
            &cred.password,
        ));
        let request = TurnMessage::with_id(MSG_ALLOCATE, random_id())
            .number(ATTR_TRANSPORT, 17 << 24)
            .put(ATTR_USERNAME, cred.username.as_bytes())
            .put(ATTR_REALM, session.realm.lock().unwrap().as_bytes())
            .put(ATTR_NONCE, session.nonce.lock().unwrap().as_bytes())
            .put(ATTR_SOFTWARE, b"voxlink");
        let reply = session.transact(&request, timeout_ms, cancelled);
        let reply = match reply {
            Ok(reply) => reply,
            Err(error) => {
                session.close();
                return Err(error);
            }
        };
        let relay = decode_xor_address(reply.attr(ATTR_RELAY), &reply.id);
        if reply.msg_type != MSG_ALLOCATE_OK || relay.as_ref().map(|r| r.port == 0).unwrap_or(true)
        {
            session.close();
            return Err(match reply.error_code {
                0 => "标准 TURN 分配失败".into(),
                code => format!("标准 TURN 分配失败 ({})", code),
            });
        }
        if let Some(life) = reply.attr(ATTR_LIFETIME) {
            if life.len() == 4 {
                session.lifetime = u32::from_be_bytes([life[0], life[1], life[2], life[3]]);
            }
        }
        session.relay = relay.unwrap();
        if cancelled.load(Ordering::SeqCst) {
            session.close();
            return Err("标准 TURN 操作已取消".into());
        }
        Ok(session)
    }

    /// 对应 bind()：先 CreatePermission，再 ChannelBind 到 0x4000
    pub fn bind(
        &self,
        peer: &RudpTarget,
        cancelled: &AtomicBool,
        timeout_ms: u64,
    ) -> Result<(), String> {
        if peer.address.parse::<IpAddr>().is_err()
            || !valid_turn_endpoint(Some(&peer.address), Some(peer.port as f64))
        {
            return Err("对端 TURN 地址无效".into());
        }
        let permission = self.auth(MSG_CREATE_PERMISSION).address(
            ATTR_PEER,
            &RudpTarget {
                address: peer.address.clone(),
                port: 0,
            },
        )?;
        let reply = self.transact(&permission, timeout_ms, cancelled)?;
        if reply.msg_type != MSG_PERMISSION_OK {
            return Err("标准 TURN 权限建立失败".into());
        }
        let bind = self
            .auth(MSG_CHANNEL_BIND)
            .put(ATTR_CHANNEL, &[0x40, 0, 0, 0])
            .address(ATTR_PEER, peer)?;
        let reply = self.transact(&bind, timeout_ms, cancelled)?;
        if reply.msg_type != MSG_CHANNEL_BIND_OK {
            return Err("标准 TURN ChannelBind 失败".into());
        }
        *self.peer.lock().unwrap() = Some(peer.clone());
        Ok(())
    }

    /// 对应 startKeepalive()：每 240 秒 Refresh(600) + 重绑 ChannelBind
    pub fn start_keepalive(&self) {
        if self.state.ended.load(Ordering::SeqCst) {
            return;
        }
        let state = self.state.clone();
        std::thread::spawn(move || {
            while !state.ended.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(TURN_KEEPALIVE_MS));
                if state.ended.load(Ordering::SeqCst) {
                    return;
                }
                let realm = state.realm.lock().unwrap().clone();
                let nonce = state.nonce.lock().unwrap().clone();
                let refresh = TurnMessage::new(MSG_REFRESH)
                    .put(ATTR_USERNAME, state.username.as_bytes())
                    .put(ATTR_REALM, realm.as_bytes())
                    .put(ATTR_NONCE, nonce.as_bytes())
                    .number(ATTR_LIFETIME, TURN_LIFETIME);
                quiet(&state, &refresh);
                let Some(peer_target) = state.peer.lock().unwrap().clone() else {
                    continue;
                };
                let request = TurnMessage::new(MSG_CHANNEL_BIND)
                    .put(ATTR_USERNAME, state.username.as_bytes())
                    .put(ATTR_REALM, realm.as_bytes())
                    .put(ATTR_NONCE, nonce.as_bytes())
                    .put(ATTR_CHANNEL, &[0x40, 0, 0, 0]);
                // peer 在 bind() 已校验过，这里只是把 Result 收住
                if let Ok(request) = request.address(ATTR_PEER, &peer_target) {
                    quiet(&state, &request);
                }
            }
        });
    }

    /// 对应 close()：先发 lifetime=0 的 Refresh 注销分配。
    /// 原版紧接着 socket.close()；Rust 侧句柄已交给 RUDP，不能在此强行关闭，
    /// 由最后一个 Arc 释放时自然回收（与私有 TURN 的 close 同一处理）。
    pub fn close(&self) {
        if self.ended.swap(true, Ordering::SeqCst) {
            return;
        }
        let key = self.key.lock().unwrap().clone();
        if let Some(secret) = key {
            let request = self.auth(MSG_REFRESH).number(ATTR_LIFETIME, 0);
            let packet = request.build(Some(&secret));
            if let Ok(list) = (self.target.address.as_str(), self.target.port).to_socket_addrs() {
                if let Some(addr) = list.into_iter().next() {
                    let _ = self.socket.send_to(&packet, addr);
                }
            }
            *self.key.lock().unwrap() = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id() -> [u8; 12] {
        [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]
    }

    #[test]
    fn crc32_matches_known_vectors() {
        assert_eq!(crc32(b""), 0);
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
        assert_eq!(
            crc32(b"The quick brown fox jumps over the lazy dog"),
            0x414f_a339
        );
    }

    #[test]
    fn xor_address_roundtrip_v4_and_v6() {
        let target = RudpTarget {
            address: "203.0.113.9".into(),
            port: 12345,
        };
        let encoded = encode_xor_address(&target, &id()).unwrap();
        assert_eq!(encoded[1], 1);
        assert_eq!(encoded.len(), 8, "XOR-ADDRESS = 4 字节头 + 4 字节 IPv4");
        // 明文里不能直接看到真实端口
        assert_ne!(u16::from_be_bytes([encoded[2], encoded[3]]), 12345);
        let back = decode_xor_address(Some(&encoded), &id()).unwrap();
        assert_eq!(back.address, "203.0.113.9");
        assert_eq!(back.port, 12345);

        let v6 = RudpTarget {
            address: "2001:db8::1".into(),
            port: 40000,
        };
        let encoded = encode_xor_address(&v6, &id()).unwrap();
        assert_eq!(encoded[1], 2);
        assert_eq!(encoded.len(), 20);
        let back = decode_xor_address(Some(&encoded), &id()).unwrap();
        assert_eq!(back.address, "2001:db8::1");
        assert_eq!(back.port, 40000);
        assert!(decode_xor_address(Some(&vec![0, 1, 0, 0, 0, 0]), &id()).is_none());
        assert!(decode_xor_address(None, &id()).is_none());
        // 域名不是 IP 字面量，原版在 ipBytes 抛「无效的 TURN 地址」
        assert_eq!(
            encode_xor_address(
                &RudpTarget {
                    address: "turn.example.com".into(),
                    port: 3478
                },
                &id()
            )
            .unwrap_err(),
            "无效的 TURN 地址"
        );
    }

    #[test]
    fn message_build_adds_integrity_then_fingerprint() {
        let key = turn_long_term_key("user", "realm", "pass");
        assert_eq!(key.len(), 16, "md5 摘要固定 16 字节");
        let request = TurnMessage::with_id(MSG_ALLOCATE, id())
            .number(ATTR_TRANSPORT, 17 << 24)
            .put(ATTR_USERNAME, b"user");
        let plain = request.build(None);
        assert_eq!(
            u16::from_be_bytes([plain[2], plain[3]]) as usize,
            plain.len() - 20
        );
        let signed = request.build(Some(&key));
        let parsed = parse_turn_message(&signed).unwrap();
        assert_eq!(parsed.msg_type, MSG_ALLOCATE);
        assert_eq!(parsed.id, id());
        assert!(parsed.has(ATTR_INTEGRITY));
        assert_eq!(parsed.attr(ATTR_INTEGRITY).unwrap().len(), 20);
        assert!(parsed.has(ATTR_FINGERPRINT));
        assert_eq!(parsed.attr(ATTR_FINGERPRINT).unwrap().len(), 4);
        // FINGERPRINT = crc32(除自身外的整包) ^ 0x5354554e
        let without_fp = &signed[..signed.len() - 8];
        let expect = crc32(without_fp) ^ 0x5354_554e;
        assert_eq!(&signed[signed.len() - 4..], &expect.to_be_bytes());
    }

    #[test]
    fn parse_rejects_non_stun_shapes() {
        assert!(parse_turn_message(&[0u8; 8]).is_none());
        let mut bad_cookie = header_bytes(MSG_ALLOCATE_OK, 0, &id());
        bad_cookie[4] ^= 0xff;
        assert!(parse_turn_message(&bad_cookie).is_none());
        // 属性长度必须是 4 的倍数
        let mut unaligned = header_bytes(MSG_ALLOCATE_OK, 6, &id());
        unaligned.extend_from_slice(&[0, 6, 0, 2, 0, 0]);
        assert!(parse_turn_message(&unaligned).is_none());
        // 声明长度超过实际包
        let lying = header_bytes(MSG_ALLOCATE_OK, 400, &id());
        assert!(parse_turn_message(&lying).is_none());
    }

    #[test]
    fn error_code_is_class_number_times_100() {
        let mut packet = header_bytes(MSG_ALLOCATE_OK, 8, &id());
        packet.extend_from_slice(&ATTR_ERROR.to_be_bytes());
        packet.extend_from_slice(&4u16.to_be_bytes());
        packet.extend_from_slice(&[0, 0, 4, 1]);
        let parsed = parse_turn_message(&packet).unwrap();
        assert_eq!(parsed.error_code, 401);
    }

    #[test]
    fn channel_data_codec_rules() {
        let codec = StdTurnCodec::default();
        let frame = codec.encode(b"rudp-frame");
        assert_eq!(&frame[0..2], &[0x40, 0x00]);
        assert_eq!(&frame[2..4], &[0, 10]);
        assert_eq!(codec.decode(&frame).unwrap(), b"rudp-frame");
        // 长度自相矛盾必须拒收
        let mut lying = frame.clone();
        lying[2..4].copy_from_slice(&[0, 200]);
        assert!(codec.decode(&lying).is_none());
        // 通道号越界必须拒收
        let narrow = StdTurnCodec { channel: 0x3fff };
        assert!(narrow.decode(&frame).is_none());
    }

    struct FakeStdTurn {
        port: u16,
        hits: Arc<Mutex<usize>>,
    }

    /// 单个属性：类型 + 长度 + 4 字节对齐补零
    fn attr_bytes(attr: u16, value: &[u8]) -> Vec<u8> {
        let mut b = vec![0u8; 4 + ((value.len() + 3) & !3)];
        b[0..2].copy_from_slice(&attr.to_be_bytes());
        b[2..4].copy_from_slice(&(value.len() as u16).to_be_bytes());
        b[4..4 + value.len()].copy_from_slice(value);
        b
    }

    /// 校验客户端发出的 MESSAGE-INTEGRITY 与 FINGERPRINT。
    /// 这是关键一环：HMAC 的输入必须用「属性长度+24」重写头部，写错长度真服务器会全部拒收。
    fn signed_ok(raw: &[u8], key: &[u8]) -> bool {
        let Some(parsed) = parse_turn_message(raw) else {
            return false;
        };
        let Some(mi) = parsed.attr(ATTR_INTEGRITY) else {
            return false;
        };
        let Some(fp) = parsed.attr(ATTR_FINGERPRINT) else {
            return false;
        };
        if mi.len() != 20 || fp.len() != 4 {
            return false;
        }
        // 尾部依次是 MESSAGE-INTEGRITY(24) + FINGERPRINT(8)
        let Some(head) = raw.strip_suffix(fp_attr_bytes(fp).as_slice()) else {
            return false;
        };
        let attrs_len = head.len() - 20 - 24;
        let mut prefix = header_bytes(parsed.msg_type, (attrs_len + 24) as u16, &parsed.id);
        prefix.extend_from_slice(&raw[20..20 + attrs_len]);
        if hmac_sha1(key, &prefix) != mi.as_slice() {
            return false;
        }
        // 指纹值在整包最后 4 字节，CRC 覆盖的是去掉尾部 FINGERPRINT 属性之后的全部字节
        u32::from_be_bytes([
            raw[raw.len() - 4],
            raw[raw.len() - 3],
            raw[raw.len() - 2],
            raw[raw.len() - 1],
        ]) == crc32(head) ^ 0x5354_554e
    }

    fn fp_attr_bytes(value: &[u8]) -> Vec<u8> {
        let mut b = vec![0u8; 8];
        b[0..2].copy_from_slice(&ATTR_FINGERPRINT.to_be_bytes());
        b[2..4].copy_from_slice(&4u16.to_be_bytes());
        b[4..8].copy_from_slice(value);
        b
    }

    /// 假标准 TURN：首次回 401 挑战，之后按类型回 Allocate/Permission/ChannelBind 成功
    fn fake_std_turn(relay_ip: &str, relay_port: u16) -> FakeStdTurn {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = socket.local_addr().unwrap().port();
        let hits = Arc::new(Mutex::new(0usize));
        let counter = hits.clone();
        let relay_ip = relay_ip.to_string();
        let key = turn_long_term_key("user", "realm", "pass");
        std::thread::spawn(move || {
            let mut buf = [0u8; 2048];
            while let Ok((size, from)) = socket.recv_from(&mut buf) {
                let raw = &buf[..size];
                let Some(request) = parse_turn_message(raw) else {
                    continue;
                };
                if request.has(ATTR_INTEGRITY) {
                    // 签名不成立就当没听见，让用例走超时而不是假装通过
                    if !signed_ok(raw, &key) {
                        continue;
                    }
                }
                *counter.lock().unwrap() += 1;
                let (reply_type, attrs) = if !request.has(ATTR_INTEGRITY) {
                    let mut error = vec![0u8; 4];
                    error[0..2].copy_from_slice(&ATTR_ERROR.to_be_bytes());
                    error[2..4].copy_from_slice(&4u16.to_be_bytes());
                    error.extend_from_slice(&[0, 0, 4, 1]);
                    let mut attrs = vec![error];
                    attrs.push(attr_bytes(ATTR_REALM, b"realm"));
                    attrs.push(attr_bytes(ATTR_NONCE, b"abcd"));
                    (MSG_ALLOCATE_ERR, attrs)
                } else {
                    let extra = match request.msg_type {
                        MSG_ALLOCATE => {
                            let target = RudpTarget {
                                address: relay_ip.clone(),
                                port: relay_port,
                            };
                            let xor = encode_xor_address(&target, &request.id).unwrap();
                            vec![
                                attr_bytes(ATTR_LIFETIME, &600u32.to_be_bytes()),
                                attr_bytes(ATTR_RELAY, &xor),
                            ]
                        }
                        MSG_CREATE_PERMISSION | MSG_CHANNEL_BIND | MSG_REFRESH => Vec::new(),
                        _ => continue,
                    };
                    let ok_type = match request.msg_type {
                        MSG_ALLOCATE => MSG_ALLOCATE_OK,
                        MSG_CREATE_PERMISSION => MSG_PERMISSION_OK,
                        MSG_CHANNEL_BIND => MSG_CHANNEL_BIND_OK,
                        _ => MSG_REFRESH_OK,
                    };
                    (ok_type, extra)
                };
                let body: Vec<u8> = attrs.concat();
                let mut reply = header_bytes(reply_type, body.len() as u16, &request.id);
                reply.extend_from_slice(&body);
                let _ = socket.send_to(&reply, from);
            }
        });
        FakeStdTurn { port, hits }
    }

    #[test]
    fn allocate_challenges_then_binds_relay() {
        let relay_listener = UdpSocket::bind("127.0.0.1:0").unwrap();
        let relay_port = relay_listener.local_addr().unwrap().port();
        let node = fake_std_turn("198.51.100.7", relay_port);
        let cred = TurnCredential {
            host: "127.0.0.1".into(),
            port: node.port,
            username: "user".into(),
            password: "pass".into(),
            expire: 0.0,
        };
        let cancelled = Arc::new(AtomicBool::new(false));
        let session = StdTurnSession::allocate(&cred, &cancelled, 4000).expect("分配应成功");
        assert_eq!(session.relay.address, "198.51.100.7");
        assert_eq!(session.relay.port, relay_port);
        assert_eq!(session.lifetime, 600);
        // 权限 + ChannelBind 两步都必须过
        session
            .bind(
                &RudpTarget {
                    address: "203.0.113.5".into(),
                    port: 25565,
                },
                &cancelled,
                4000,
            )
            .expect("bind 应成功");
        assert!(session.peer.lock().unwrap().is_some());
        // 节点侧应当只收到 4 个报文：裸 Allocate → 401 → 带签名 Allocate → Permission → ChannelBind
        // 挑战重试或重发都会推高计数，这条断言把发包序列钉死
        assert_eq!(*node.hits.lock().unwrap(), 4);
        session.close();
        drop(relay_listener);
    }

    #[test]
    fn fingerprint_covers_everything_before_itself() {
        let key = turn_long_term_key("user", "realm", "pass");
        let packet = TurnMessage::with_id(MSG_ALLOCATE, id())
            .put(ATTR_USERNAME, b"user")
            .build(Some(&key));
        let head = &packet[..packet.len() - 8];
        let value = u32::from_be_bytes([
            packet[packet.len() - 4],
            packet[packet.len() - 3],
            packet[packet.len() - 2],
            packet[packet.len() - 1],
        ]);
        // 指纹覆盖"除自身以外的整包"，取值放在整包最后 4 字节
        assert_eq!(value, crc32(head) ^ 0x5354_554e);
    }

    #[test]
    fn signed_packet_verifies_only_with_derived_key() {
        let key = turn_long_term_key("user", "realm", "pass");
        let packet = TurnMessage::with_id(MSG_ALLOCATE, id())
            .put(ATTR_USERNAME, b"user")
            .build(Some(&key));
        assert!(signed_ok(&packet, &key));
        assert!(!signed_ok(&packet, b"wrong-key"));
        // 篡改属性值：HMAC 与 FINGERPRINT 都必须失效
        let mut tampered = packet.clone();
        tampered[24] ^= 0x01;
        assert!(!signed_ok(&tampered, &key));
    }

    #[test]
    fn allocate_without_challenge_reports_original_text() {
        // 完全静默的端口：拿不到 401 挑战
        let listener = UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let cred = TurnCredential {
            host: "127.0.0.1".into(),
            port,
            username: "u".into(),
            password: "p".into(),
            expire: 0.0,
        };
        let cancelled = Arc::new(AtomicBool::new(false));
        let err = StdTurnSession::allocate(&cred, &cancelled, 1200).unwrap_err();
        assert_eq!(err, "标准 TURN 未返回有效鉴权挑战");
    }

    #[test]
    fn bind_rejects_bad_peer_before_any_packet() {
        let relay_listener = UdpSocket::bind("127.0.0.1:0").unwrap();
        let relay_port = relay_listener.local_addr().unwrap().port();
        let node = fake_std_turn("198.51.100.8", relay_port);
        let cred = TurnCredential {
            host: "127.0.0.1".into(),
            port: node.port,
            username: "user".into(),
            password: "pass".into(),
            expire: 0.0,
        };
        let cancelled = Arc::new(AtomicBool::new(false));
        let session = StdTurnSession::allocate(&cred, &cancelled, 4000).unwrap();
        let err = session
            .bind(
                &RudpTarget {
                    address: "不是地址".into(),
                    port: 25565,
                },
                &cancelled,
                2000,
            )
            .unwrap_err();
        assert_eq!(err, "对端 TURN 地址无效");
        session.close();
        drop(relay_listener);
    }
}
