//! TCP 同时打开打洞 + 打洞成功后的本地对接（对应原版 voxlink/tcpPunch.ts 69 行，LGPL-3.0-only）
//! 上游 AUGUHDAR/VoxLink TcpHolePuncher / P2PBridge, revision 721c7fae。
//!
//! 语义要点（逐条对齐原版，不许"顺手改"）：
//! 1. 同时打开靠的是 `connect({host, port, localPort: port})`——本地端口钉死等于对端端口；
//! 2. host 侧最多 5 次尝试、guest 侧 1 次，两次尝试之间随机退避 50~250ms；
//! 3. 尝试用尽后两侧都会退化成"在本机监听同一个端口"，谁先 accept 到对方谁赢，10 秒不中就报未命中；
//! 4. accept 后必须核对来源地址，地址不符直接拆掉继续等（原版 tcpPunch.ts:53）。

use std::io::{ErrorKind, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// tcpPunch.ts:15 总预算 10 秒
pub const TCP_PUNCH_BUDGET_MS: u64 = 10_000;
/// tcpPunch.ts:16 host 侧 5 次、guest 侧 1 次
pub const TCP_PUNCH_HOST_ATTEMPTS: u32 = 5;
/// tcpPunch.ts:23 单次连接超时 3 秒（且不超过总预算）
pub const TCP_PUNCH_CONNECT_MS: u64 = 3_000;
/// tcpPunch.ts:38 退避 50 + rand(200)
pub const TCP_PUNCH_RETRY_MIN_MS: u64 = 50;
pub const TCP_PUNCH_RETRY_SPAN_MS: u64 = 200;
/// tcpPunch.ts:45 回退监听后的命中窗口 10 秒
pub const TCP_PUNCH_HIT_MS: u64 = 10_000;
/// tcpPunch.ts:62 本地游戏连接超时 10 秒
pub const TCP_PUNCH_LOCAL_MS: u64 = 10_000;
/// tcpPunch.ts:34 setKeepAlive(true, 10000)
pub const TCP_PUNCH_KEEPALIVE_MS: u64 = 10_000;
/// tcpPunch.ts:14 同时打开的本地端口下限
pub const TCP_PUNCH_MIN_PORT: u16 = 1024;

/// 对应 chooseTcpPunchPort()：借一个临时监听端口，用完立刻还回去
pub fn choose_tcp_punch_port() -> Result<u16, String> {
    let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    drop(listener);
    Ok(port)
}

/// 原版 `replace(/^::ffff:/,'')`：IPv4 映射地址要和裸 IPv4 比成同一个
fn normalize(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => IpAddr::V4(v4),
            None => IpAddr::V6(v6),
        },
        other => other,
    }
}

fn sleep_cancellable(total: Duration, cancelled: &AtomicBool) -> bool {
    let end = Instant::now() + total;
    loop {
        if cancelled.load(Ordering::SeqCst) {
            return false;
        }
        let left = end.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return true;
        }
        std::thread::sleep(left.min(Duration::from_millis(10)));
    }
}

