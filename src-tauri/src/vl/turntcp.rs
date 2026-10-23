//! TURN over TCP 垫片（对应原版 voxlink/turnTcp.ts 46 行，LGPL-3.0-only）
//! 上游 AUGUHDAR/VoxLink TurnTcpChannel.java @721c7fae。
//!
//! 结构：RUDP 只跟 `client` 这块回环 UDP 说话；`shim` 收到就加 u16BE 长度前缀写进 TCP，
//! TCP 回来的字节按前缀切帧再投回 `client` 端口。Node 的 writableLength 在 Rust 里
//! 用"还没被 TCP 写线程取走的队列积压"等价实现。

use std::collections::VecDeque;
use std::io::{ErrorKind, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream, ToSocketAddrs, UdpSocket};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const TURN_TCP_CONNECT_TIMEOUT_MS: u64 = 3000;
pub const TURN_TCP_MAX_PACKET: usize = 2048;
pub const TURN_TCP_BACKPRESSURE_BYTES: usize = 1024 * 1024;
pub const TURN_TCP_TIMEOUT_CONNECT: &str = "TURN TCP 连接超时";
pub const TURN_TCP_TIMEOUT_CLOSED: &str = "TURN TCP 已关闭";
pub const TURN_TCP_TIMEOUT_CANCELLED: &str = "TURN TCP 已取消";

#[derive(Debug)]
pub struct TurnTcpChannel {
    pub socket: Arc<UdpSocket>,
    pub shim_addr: String,
    pub shim_port: u16,
    closed: Arc<Mutex<bool>>,
    keep: Vec<Arc<UdpSocket>>,
    shutdown: Arc<Mutex<Option<TcpStream>>>,
}