/// 对应 tcpSimOpen。`cancelled` 等价于原版的 AbortSignal。
pub fn tcp_sim_open(ip: &str, port: u16, host: bool, cancelled: &AtomicBool) -> Result<TcpStream, String> {
    let Ok(remote_ip) = ip.parse::<IpAddr>() else {
        return Err("TCP 打洞地址无效".into());
    };
    if port < TCP_PUNCH_MIN_PORT {
        return Err("TCP 打洞地址无效".into());
    }
    let deadline = Instant::now() + Duration::from_millis(TCP_PUNCH_BUDGET_MS);
    let attempts = if host { TCP_PUNCH_HOST_ATTEMPTS } else { 1 };
    let mut attempt = 0u32;
    while attempt < attempts && Instant::now() < deadline {
        attempt += 1;
        if cancelled.load(Ordering::SeqCst) {
            return Err("TCP 打洞已取消".into());
        }
        let budget = TCP_PUNCH_CONNECT_MS.min(deadline.saturating_duration_since(Instant::now()).as_millis() as u64);
        match connect_from_local(SocketAddr::new(remote_ip, port), port, Duration::from_millis(budget)) {
            Ok(stream) => {
                if cancelled.load(Ordering::SeqCst) {
                    drop(stream);
                    return Err("TCP 打洞已取消".into());
                }
                let _ = stream.set_nodelay(true);
                return Ok(stream);
            }
            Err(_) => {
                if cancelled.load(Ordering::SeqCst) {
                    return Err("TCP 打洞已取消".into());
                }
            }
        }
        let jitter = rand::Rng::random_range(&mut rand::rng(), 0..TCP_PUNCH_RETRY_SPAN_MS);
        if !sleep_cancellable(Duration::from_millis(TCP_PUNCH_RETRY_MIN_MS + jitter), cancelled) {
            return Err("TCP 打洞已取消".into());
        }
    }
    if cancelled.load(Ordering::SeqCst) {
        return Err("TCP 打洞已取消".into());
    }
    // 尝试用尽：两侧都改成监听同一个端口，等对方连进来
    let any: SocketAddr = if remote_ip.is_ipv6() {
        (std::net::Ipv6Addr::UNSPECIFIED, port).into()
    } else {
        (Ipv4Addr::UNSPECIFIED, port).into()
    };
    let listener = TcpListener::bind(any).map_err(|e| e.to_string())?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let deadline = Instant::now() + Duration::from_millis(TCP_PUNCH_HIT_MS);
    loop {
        if cancelled.load(Ordering::SeqCst) {
            drop(listener);
            return Err("TCP 打洞已取消".into());
        }
        if Instant::now() >= deadline {
            drop(listener);
            return Err("TCP 打洞未命中".into());
        }
        match listener.accept() {
            Ok((stream, peer)) => {
                // Windows 上 accept 出来的连接继承监听套接字的非阻塞模式，必须纠正回来
                let _ = stream.set_nonblocking(false);
                if normalize(peer.ip()) != normalize(remote_ip) {
                    drop(stream);
                    continue;
                }
                drop(listener);
                let _ = stream.set_nodelay(true);
                return Ok(stream);
            }
            Err(err) if err.kind() == ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(10)),
            Err(err) => {
                drop(listener);
                return Err(err.to_string());
            }
        }
    }
}

/// 一次性把 socket 走完"绑本地端口 → 非阻塞 connect → select 等结果"
#[cfg(windows)]
fn connect_from_local(addr: SocketAddr, local_port: u16, timeout: Duration) -> Result<TcpStream, String> {
    use std::os::windows::io::{FromRawSocket, RawSocket};
    use windows_sys::core::PSTR;
    use windows_sys::Win32::Networking::WinSock::{
        bind, closesocket, connect, getsockopt, ioctlsocket, select, socket, WSAGetLastError, FD_SET, IN6_ADDR, IN6_ADDR_0, IN_ADDR, IN_ADDR_0, INVALID_SOCKET, IPPROTO_TCP,
        SOCKADDR, SOCKADDR_IN, SOCKADDR_IN6, SOCKADDR_IN6_0, SOCKET_ERROR, SOCK_STREAM, TIMEVAL, AF_INET, AF_INET6, FIONBIO, SOL_SOCKET, SO_ERROR, WSAEWOULDBLOCK,
    };

    let af = if addr.is_ipv6() { AF_INET6 } else { AF_INET };
    let sock = unsafe { socket(af as i32, SOCK_STREAM, IPPROTO_TCP as i32) };
    if sock == INVALID_SOCKET {
        return Err(format!("TCP 打洞套接字创建失败 ({})", unsafe { WSAGetLastError() }));
    }
    // 本地端口的意义就在于它等于对端端口，这一步失败就没有同时打开可言
    let bound = unsafe {
        match addr {
            SocketAddr::V4(_) => {
                let local = SOCKADDR_IN {
                    sin_family: AF_INET,
                    sin_port: local_port.to_be(),
                    sin_addr: IN_ADDR { S_un: IN_ADDR_0 { S_addr: 0 } },
                    sin_zero: [0; 8],
                };
                bind(sock, &local as *const SOCKADDR_IN as *const SOCKADDR, std::mem::size_of::<SOCKADDR_IN>() as i32)
            }
            SocketAddr::V6(_) => {
                let local = SOCKADDR_IN6 {
                    sin6_family: AF_INET6,
                    sin6_port: local_port.to_be(),
                    sin6_flowinfo: 0,
                    sin6_addr: IN6_ADDR { u: IN6_ADDR_0 { Byte: [0; 16] } },
                    Anonymous: SOCKADDR_IN6_0 { sin6_scope_id: 0 },
                };
                bind(sock, &local as *const SOCKADDR_IN6 as *const SOCKADDR, std::mem::size_of::<SOCKADDR_IN6>() as i32)
            }
        }
    };
    if bound == SOCKET_ERROR {
        let code = unsafe { WSAGetLastError() };
        unsafe { closesocket(sock) };
        return Err(format!("TCP 打洞本地端口绑定失败 ({})", code));
    }
    let mut nonblocking = 1u32;
    unsafe { ioctlsocket(sock, FIONBIO, &mut nonblocking) };
    let mut rc = unsafe {
        match addr {
            SocketAddr::V4(v4) => {
                // S_addr 是网络字节序：内存里就是四段点分十进制的顺序
                let remote = SOCKADDR_IN {
                    sin_family: AF_INET,
                    sin_port: addr.port().to_be(),
                    sin_addr: IN_ADDR { S_un: IN_ADDR_0 { S_addr: u32::from_ne_bytes(v4.ip().octets()) } },
                    sin_zero: [0; 8],
                };
                connect(sock, &remote as *const SOCKADDR_IN as *const SOCKADDR, std::mem::size_of::<SOCKADDR_IN>() as i32)
            }
            SocketAddr::V6(v6) => {
                let remote = SOCKADDR_IN6 {
                    sin6_family: AF_INET6,
                    sin6_port: addr.port().to_be(),
                    sin6_flowinfo: 0,
                    sin6_addr: IN6_ADDR { u: IN6_ADDR_0 { Byte: v6.ip().octets() } },
                    Anonymous: SOCKADDR_IN6_0 { sin6_scope_id: 0 },
                };
                connect(sock, &remote as *const SOCKADDR_IN6 as *const SOCKADDR, std::mem::size_of::<SOCKADDR_IN6>() as i32)
            }
        }
    };
    if rc == SOCKET_ERROR && unsafe { WSAGetLastError() } == WSAEWOULDBLOCK {
        let ms = timeout.as_millis() as u64;
        let tv = TIMEVAL { tv_sec: (ms / 1000) as i32, tv_usec: ((ms % 1000) * 1000) as i32 };
        let mut write_set = FD_SET { fd_count: 1, fd_array: [sock; 64] };
        let mut except_set = FD_SET { fd_count: 1, fd_array: [sock; 64] };
        rc = unsafe { select(0, std::ptr::null_mut(), &mut write_set, &mut except_set, &tv) };
        if rc == 0 {
            unsafe { closesocket(sock) };
            return Err("TCP 同时打开超时".into());
        }
        if rc == SOCKET_ERROR || except_set.fd_count > 0 || write_set.fd_count == 0 {
            let code = unsafe { WSAGetLastError() };
            unsafe { closesocket(sock) };
            return Err(format!("TCP 同时打开已关闭 ({})", code));
        }
        let mut so_error = 0i32;
        let mut len = std::mem::size_of::<i32>() as i32;
        unsafe { getsockopt(sock, SOL_SOCKET, SO_ERROR, &mut so_error as *mut i32 as PSTR, &mut len) };
        if so_error != 0 {
            unsafe { closesocket(sock) };
            return Err(format!("TCP 同时打开已关闭 ({})", so_error));
        }
    } else if rc == SOCKET_ERROR {
        let code = unsafe { WSAGetLastError() };
        unsafe { closesocket(sock) };
        return Err(format!("TCP 同时打开已关闭 ({})", code));
    }
    let stream = unsafe { TcpStream::from_raw_socket(sock as RawSocket) };
    let _ = stream.set_nonblocking(false);
    Ok(stream)
}

/// 非 Windows 平台不发布，这里保留可用的普通连接（不钉本地端口）
#[cfg(not(windows))]
fn connect_from_local(addr: SocketAddr, _local_port: u16, timeout: Duration) -> Result<TcpStream, String> {
    TcpStream::connect_timeout(&addr, timeout).map_err(|e| e.to_string())
}

type DownCb = Arc<dyn Fn() + Send + Sync>;

struct PunchedInner {
    peer: MutexOption,
    local: MutexOption,
    listener: std::sync::Mutex<Option<TcpListener>>,
    stopped: AtomicBool,
    on_down: DownCb,
}

type MutexOption = std::sync::Mutex<Option<TcpStream>>;

fn stop_punched(inner: &Arc<PunchedInner>) {
    if inner.stopped.swap(true, Ordering::SeqCst) {
        return;
    }
    if let Some(listener) = inner.listener.lock().unwrap().take() {
        drop(listener);
    }
    for socket in [inner.peer.lock().unwrap().take(), inner.local.lock().unwrap().take()] {
        if let Some(socket) = socket {
            let _ = socket.shutdown(std::net::Shutdown::Both);
        }
    }
    (inner.on_down)();
}