impl TurnTcpChannel {
    pub fn close(&self) {
        let mut guard = self.closed.lock().unwrap();
        if *guard {
            return;
        }
        *guard = true;
        if let Some(stream) = self.shutdown.lock().unwrap().as_ref() {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
    }

    pub fn is_closed(&self) -> bool {
        *self.closed.lock().unwrap()
    }
}

fn bind_loopback() -> std::io::Result<Arc<UdpSocket>> {
    Ok(Arc::new(UdpSocket::bind("127.0.0.1:0")?))
}

/// 对应 openTurnTcp：连接（3 秒超时）→ 两块回环 UDP → 起三个搬运线程
pub fn open_turn_tcp(host: &str, port: u16, cancelled: &Arc<Mutex<bool>>) -> Result<TurnTcpChannel, String> {
    if cancelled.lock().unwrap().to_owned() {
        return Err(TURN_TCP_TIMEOUT_CANCELLED.to_string());
    }
    let stream = connect_with_timeout(host, port, Duration::from_millis(TURN_TCP_CONNECT_TIMEOUT_MS), cancelled)?;
    stream.set_nodelay(true).map_err(|e| e.to_string())?;
    let client = bind_loopback().map_err(|e| e.to_string())?;
    let shim = bind_loopback().map_err(|e| e.to_string())?;
    let client_port = client.local_addr().map_err(|e| e.to_string())?.port();
    let shim_port = shim.local_addr().map_err(|e| e.to_string())?.port();

    let closed = Arc::new(Mutex::new(false));
    let writer_half = stream.try_clone().map_err(|e| e.to_string())?;
    let reader_half = stream.try_clone().map_err(|e| e.to_string())?;

    // shim → 队列 → TCP
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    let queue = Arc::new(Mutex::new(VecDeque::<usize>::new()));
    let total = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    {
        let closed = closed.clone();
        let shim = shim.clone();
        let queue = queue.clone();
        let total = total.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                if *closed.lock().unwrap() {
                    return;
                }
                let _ = shim.set_read_timeout(Some(Duration::from_millis(50)));
                match shim.recv_from(&mut buf) {
                    Ok((size, from)) => {
                        // 只接受来自 client 这块套接字的包，且必须 1..=2048
                        if from.ip().to_string() != "127.0.0.1"
                            || from.port() != client_port
                            || size == 0
                            || size > TURN_TCP_MAX_PACKET
                        {
                            continue;
                        }
                        if total.load(std::sync::atomic::Ordering::SeqCst) > TURN_TCP_BACKPRESSURE_BYTES {
                            *closed.lock().unwrap() = true;
                            return;
                        }
                        let mut frame = Vec::with_capacity(size + 2);
                        frame.extend_from_slice(&(size as u16).to_be_bytes());
                        frame.extend_from_slice(&buf[..size]);
                        queue.lock().unwrap().push_back(frame.len());
                        total.fetch_add(frame.len(), std::sync::atomic::Ordering::SeqCst);
                        if tx.send(frame).is_err() {
                            return;
                        }
                    }
                    Err(ref e) if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut => {}
                    Err(_) => return,
                }
            }
        });
    }
    {
        let mut writer = writer_half;
        let queue = queue.clone();
        let total = total.clone();
        let closed = closed.clone();
        std::thread::spawn(move || {
            while let Ok(frame) = rx.recv() {
                if *closed.lock().unwrap() {
                    return;
                }
                if writer.write_all(&frame).is_err() {
                    *closed.lock().unwrap() = true;
                    return;
                }
                let _ = writer.flush();
                if let Some(len) = queue.lock().unwrap().pop_front() {
                    total.fetch_sub(len, std::sync::atomic::Ordering::SeqCst);
                }
            }
        });
    }
    // TCP → 拆帧 → client
    {
        let closed = closed.clone();
        let shim = shim.clone();
        std::thread::spawn(move || {
            let mut reader = reader_half;
            let mut pending: Vec<u8> = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                if *closed.lock().unwrap() {
                    return;
                }
                match reader.read(&mut chunk) {
                    Ok(0) => {
                        *closed.lock().unwrap() = true;
                        return;
                    }
                    Ok(size) => {
                        pending.extend_from_slice(&chunk[..size]);
                        loop {
                            if pending.len() < 2 {
                                break;
                            }
                            let need = u16::from_be_bytes([pending[0], pending[1]]) as usize;
                            if need == 0 || need > TURN_TCP_MAX_PACKET {
                                *closed.lock().unwrap() = true;
                                return;
                            }
                            if pending.len() < need + 2 {
                                break;
                            }
                            let payload = pending[2..2 + need].to_vec();
                            pending.drain(..2 + need);
                            if shim.send_to(&payload, ("127.0.0.1", client_port)).is_err() {
                                return;
                            }
                        }
                    }
                    Err(ref e) if e.kind() == ErrorKind::Interrupted => continue,
                    Err(_) => {
                        *closed.lock().unwrap() = true;
                        return;
                    }
                }
            }
        });
    }
    Ok(TurnTcpChannel {
        socket: client,
        shim_addr: "127.0.0.1".to_string(),
        shim_port,
        closed: closed.clone(),
        keep: vec![shim],
        shutdown: Arc::new(Mutex::new(Some(stream))),
    })
}