/// 对应 pair()：只认第一条本地连接，后来的直接拆掉
fn pair_punched(inner: &Arc<PunchedInner>, socket: TcpStream) {
    if inner.stopped.load(Ordering::SeqCst) || inner.local.lock().unwrap().is_some() {
        drop(socket);
        return;
    }
    let _ = socket.set_nodelay(true);
    let peer = inner.peer.lock().unwrap().as_ref().and_then(|peer| peer.try_clone().ok());
    let Some(peer) = peer else {
        drop(socket);
        stop_punched(inner);
        return;
    };
    let local = socket.try_clone();
    match local {
        Ok(handle) => {
            *inner.local.lock().unwrap() = Some(handle);
            spawn_pump(inner, peer, socket);
        }
        Err(_) => {
            drop(socket);
            stop_punched(inner);
        }
    }
}

/// `peer.pipe(socket); socket.pipe(peer)`：两个方向各一条线程，任一先结束就停
fn spawn_pump(inner: &Arc<PunchedInner>, peer: TcpStream, local: TcpStream) {
    let (Ok(peer_reader), Ok(local_writer)) = (peer.try_clone(), local.try_clone()) else {
        stop_punched(inner);
        return;
    };
    pump_direction(inner.clone(), peer_reader, local_writer);
    pump_direction(inner.clone(), local, peer);
}

fn pump_direction(inner: Arc<PunchedInner>, mut reader: TcpStream, mut writer: TcpStream) {
    std::thread::spawn(move || {
        let mut buf = [0u8; 32768];
        loop {
            if inner.stopped.load(Ordering::SeqCst) {
                break;
            }
            let size = match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(size) => size,
                Err(err) if err.kind() == ErrorKind::Interrupted => continue,
                Err(_) => break,
            };
            if writer.write_all(&buf[..size]).is_err() {
                break;
            }
        }
        stop_punched(&inner);
    });
}

/// 对应 bridgePunchedSocket 的返回形状
pub struct PunchedBridge {
    /// guest 侧给游戏用的 `127.0.0.1:<port>`；host 侧原版返回空串
    pub address: String,
    inner: Arc<PunchedInner>,
}

impl PunchedBridge {
    pub fn stop(&self) {
        stop_punched(&self.inner);
    }
}

impl std::fmt::Debug for PunchedBridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PunchedBridge").field("address", &self.address).field("stopped", &self.inner.stopped.load(Ordering::SeqCst)).finish()
    }
}

/// tcpPunch.ts:34 `setKeepAlive(true, 10000)`：Windows 走 SIO_KEEPALIVE_VALS，
/// 空闲阈值按原版 10 秒、探测间隔 1 秒（libuv 的 TCP_KEEPALIVE_INTERVAL）
fn set_punch_keepalive(socket: &TcpStream) {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawSocket;
        use windows_sys::Win32::Networking::WinSock::{tcp_keepalive, WSAIoctl, SIO_KEEPALIVE_VALS};
        let mut value = tcp_keepalive { onoff: 1, keepalivetime: TCP_PUNCH_KEEPALIVE_MS as u32, keepaliveinterval: 1000 };
        let mut returned = 0u32;
        unsafe {
            WSAIoctl(
                socket.as_raw_socket() as usize,
                SIO_KEEPALIVE_VALS,
                &mut value as *mut tcp_keepalive as *mut core::ffi::c_void,
                std::mem::size_of::<tcp_keepalive>() as u32,
                std::ptr::null_mut(),
                0,
                &mut returned,
                std::ptr::null_mut(),
                None,
            )
        };
    }
    #[cfg(not(windows))]
    {
        let _ = socket.set_keepalive(Some(Duration::from_millis(TCP_PUNCH_KEEPALIVE_MS)));
    }
}

/// hostPort 有值＝主动连本地游戏；否则在 127.0.0.1 临时端口等游戏连进来。
/// 原版这里不给 peer 设 nodelay——那一步在 tcpSimOpen 成功时已经做过。
pub fn bridge_punched_socket(peer: TcpStream, host_port: Option<u16>, on_down: DownCb) -> Result<PunchedBridge, String> {
    set_punch_keepalive(&peer);
    let inner = Arc::new(PunchedInner {
        peer: std::sync::Mutex::new(Some(peer)),
        local: std::sync::Mutex::new(None),
        listener: std::sync::Mutex::new(None),
        stopped: AtomicBool::new(false),
        on_down,
    });
    if let Some(port) = host_port {
        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
        match connect_timeout(address, Duration::from_millis(TCP_PUNCH_LOCAL_MS)) {
            Ok(local) => {
                pair_punched(&inner, local);
                Ok(PunchedBridge { address: String::new(), inner })
            }
            Err(error) => {
                stop_punched(&inner);
                Err(error)
            }
        }
    } else {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|e| e.to_string())?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        *inner.listener.lock().unwrap() = Some(listener);
        let scanning = inner.clone();
        std::thread::spawn(move || loop {
            if scanning.stopped.load(Ordering::SeqCst) {
                break;
            }
            let accepted = {
                let guard = scanning.listener.lock().unwrap();
                match guard.as_ref() {
                    Some(listener) => listener.accept(),
                    None => break,
                }
            };
            match accepted {
                Ok((socket, _)) => {
                    let _ = socket.set_nonblocking(false);
                    pair_punched(&scanning, socket);
                }
                Err(err) if err.kind() == ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(10)),
                Err(_) => {
                    stop_punched(&scanning);
                    break;
                }
            }
        });
        Ok(PunchedBridge { address: format!("127.0.0.1:{port}"), inner })
    }
}