fn connect_with_timeout(host: &str, port: u16, timeout: Duration, cancelled: &Arc<Mutex<bool>>) -> Result<TcpStream, String> {
    let addrs: Vec<SocketAddr> = match host.parse::<IpAddr>() {
        Ok(ip) => vec![SocketAddr::new(ip, port)],
        Err(_) => match (host, port).to_socket_addrs() {
            Ok(list) => list.collect(),
            Err(_) => return Err(TURN_TCP_TIMEOUT_CLOSED.to_string()),
        },
    };
    let deadline = Instant::now() + timeout;
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, timeout.min(Duration::from_secs(30))) {
            Ok(stream) => return Ok(stream),
            Err(_) => {
                if *cancelled.lock().unwrap() {
                    return Err(TURN_TCP_TIMEOUT_CANCELLED.to_string());
                }
                if Instant::now() >= deadline {
                    return Err(TURN_TCP_TIMEOUT_CONNECT.to_string());
                }
            }
        }
    }
    if Instant::now() >= deadline {
        return Err(TURN_TCP_TIMEOUT_CONNECT.to_string());
    }
    Err(TURN_TCP_TIMEOUT_CLOSED.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeTurn {
        port: u16,
        received: Arc<Mutex<Vec<Vec<u8>>>>,
        stop: Arc<Mutex<bool>>,
    }

    /// 假 TURN/TCP 服务端：收长度前缀帧，原样回投
    fn fake_turn_server(echo_back: bool) -> FakeTurn {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let received = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(Mutex::new(false));
        let sink = received.clone();
        let stop2 = stop.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let sink = sink.clone();
                let stop = stop2.clone();
                std::thread::spawn(move || {
                    let mut pending: Vec<u8> = Vec::new();
                    let mut buf = [0u8; 4096];
                    loop {
                        if *stop.lock().unwrap() {
                            return;
                        }
                        match stream.read(&mut buf) {
                            Ok(0) | Err(_) => return,
                            Ok(size) => {
                                pending.extend_from_slice(&buf[..size]);
                                loop {
                                    if pending.len() < 2 {
                                        break;
                                    }
                                    let need = u16::from_be_bytes([pending[0], pending[1]]) as usize;
                                    if pending.len() < need + 2 {
                                        break;
                                    }
                                    let payload = pending[2..2 + need].to_vec();
                                    pending.drain(..2 + need);
                                    sink.lock().unwrap().push(payload.clone());
                                    if echo_back {
                                        let mut frame = (need as u16).to_be_bytes().to_vec();
                                        frame.extend_from_slice(&payload);
                                        let _ = stream.write_all(&frame);
                                    }
                                }
                            }
                        }
                    }
                });
            }
        });
        FakeTurn { port, received, stop }
    }


    #[test]
    fn frames_out_with_big_endian_length_prefix() {
        let server = fake_turn_server(false);
        let cancelled = Arc::new(Mutex::new(false));
        let channel = open_turn_tcp("127.0.0.1", server.port, &cancelled).unwrap();
        channel.socket.send_to(b"hello", ("127.0.0.1", channel.shim_port)).unwrap();
        let deadline = Instant::now() + Duration::from_secs(4);
        while server.received.lock().unwrap().is_empty() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        let got = server.received.lock().unwrap().clone();
        assert_eq!(got, vec![b"hello".to_vec()], "UDP→TCP 必须整帧搬运且不截断");
        *server.stop.lock().unwrap() = true;
        channel.close();
    }

    #[test]
    fn inbound_frames_reach_the_client_socket() {
        let server = fake_turn_server(true);
        let cancelled = Arc::new(Mutex::new(false));
        let channel = open_turn_tcp("127.0.0.1", server.port, &cancelled).unwrap();
        channel.socket.send_to(b"ping", ("127.0.0.1", channel.shim_port)).unwrap();
        let _ = channel.keep;
        let mut buf = [0u8; 64];
        channel.socket.set_read_timeout(Some(Duration::from_secs(4))).unwrap();
        let (size, from) = channel.socket.recv_from(&mut buf).unwrap();
        assert_eq!(&buf[..size], b"ping", "TCP→UDP 拆帧必须回到 client");
        assert_eq!(from.port(), channel.shim_port, "回程要来自 shim 端口，RUDP 才认这个对端");
        *server.stop.lock().unwrap() = true;
        channel.close();
    }

    #[test]
    fn oversized_and_foreign_packets_are_dropped() {
        let server = fake_turn_server(false);
        let cancelled = Arc::new(Mutex::new(false));
        let channel = open_turn_tcp("127.0.0.1", server.port, &cancelled).unwrap();
        let other = UdpSocket::bind("127.0.0.1:0").unwrap();
        // 超过 2048 的包：原版直接 return，不转发也不关
        other.send_to(&vec![7u8; 3000], ("127.0.0.1", channel.shim_port)).unwrap();
        // 来自别的端口的包：同样忽略
        channel.socket.send_to(b"ok?", ("127.0.0.1", channel.shim_port)).unwrap();
        std::thread::sleep(Duration::from_millis(400));
        let got = server.received.lock().unwrap().clone();
        assert_eq!(got, vec![b"ok?".to_vec()], "只接受 client 端口发来的 1..=2048 字节包");
        assert!(!channel.is_closed());
        *server.stop.lock().unwrap() = true;
        channel.close();
    }

    #[test]
    fn connect_failure_reports_the_original_texts() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let cancelled = Arc::new(Mutex::new(false));
        let err = open_turn_tcp("127.0.0.1", port, &cancelled).unwrap_err();
        assert!([TURN_TCP_TIMEOUT_CONNECT, TURN_TCP_TIMEOUT_CLOSED].contains(&err.as_str()), "实际={}", err);
        let flag = Arc::new(Mutex::new(true));
        assert_eq!(open_turn_tcp("127.0.0.1", port, &flag).unwrap_err(), TURN_TCP_TIMEOUT_CANCELLED);
    }
}