/// 本地游戏连接：普通拨号，但要按原版的 10 秒窗口等结果
/// （复用 connect_from_local，local_port=0 即让系统挑本地端口）
fn connect_timeout(address: SocketAddr, timeout: Duration) -> Result<TcpStream, String> {
    let stream = connect_from_local(address, 0, timeout)?;
    let _ = stream.set_nodelay(true);
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    fn loopback_pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let dialer = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let (accepted, _) = listener.accept().unwrap();
        drop(listener);
        (accepted, dialer)
    }

    fn closed_port() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        port
    }

    fn until(flag: &AtomicBool, ms: u64) -> bool {
        let end = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < end {
            if flag.load(Ordering::SeqCst) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    #[test]
    fn chosen_port_is_given_back() {
        let port = choose_tcp_punch_port().unwrap();
        assert!(port >= TCP_PUNCH_MIN_PORT, "端口要能用于同时打开");
        // 临时监听必须已经关掉，否则同一个端口再绑不上
        assert!(TcpListener::bind(("127.0.0.1", port)).is_ok());
    }

    #[test]
    fn mapped_ipv6_compares_as_ipv4() {
        let mapped: IpAddr = "::ffff:203.0.113.9".parse().unwrap();
        assert_eq!(normalize(mapped), "203.0.113.9".parse::<IpAddr>().unwrap());
        assert_ne!(normalize("::1".parse().unwrap()), mapped);
    }

    #[test]
    fn sim_open_rejects_bad_arguments_before_any_socket() {
        let cancelled = AtomicBool::new(false);
        assert_eq!(tcp_sim_open("不是一个地址", 25565, true, &cancelled).unwrap_err(), "TCP 打洞地址无效");
        assert_eq!(tcp_sim_open("127.0.0.1", 80, true, &cancelled).unwrap_err(), "TCP 打洞地址无效");
    }

    #[test]
    fn sim_open_checks_cancellation_first() {
        let cancelled = AtomicBool::new(true);
        assert_eq!(tcp_sim_open("127.0.0.1", 25565, true, &cancelled).unwrap_err(), "TCP 打洞已取消");
        assert_eq!(tcp_sim_open("127.0.0.1", 25565, false, &cancelled).unwrap_err(), "TCP 打洞已取消");
    }

    #[test]
    fn punched_socket_keeps_the_requested_local_port() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let remote_port = listener.local_addr().unwrap().port();
        let local_port = choose_tcp_punch_port().unwrap();
        let accepted = std::thread::spawn(move || listener.accept());
        let remote = "127.0.0.1".parse::<IpAddr>().unwrap();
        let stream = connect_from_local(SocketAddr::new(remote, remote_port), local_port, Duration::from_secs(3)).expect("按指定本地端口连接应当成功");
        assert_eq!(stream.local_addr().unwrap().port(), local_port, "本地端口必须是被钉住的那个，否则谈不上同时打开");
        let (_peer, from) = accepted.join().unwrap().unwrap();
        assert_eq!(from.port(), local_port, "对端看到的来源端口也要是同一个");
    }

    #[test]
    fn fallback_listener_refuses_a_non_matching_source() {
        // 目标取 TEST-NET 地址：本机不可能以它作为来源，因此回退监听起来后本地拨号必须被拒收
        let port = choose_tcp_punch_port().unwrap();
        let cancelled = AtomicBool::new(false);
        let reached = Arc::new(AtomicBool::new(false));
        let dial_flag = reached.clone();
        let dialer = std::thread::spawn(move || {
            let end = Instant::now() + Duration::from_secs(9);
            while Instant::now() < end {
                if let Ok(stream) = TcpStream::connect(("127.0.0.1", port)) {
                    dial_flag.store(true, Ordering::SeqCst);
                    drop(stream);
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        });
        let started = Instant::now();
        let error = tcp_sim_open("192.0.2.1", port, false, &cancelled).unwrap_err();
        let _ = dialer.join();
        assert_eq!(error, "TCP 打洞未命中");
        assert!(reached.load(Ordering::SeqCst), "回退阶段应当真的在本机监听同一个端口");
        assert!(started.elapsed() >= Duration::from_millis(TCP_PUNCH_HIT_MS), "命中窗口是 10 秒，不能提前收工");
    }

    #[test]
    fn host_bridge_reports_local_game_absence_and_fires_down() {
        let (peer, _remote) = loopback_pair();
        let down = Arc::new(AtomicBool::new(false));
        let flag = down.clone();
        let error = bridge_punched_socket(peer, Some(closed_port()), Arc::new(move || flag.store(true, Ordering::SeqCst))).unwrap_err();
        assert!(!error.is_empty(), "原版把 socket 错误原样抛出");
        assert!(down.load(Ordering::SeqCst), "失败路径同样要 stop() → onDown");
    }

    #[test]
    fn host_bridge_pumps_both_directions() {
        let game = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = game.local_addr().unwrap().port();
        let (peer, mut remote) = loopback_pair();
        let bridge = bridge_punched_socket(peer, Some(port), Arc::new(|| {})).unwrap();
        assert_eq!(bridge.address, "", "host 侧原版返回空串");
        let mut local = game.accept().unwrap().0;
        remote.write_all(b"MC").unwrap();
        let mut buf = [0u8; 2];
        local.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"MC");
        local.write_all(b"VT").unwrap();
        remote.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"VT");
    }

    #[test]
    fn guest_bridge_only_pairs_the_first_local_connection() {
        let (peer, mut remote) = loopback_pair();
        let bridge = bridge_punched_socket(peer, None, Arc::new(|| {})).unwrap();
        assert!(bridge.address.starts_with("127.0.0.1:"), "guest 侧要给出可连的本地地址");
        let mut game = TcpStream::connect(bridge.address.as_str()).unwrap();
        remote.write_all(b"up").unwrap();
        let mut buf = [0u8; 2];
        game.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"up");
        // 第二条本地连接必须被拆掉：local 槽位只有一个
        let mut extra = TcpStream::connect(bridge.address.as_str()).unwrap();
        extra.set_read_timeout(Some(Duration::from_millis(1500))).unwrap();
        let mut probe = [0u8; 1];
        assert!(!matches!(extra.read(&mut probe), Ok(1)), "多余的本地连接不该收到数据");
        // 主通道不受影响
        game.write_all(b"down").unwrap();
        remote.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"do");
    }

    #[test]
    fn peer_shutdown_tears_the_bridge_down_once() {
        let (peer, remote) = loopback_pair();
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        let bridge = bridge_punched_socket(peer, None, Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }))
        .unwrap();
        let _game = TcpStream::connect(bridge.address.as_str()).unwrap();
        bridge.stop();
        bridge.stop();
        assert_eq!(hits.load(Ordering::SeqCst), 1, "stop 必须幂等");
        assert!(TcpStream::connect(bridge.address.as_str()).is_err(), "stop 之后本地监听要收掉");
        drop(remote);
    }

    #[test]
    fn peer_end_of_stream_stops_the_bridge() {
        let (peer, remote) = loopback_pair();
        let down = Arc::new(AtomicBool::new(false));
        let flag = down.clone();
        let bridge = bridge_punched_socket(peer, None, Arc::new(move || flag.store(true, Ordering::SeqCst))).unwrap();
        let _game = TcpStream::connect(bridge.address.as_str()).unwrap();
        drop(remote);
        assert!(until(&down, 3000), "任一方向先结束即停（tcpPunch.ts:36）");
    }
}

