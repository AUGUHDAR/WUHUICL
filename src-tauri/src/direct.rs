//! 局域网直连房间（对应原版 core/directConnect.ts + core/directProtocol.ts + core/directUpnp.ts）。
//! 通道：direct:overview / direct:host / direct:stop / direct:state / direct:resolve / direct:prepareJoin。

use base64::Engine;
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::net::{
    IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream, ToSocketAddrs, UdpSocket,
};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::AppHandle;

const PROBE_TIMEOUT_MS: u64 = 2500;
const FORWARD_MAX_CONNECTIONS: usize = 32;
const FORWARD_IDLE_MS: u64 = 120000;
const FORWARD_UPSTREAM_MS: u64 = 8000;
const HOST_WATCHDOG_MS: u64 = 15000;
const HOST_RENEW_MS: i64 = 120000;
const INVITE_TTL_MS: i64 = 24 * 3600000;
const INVITE_PREFIX: &str = "KAMUCL-DIRECT-1:";
const INVITE_MAX_LEN: usize = 16384;
const INVITE_MAX_ENDPOINTS: usize = 12;
const HOST_ENDPOINT_LIMIT: usize = 10;
const HOST_UNIQUE_LIMIT: usize = 12;
const HOST_UPNP_GATEWAY_LIMIT: usize = 2;
const SSDP_ADDR: &str = "239.255.255.250:1900";
const SSDP_TIMEOUT_MS: u64 = 2200;
const SSDP_MAX_LOCATIONS: usize = 12;
const SSDP_MAX_PACKET: usize = 16384;
const SSDP_IFACE_LIMIT: usize = 8;
const SSDP_ST1: &str = "urn:schemas-upnp-org:device:InternetGatewayDevice:1";
const SSDP_ST2: &str = "urn:schemas-upnp-org:device:InternetGatewayDevice:2";
const UPNP_MAX_XML: usize = 512 * 1024;
const UPNP_HTTP_TIMEOUT_MS: u64 = 5000;
const LAN_LOG_TAIL: usize = 256 * 1024;
const PORT_INVALID: &str = "端口必须是 1–65535 的整数";
const ROUTER_ADDR_INVALID: &str = "路由器地址必须来自同一本地网络设备";
const UPNP_SCOPE_RE: &str = r"^urn:schemas-upnp-org:service:WAN(?:IP|PPP)Connection:[12]$";

#[derive(Clone)]
pub struct Gateway {
    control_url: String,
    service_type: String,
    local_address: String,
    external_address: String,
}

pub struct UpnpErr {
    code: String,
    message: String,
}

impl From<String> for UpnpErr {
    fn from(message: String) -> Self {
        UpnpErr { code: String::new(), message }
    }
}

impl From<&str> for UpnpErr {
    fn from(message: &str) -> Self {
        UpnpErr { code: String::new(), message: message.to_string() }
    }
}

struct Session {
    state: Arc<Mutex<Value>>,
    connections: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    mappings: Arc<Mutex<Vec<Mapping>>>,
    forwarder: Arc<Forwarder>,
}

struct Op {
    abort: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
}

static NETWORK: Mutex<Option<Value>> = Mutex::new(None);
static GATEWAYS: Mutex<Vec<Gateway>> = Mutex::new(Vec::new());
static LAST_MESSAGES: Mutex<Vec<String>> = Mutex::new(Vec::new());
static SESSION: Mutex<Option<Arc<Session>>> = Mutex::new(None);
static OPERATION: Mutex<Option<Arc<Op>>> = Mutex::new(None);
static SCAN: Mutex<()> = Mutex::new(());

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn iso_now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn iso_at(millis: i64) -> String {
    chrono::DateTime::from_timestamp_millis(millis)
        .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .unwrap_or_default()
}

fn upnp_client() -> &'static reqwest::blocking::Client {
    static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_millis(UPNP_HTTP_TIMEOUT_MS))
            .build()
            .expect("UPnP HTTP 客户端初始化失败")
    })
}

// 对应原版 core/directProtocol.ts ipv4Scope / isGlobalIPv6 / endpointAddress / validatePort / detectLanPort / probeTcp
fn ipv4_scope(ip: &str) -> &'static str {
    let Ok(v4) = ip.parse::<Ipv4Addr>() else {
        return "reserved";
    };
    let [a, b, c, _] = v4.octets();
    if a == 100 && (64..=127).contains(&b) {
        return "cgnat";
    }
    if a == 10 || (a == 172 && (16..=31).contains(&b)) || (a == 192 && b == 168) {
        return "private";
    }
    if a == 0
        || a == 127
        || a >= 224
        || (a == 169 && b == 254)
        || (a == 192 && b == 0 && (c == 0 || c == 2))
        || (a == 198 && (b == 18 || b == 19 || (b == 51 && c == 100)))
        || (a == 203 && b == 0 && c == 113)
    {
        return "reserved";
    }
    "public"
}

fn is_global_ipv6(ip: &str) -> bool {
    let Ok(v6) = ip.parse::<Ipv6Addr>() else {
        return false;
    };
    let normalized = v6.to_string();
    let second = normalized
        .split(':')
        .nth(1)
        .and_then(|s| u32::from_str_radix(s, 16).ok())
        .unwrap_or(0);
    (normalized.starts_with('2') || normalized.starts_with('3'))
        && !normalized.starts_with("2001:db8:")
        && !(normalized.starts_with("2001:") && second < 0x200)
        && !normalized.starts_with("2002:")
        && !normalized.starts_with("3fff:")
}

fn endpoint_address(host: &str, port: u16) -> String {
    if host.parse::<Ipv6Addr>().is_ok() {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

fn endpoint_text(item: &Value) -> String {
    endpoint_address(
        item["host"].as_str().unwrap_or(""),
        item["port"].as_u64().unwrap_or(0) as u16,
    )
}

fn validate_port(value: &Value) -> Result<u16, String> {
    let n = match value.as_i64() {
        Some(n) => n,
        None => match value.as_f64() {
            Some(f) if f == f.trunc() => f as i64,
            _ => return Err(PORT_INVALID.into()),
        },
    };
    if n < 1 || n > 65535 {
        return Err(PORT_INVALID.into());
    }
    Ok(n as u16)
}

fn detect_lan_port(log: &str) -> Option<u16> {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(r"(?i)(?:Started serving on|Local game hosted on port|本地游戏已在端口)\s*:?\s*(\d{1,5})").unwrap()
    });
    let last = re.captures_iter(log).last()?;
    let port: i64 = last[1].parse().ok()?;
    if port > 0 && port <= 65535 {
        Some(port as u16)
    } else {
        None
    }
}

fn resolve_first(host: &str, port: u16) -> Option<SocketAddr> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Some(SocketAddr::new(ip, port));
    }
    (host, port).to_socket_addrs().ok()?.next()
}

fn probe_tcp(host: &str, port: u16, abort: Option<&AtomicBool>) -> bool {
    if abort.map(|a| a.load(Ordering::SeqCst)).unwrap_or(false) {
        return false;
    }
    let Some(addr) = resolve_first(host, port) else {
        return false;
    };
    TcpStream::connect_timeout(&addr, Duration::from_millis(PROBE_TIMEOUT_MS)).is_ok()
}

fn local_source_address(ip: IpAddr, port: u16) -> String {
    let bind = if ip.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" };
    let Ok(sock) = UdpSocket::bind(bind) else {
        return String::new();
    };
    if sock.connect((ip, port)).is_err() {
        return String::new();
    }
    sock.local_addr().map(|a| a.ip().to_string()).unwrap_or_default()
}

fn url_host(input: &str) -> String {
    reqwest::Url::parse(input)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .unwrap_or_default()
}

struct IfaceAddr {
    name: String,
    address: String,
    v6: bool,
    internal: bool,
}

#[cfg(windows)]
fn utf16_to_string(ptr: *mut u16) -> String {
    if ptr.is_null() {
        return String::new();
    }
    let mut len = 0usize;
    while unsafe { *ptr.add(len) } != 0 {
        len += 1;
    }
    String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(ptr, len) })
}

#[cfg(windows)]
fn ansi_to_string(ptr: *const u8) -> String {
    if ptr.is_null() {
        return String::new();
    }
    let mut len = 0usize;
    while unsafe { *ptr.add(len) } != 0 {
        len += 1;
    }
    String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(ptr, len) }).to_string()
}

// Node os.networkInterfaces() 走的是 GetAdaptersAddresses：网卡顺序与每个网卡内的地址顺序都由系统给出，
// 换用 PowerShell 会重排顺序并把中文网卡名按码页解码成乱码，所以这里直接调同一个 API。
#[cfg(windows)]
fn network_interfaces() -> Vec<IfaceAddr> {
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetAdaptersAddresses, IP_ADAPTER_ADDRESSES_LH, IP_ADAPTER_UNICAST_ADDRESS_LH,
    };
    use windows_sys::Win32::Networking::WinSock::{AF_INET, AF_INET6};
    const ERROR_SUCCESS: u32 = 0;
    const ERROR_BUFFER_OVERFLOW: u32 = 111;
    const IF_TYPE_SOFTWARE_LOOPBACK: u32 = 24;
    const GAA_SKIP: u32 = 2 | 4 | 8;

    let mut size: u32 = 15600;
    let mut buf = vec![0u8; size as usize];
    let mut ok = false;
    for _ in 0..6 {
        let rc = unsafe {
            GetAdaptersAddresses(
                0,
                GAA_SKIP,
                std::ptr::null(),
                buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH,
                &mut size,
            )
        };
        if rc == ERROR_SUCCESS {
            ok = true;
            break;
        }
        if rc != ERROR_BUFFER_OVERFLOW {
            return Vec::new();
        }
        buf = vec![0u8; (size as usize).max(1)];
    }
    if !ok {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut adapter = buf.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
    while !adapter.is_null() {
        let a = unsafe { &*adapter };
        let friendly = utf16_to_string(a.FriendlyName);
        let name = if friendly.is_empty() {
            ansi_to_string(a.AdapterName as *const u8)
        } else {
            friendly
        };
        let internal = a.IfType == IF_TYPE_SOFTWARE_LOOPBACK;
        let mut unicast = a.FirstUnicastAddress as *const IP_ADAPTER_UNICAST_ADDRESS_LH;
        while !unicast.is_null() {
            let u = unsafe { &*unicast };
            let raw = u.Address.lpSockaddr as *const u8;
            if !raw.is_null() {
                // sockaddr_in 地址在第 4 字节、sockaddr_in6 在第 8 字节（winsock2.h 布局）
                let family = unsafe { *(raw as *const u16) };
                if family == AF_INET || family == AF_INET6 {
                    let address = if family == AF_INET {
                        let mut b = [0u8; 4];
                        b.copy_from_slice(unsafe { std::slice::from_raw_parts(raw.add(4), 4) });
                        Ipv4Addr::from(b).to_string()
                    } else {
                        let mut b = [0u8; 16];
                        b.copy_from_slice(unsafe { std::slice::from_raw_parts(raw.add(8), 16) });
                        Ipv6Addr::from(b).to_string()
                    };
                    out.push(IfaceAddr {
                        name: name.clone(),
                        address,
                        v6: family == AF_INET6,
                        internal,
                    });
                }
            }
            unicast = u.Next;
        }
        adapter = a.Next;
    }
    out
}

#[cfg(not(windows))]
fn network_interfaces() -> Vec<IfaceAddr> {
    let Ok(out) = std::process::Command::new("ip")
        .args(["-o", "addr", "show"])
        .stdin(std::process::Stdio::null())
        .output()
    else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let mut list = Vec::new();
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let _ = parts.next();
        let Some(name) = parts.next() else { continue };
        let family = parts.next().unwrap_or("");
        let Some(addr) = parts.next() else { continue };
        if !matches!(family, "inet" | "inet6") {
            continue;
        }
        let address = addr.split('/').next().unwrap_or("").to_string();
        if address.is_empty() {
            continue;
        }
        let v6 = family == "inet6";
        let internal = name == "lo" || address == "::1" || address.starts_with("127.");
        list.push(IfaceAddr { name: name.to_string(), address, v6, internal });
    }
    list
}

fn xml_re(tag: &str) -> regex::Regex {
    regex::Regex::new(&format!(
        r"(?i)<(?:(?:[\w-]+):)?{tag}(?:\s[^>]*)?>([\s\S]*?)</(?:(?:[\w-]+):)?{tag}>"
    ))
    .unwrap()
}

fn decode_xml(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(pos) = rest.find('&') {
        out.push_str(&rest[..pos]);
        let tail = &rest[pos + 1..];
        let (repl, consumed) = if let Some(t) = tail.strip_prefix("amp;") {
            ("&", t)
        } else if let Some(t) = tail.strip_prefix("lt;") {
            ("<", t)
        } else if let Some(t) = tail.strip_prefix("gt;") {
            (">", t)
        } else if let Some(t) = tail.strip_prefix("quot;") {
            ("\"", t)
        } else if let Some(t) = tail.strip_prefix("apos;") {
            ("'", t)
        } else {
            out.push('&');
            rest = tail;
            continue;
        };
        out.push_str(repl);
        rest = consumed;
    }
    out.push_str(rest);
    out
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn xml_has_doctype(xml: &str) -> bool {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = RE.get_or_init(|| regex::Regex::new(r"(?i)(?:<!DOCTYPE|<!ENTITY)").unwrap());
    re.is_match(xml)
}

fn xml_value(xml: &str, tag: &str) -> Result<String, String> {
    if xml_has_doctype(xml) {
        return Err("不支持含外部实体的路由器响应".into());
    }
    let re = xml_re(tag);
    let caps = re.captures(xml);
    Ok(caps
        .as_ref()
        .and_then(|c| c.get(1))
        .map(|m| decode_xml(m.as_str().trim()))
        .unwrap_or_default())
}

fn local_router_url(input: &str, expected_host: Option<&str>) -> Result<reqwest::Url, String> {
    let Ok(url) = reqwest::Url::parse(input) else {
        return Err(ROUTER_ADDR_INVALID.into());
    };
    let host = url.host_str().unwrap_or("");
    if url.scheme() != "http"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || ipv4_scope(host) != "private"
        || expected_host.map(|expected| host != expected).unwrap_or(false)
    {
        return Err(ROUTER_ADDR_INVALID.into());
    }
    Ok(url)
}

// 对应原版 core/directUpnp.ts localRequest / soapEnvelope / gatewayAction / parseGatewayServices
struct Reply {
    body: String,
    status: u16,
    local_address: String,
}

fn local_request(input: &str, body: Option<&str>, action: Option<&str>) -> Result<Reply, String> {
    let url = local_router_url(input, None)?;
    let local_address = url
        .host_str()
        .and_then(|h| h.parse::<IpAddr>().ok())
        .map(|ip| local_source_address(ip, url.port_or_known_default().unwrap_or(80)))
        .unwrap_or_default();
    let request = match body {
        Some(text) => {
            let mut req = upnp_client()
                .post(url.as_str())
                .header("Content-Type", "text/xml; charset=\"utf-8\"")
                .header("Content-Length", text.len().to_string())
                .body(text.to_string());
            if let Some(act) = action {
                req = req.header("SOAPAction", format!("\"{act}\""));
            }
            req
        }
        None => upnp_client().get(url.as_str()),
    };
    let response = request.send().map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    let bytes = response.bytes().map_err(|e| e.to_string())?;
    if bytes.len() > UPNP_MAX_XML {
        return Err("路由器响应过大".into());
    }
    Ok(Reply {
        body: String::from_utf8_lossy(&bytes).to_string(),
        status,
        local_address,
    })
}

fn service_pattern(service_type: &str) -> bool {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(UPNP_SCOPE_RE).unwrap()).is_match(service_type)
}

fn soap_envelope(service_type: &str, action: &str, values: &[(&str, String)]) -> Result<String, String> {
    if !service_pattern(service_type) || action.is_empty() || !action.chars().all(|c| c.is_ascii_alphabetic()) {
        return Err("无效 UPnP 请求".into());
    }
    let mut body = String::new();
    for (key, value) in values {
        body.push_str(&format!("<{tag}>{text}</{tag}>", tag = key, text = escape_xml(value)));
    }
    Ok(format!(
        r#"<?xml version="1.0"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><u:{action} xmlns:u="{service_type}">{body}</u:{action}></s:Body></s:Envelope>"#
    ))
}

fn gateway_action(gateway: &Gateway, action: &str, values: &[(&str, String)]) -> Result<String, UpnpErr> {
    let envelope = soap_envelope(&gateway.service_type, action, values)?;
    let soap_action = format!("{}#{}", gateway.service_type, action);
    let reply = local_request(&gateway.control_url, Some(&envelope), Some(&soap_action))?;
    let error_code = xml_value(&reply.body, "errorCode")?;
    let error_description = xml_value(&reply.body, "errorDescription")?;
    if !error_code.is_empty() || reply.status != 200 {
        let code = if error_code.is_empty() {
            reply.status.to_string()
        } else {
            error_code
        };
        let desc = if error_description.is_empty() {
            "路由器未接受请求"
        } else {
            error_description.as_str()
        };
        return Err(UpnpErr {
            code: code.clone(),
            message: format!("UPnP {action} 失败（{code}）：{desc}"),
        });
    }
    Ok(reply.body)
}

fn parse_gateway_services(xml: &str, location: &str) -> Result<Vec<(String, String)>, String> {
    let base = local_router_url(location, None)?;
    if xml.len() > UPNP_MAX_XML || xml_has_doctype(xml) {
        return Err("路由器描述无效".into());
    }
    let base_host = base.host_str().unwrap_or("").to_string();
    let url_base = xml_value(xml, "URLBase")?;
    let root = if url_base.is_empty() {
        base.clone()
    } else {
        local_router_url(&url_base, Some(&base_host))?
    };
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(r"(?i)<(?:[\w-]+:)?service\b[^>]*>([\s\S]*?)</(?:[\w-]+:)?service>").unwrap()
    });
    let mut out = Vec::new();
    for cap in re.captures_iter(xml) {
        let block = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        let service_type = xml_value(block, "serviceType")?;
        if !service_pattern(&service_type) {
            continue;
        }
        let control_path = xml_value(block, "controlURL")?;
        let Some(joined) = root.join(&control_path).ok() else {
            continue;
        };
        let control_url = local_router_url(joined.as_str(), Some(&base_host))?;
        out.push((control_url.to_string(), service_type));
    }
    Ok(out)
}

// 对应原版 core/directUpnp.ts discoverLocations / discoverGateways
fn discover_locations() -> Vec<String> {
    let targets: Vec<Ipv4Addr> = network_interfaces()
        .iter()
        .filter(|item| !item.internal && !item.v6 && ipv4_scope(&item.address) == "private")
        .filter_map(|item| item.address.parse::<Ipv4Addr>().ok())
        .take(SSDP_IFACE_LIMIT)
        .collect();
    if targets.is_empty() {
        return Vec::new();
    }
    let found: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let mut threads = Vec::new();
    for ip in targets {
        let found = found.clone();
        let handle = std::thread::spawn(move || {
            #[cfg(windows)]
            let octets = ip.octets();
            let Ok(sock) = UdpSocket::bind((ip, 0)) else {
                return;
            };
            let _ = sock.set_multicast_ttl_v4(2);
            // 对应原版 socket.setMulticastInterface：多网卡机器上把 M-SEARCH 钉在这块网卡出去
            #[cfg(windows)]
            {
                use std::os::windows::io::AsRawSocket;
                use windows_sys::Win32::Networking::WinSock::{setsockopt, IP_MULTICAST_IF, IPPROTO_IP};
                unsafe {
                    setsockopt(
                        sock.as_raw_socket() as usize,
                        IPPROTO_IP,
                        IP_MULTICAST_IF,
                        octets.as_ptr() as windows_sys::core::PCSTR,
                        octets.len() as i32,
                    )
                };
            }
            let _ = sock.set_read_timeout(Some(Duration::from_millis(SSDP_TIMEOUT_MS)));
            let dest: SocketAddr = SSDP_ADDR.parse().expect("SSDP 组播地址解析失败");
            for target in [SSDP_ST1, SSDP_ST2] {
                let msg = format!(
                    "M-SEARCH * HTTP/1.1\r\nHOST: {}\r\nMAN: \"ssdp:discover\"\r\nMX: 1\r\nST: {}\r\n\r\n",
                    SSDP_ADDR, target
                );
                let _ = sock.send_to(msg.as_bytes(), dest);
            }
            static RE: OnceLock<regex::Regex> = OnceLock::new();
            let re = RE.get_or_init(|| regex::Regex::new(r"(?im)^location:\s*(.+)$").unwrap());
            let deadline = Instant::now() + Duration::from_millis(SSDP_TIMEOUT_MS);
            let mut buf = vec![0u8; SSDP_MAX_PACKET + 1];
            loop {
                if found.lock().unwrap().len() >= SSDP_MAX_LOCATIONS || Instant::now() >= deadline {
                    break;
                }
                let Ok((size, sender)) = sock.recv_from(&mut buf) else {
                    break;
                };
                if size > SSDP_MAX_PACKET {
                    continue;
                }
                let text = String::from_utf8_lossy(&buf[..size]).to_string();
                let caps = re.captures(&text);
                let Some(raw) = caps.as_ref().and_then(|c| c.get(1)) else {
                    continue;
                };
                let location = raw.as_str().trim().to_string();
                if location.is_empty() {
                    continue;
                }
                let sender_ip = sender.ip().to_string();
                if let Ok(url) = local_router_url(&location, Some(&sender_ip)) {
                    let mut list = found.lock().unwrap();
                    if list.len() < SSDP_MAX_LOCATIONS && !list.iter().any(|item| item == url.as_str()) {
                        list.push(url.to_string());
                    }
                }
            }
        });
        threads.push(handle);
    }
    for thread in threads {
        let _ = thread.join();
    }
    // 原版用 Set 收 Location，返回插入顺序；这里同样按到达顺序，不额外排序
    let list = found.lock().unwrap().clone();
    list
}

fn discover_gateways() -> Vec<Gateway> {
    let locations = discover_locations();
    if locations.is_empty() {
        return Vec::new();
    }
    let mut threads = Vec::new();
    for location in locations {
        threads.push(std::thread::spawn(move || {
            let Ok(reply) = local_request(&location, None, None) else {
                return Vec::new();
            };
            if reply.status != 200 {
                return Vec::new();
            }
            let Ok(services) = parse_gateway_services(&reply.body, &location) else {
                return Vec::new();
            };
            let mut found: Vec<Gateway> = Vec::new();
            for (control_url, service_type) in services {
                let mut gateway = Gateway {
                    control_url,
                    service_type,
                    local_address: reply.local_address.clone(),
                    external_address: String::new(),
                };
                if let Ok(body) = gateway_action(&gateway, "GetExternalIPAddress", &[]) {
                    gateway.external_address = xml_value(&body, "NewExternalIPAddress").unwrap_or_default();
                    found.push(gateway);
                }
            }
            found
        }));
    }
    let mut all: Vec<Gateway> = Vec::new();
    for thread in threads {
        match thread.join() {
            Ok(part) => all.extend(part),
            Err(_) => {}
        }
    }
    let mut seen: HashSet<String> = HashSet::new();
    all.retain(|gateway| seen.insert(gateway.control_url.clone()));
    all
}

// 对应原版 core/directConnect.ts inspectDirectNetwork / directState / directOverview / latestLanPort
pub fn inspect_direct_network() -> Value {
    let _guard = SCAN.lock().unwrap();
    let mut addresses: Vec<Value> = Vec::new();
    for item in network_interfaces() {
        if item.internal {
            continue;
        }
        let kind = if is_global_ipv6(&item.address) {
            Some("ipv6")
        } else {
            match ipv4_scope(&item.address) {
                "public" => Some("ipv4"),
                "private" => Some("lan"),
                _ => None,
            }
        };
        let Some(kind) = kind else { continue };
        if addresses
            .iter()
            .any(|existing| existing["address"].as_str() == Some(item.address.as_str()))
        {
            continue;
        }
        addresses.push(json!({ "name": item.name, "address": item.address, "kind": kind }));
    }
    let gateways = discover_gateways();
    let mut messages = vec!["公网可达性需要好友从另一网络验证；本机检测无法确认运营商及防火墙是否放行。".to_string()];
    if !addresses.iter().any(|item| item["kind"].as_str() == Some("ipv6")) {
        messages.push("未发现公网 IPv6 地址。".to_string());
    }
    if gateways.is_empty() {
        messages.push("未发现可用 UPnP 网关：路由器可能未开启 UPnP，或当前网络不支持。可使用公网 IPv6 / 手动 IPv4 端口映射。".to_string());
    }
    let gateway_values: Vec<Value> = gateways
        .iter()
        .map(|gateway| {
            let diagnosis = match ipv4_scope(&gateway.external_address) {
                "cgnat" => "网关 WAN 地址属于 CGNAT（100.64.0.0/10），UPnP 无法穿过运营商 NAT。",
                "private" => "网关 WAN 仍是私网地址，存在上级 NAT；仅映射本层路由器不能保证公网可达。",
                "public" => "发现公网 IPv4；开启房间时可尝试 UPnP 映射。",
                _ => "网关未提供可用的公网 IPv4。",
            };
            json!({
                "address": url_host(&gateway.control_url),
                "externalAddress": gateway.external_address,
                "diagnosis": diagnosis
            })
        })
        .collect();
    let network = json!({ "addresses": addresses, "messages": messages, "gateways": gateway_values });
    *GATEWAYS.lock().unwrap() = gateways;
    *NETWORK.lock().unwrap() = Some(network.clone());
    network
}

fn direct_state() -> Value {
    let session = SESSION.lock().unwrap().clone();
    match session {
        Some(session) => {
            let mut state = session.state.lock().unwrap().clone();
            state["connections"] = json!(session.connections.load(Ordering::SeqCst));
            state
        }
        None => json!({
            "active": false,
            "endpoints": [],
            "connections": 0,
            "messages": LAST_MESSAGES.lock().unwrap().clone()
        }),
    }
}

fn read_tail(path: &Path, max: usize) -> Option<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).ok()?;
    let size = file.metadata().ok()?.len() as usize;
    let length = size.min(max);
    let mut buffer = vec![0u8; length];
    file.seek(SeekFrom::Start((size - length) as u64)).ok()?;
    file.read_exact(&mut buffer).ok()?;
    Some(buffer)
}

fn latest_lan_port() -> Option<u16> {
    let launch = crate::launch::last_launch()?;
    let dir = launch["effectiveGameDir"].as_str().unwrap_or("");
    if dir.is_empty() {
        return None;
    }
    let ended = launch.get("endedAt").cloned().unwrap_or(Value::Null);
    if !ended.is_null() && ended.as_str().map(|s| !s.is_empty()).unwrap_or(true) {
        return None;
    }
    let log = Path::new(dir).join("logs").join("latest.log");
    let tail = read_tail(&log, LAN_LOG_TAIL)?;
    detect_lan_port(&String::from_utf8_lossy(&tail))
}

fn direct_overview(app: &AppHandle) -> Value {
    let network = inspect_direct_network();
    let mut out = Map::new();
    out.insert("network".into(), network);
    out.insert("instances".into(), crate::versions::list_installed(&crate::settings::get(app), true));
    out.insert("state".into(), direct_state());
    if let Some(port) = latest_lan_port() {
        out.insert("detectedPort".into(), json!(port));
    }
    Value::Object(out)
}

fn stop_direct_host() -> Value {
    let op = OPERATION.lock().unwrap().clone();
    if let Some(op) = op {
        op.abort.store(true, Ordering::SeqCst);
        let started = Instant::now();
        while !op.finished.load(Ordering::SeqCst) && started.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    let previous = SESSION.lock().unwrap().take();
    if let Some(session) = previous {
        session.stop.store(true, Ordering::SeqCst);
        close_room(&session.forwarder, &session.mappings);
    }
    direct_state()
}

// 对应原版 core/directProtocol.ts encodeInvitation / parseInvitation
fn base64url_encode(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn base64url_decode(text: &str) -> Option<Vec<u8>> {
    let normalized: String = text
        .chars()
        .filter(|c| *c != '=')
        .map(|c| match c { '+' => '-', '/' => '_', other => other })
        .collect();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(normalized).ok()
}

fn encode_invitation(invitation: &Value) -> String {
    let text = serde_json::to_string(invitation).unwrap_or_default();
    format!("{}{}", INVITE_PREFIX, base64url_encode(text.as_bytes()))
}

fn date_parse(value: &str) -> Option<i64> {
    let text = value.trim();
    let mut candidates: Vec<String> = Vec::new();
    if !text.is_empty() {
        candidates.push(text.to_string());
        if let Some(stripped) = text.strip_suffix('Z') {
            candidates.push(format!("{stripped}+00:00"));
        }
    }
    for candidate in &candidates {
        if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(candidate) {
            return Some(parsed.timestamp_millis());
        }
    }
    None
}

fn parse_invitation(input: &str, now: i64) -> Result<Value, String> {
    if input.len() > INVITE_MAX_LEN {
        return Err("邀请内容过大或格式无效".into());
    }
    let raw = input.trim();
    let payload = match raw.strip_prefix(INVITE_PREFIX) {
        Some(body) => {
            let bytes = base64url_decode(body).ok_or("无法识别邀请，请粘贴完整的 KAMUCL-DIRECT-1 邀请信息".to_string())?;
            String::from_utf8_lossy(&bytes).to_string()
        }
        None => raw.to_string(),
    };
    let Ok(value) = serde_json::from_str::<Value>(&payload) else {
        return Err("无法识别邀请，请粘贴完整的 KAMUCL-DIRECT-1 邀请信息".into());
    };
    let name = value["name"].as_str();
    let mc_version = value["minecraftVersion"].as_str();
    let declared_endpoints = value["endpoints"].as_array();
    let valid = value["format"].as_str() == Some("KAMUCL-DIRECT")
        && value["version"].as_i64() == Some(1)
        && name.map(|item| !item.trim().is_empty() && item.chars().count() <= 128).unwrap_or(false)
        && mc_version.map(|item| item.chars().count() <= 128).unwrap_or(false)
        && declared_endpoints
            .as_ref()
            .map(|items| !items.is_empty() && items.len() <= INVITE_MAX_ENDPOINTS)
            .unwrap_or(false);
    if !valid {
        return Err("邀请格式或版本无效".into());
    }
    let Some(expires) = date_parse(value["expiresAt"].as_str().unwrap_or("")) else {
        return Err("邀请已过期或有效期无效，请让房主重新生成".into());
    };
    if expires < now || expires > now + 25 * 3600000 {
        return Err("邀请已过期或有效期无效，请让房主重新生成".into());
    }
    let loader = value.get("loader").filter(|item| !item.is_null());
    if let Some(item) = loader {
        if !item.as_str().map(|s| matches!(s, "forge" | "fabric" | "quilt" | "neoforge")).unwrap_or(false) {
            return Err("邀请的加载器信息无效".into());
        }
    }
    let loader_version = value.get("loaderVersion").filter(|item| !item.is_null());
    if let Some(item) = loader_version {
        if !item.as_str().map(|s| s.chars().count() <= 128).unwrap_or(false) {
            return Err("邀请的加载器信息无效".into());
        }
    }
    let mut endpoints: Vec<Value> = Vec::new();
    for endpoint in declared_endpoints.cloned().unwrap_or_default() {
        let Some(host_raw) = endpoint.get("host").and_then(|item| item.as_str()) else {
            return Err("邀请地址无效".into());
        };
        let kind = if is_global_ipv6(host_raw) {
            Some("ipv6")
        } else {
            match ipv4_scope(host_raw) {
                "public" => Some("ipv4"),
                "private" => Some("lan"),
                _ => None,
            }
        };
        let Some(kind) = kind else {
            return Err("邀请含有非公网或局域网单播 IP 地址".into());
        };
        let host = match host_raw.parse::<Ipv6Addr>() {
            Ok(v6) => v6.to_string(),
            Err(_) => host_raw.to_string(),
        };
        let port = validate_port(&endpoint["port"])?;
        let address = endpoint_address(&host, port);
        if endpoints.iter().any(|item| endpoint_text(item) == address) {
            continue;
        }
        endpoints.push(json!({ "host": host, "port": port, "kind": kind }));
    }
    let mut out = Map::new();
    out.insert("format".into(), json!("KAMUCL-DIRECT"));
    out.insert("version".into(), json!(1));
    out.insert("name".into(), value["name"].clone());
    out.insert("minecraftVersion".into(), value["minecraftVersion"].clone());
    if let Some(item) = loader {
        out.insert("loader".into(), item.clone());
    }
    if let Some(item) = loader_version {
        out.insert("loaderVersion".into(), item.clone());
    }
    out.insert("endpoints".into(), Value::Array(endpoints));
    out.insert("expiresAt".into(), value["expiresAt"].clone());
    Ok(Value::Object(out))
}

// 对应原版 core/directProtocol.ts createLocalForwarder
type LiveStreams = Arc<Mutex<HashMap<u64, (TcpStream, TcpStream)>>>;

static CONN_SEQ: AtomicU64 = AtomicU64::new(1);

pub struct Forwarder {
    port: u16,
    connections: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    streams: LiveStreams,
}

impl Forwarder {
    fn close(&self) {
        self.stop.store(true, Ordering::SeqCst);
        let streams = self.streams.lock().unwrap();
        for (client, upstream) in streams.values() {
            let _ = client.shutdown(std::net::Shutdown::Both);
            let _ = upstream.shutdown(std::net::Shutdown::Both);
        }
    }
}

fn kill_connection(id: u64, streams: &LiveStreams) {
    if let Some((client, upstream)) = streams.lock().unwrap().remove(&id) {
        let _ = client.shutdown(std::net::Shutdown::Both);
        let _ = upstream.shutdown(std::net::Shutdown::Both);
    }
}

fn forward_connection(
    client: TcpStream,
    target: SocketAddr,
    id: u64,
    connections: Arc<AtomicUsize>,
    streams: LiveStreams,
) {
    let _ = client.set_read_timeout(Some(Duration::from_millis(FORWARD_IDLE_MS)));
    let upstream = TcpStream::connect_timeout(&target, Duration::from_millis(FORWARD_UPSTREAM_MS));
    let Ok(upstream) = upstream else {
        kill_connection(id, &streams);
        connections.fetch_sub(1, Ordering::SeqCst);
        return;
    };
    let _ = upstream.set_read_timeout(Some(Duration::from_millis(FORWARD_IDLE_MS)));
    let handles = (client.try_clone(), upstream.try_clone(), client.try_clone(), upstream.try_clone());
    let (mut client_writer, mut upstream_reader, client_shutdown, upstream_shutdown) = match handles {
        (Ok(a), Ok(b), Ok(c), Ok(d)) => (a, b, c, d),
        _ => {
            kill_connection(id, &streams);
            connections.fetch_sub(1, Ordering::SeqCst);
            return;
        }
    };
    streams.lock().unwrap().insert(id, (client_shutdown, upstream_shutdown));
    let mut client_reader = client;
    let mut upstream_writer = upstream;
    let reverse_streams = streams.clone();
    let reverse = std::thread::spawn(move || {
        let _ = std::io::copy(&mut upstream_reader, &mut client_writer);
        kill_connection(id, &reverse_streams);
    });
    let _ = std::io::copy(&mut client_reader, &mut upstream_writer);
    kill_connection(id, &streams);
    let _ = reverse.join();
    connections.fetch_sub(1, Ordering::SeqCst);
}

fn create_local_forwarder(target_port: u16, listen_host: &str) -> std::io::Result<Forwarder> {
    let bind_host = if listen_host.contains(':') && !listen_host.starts_with('[') {
        format!("[{listen_host}]")
    } else {
        listen_host.to_string()
    };
    let listener = TcpListener::bind((bind_host.as_str(), 0))?;
    let port = listener.local_addr()?.port();
    listener.set_nonblocking(true)?;
    let connections = Arc::new(AtomicUsize::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let streams: LiveStreams = Arc::new(Mutex::new(HashMap::new()));
    let target = resolve_first("127.0.0.1", target_port);
    let (loop_connections, loop_stop, loop_streams) = (connections.clone(), stop.clone(), streams.clone());
    std::thread::spawn(move || {
        let Some(target) = target else {
            return;
        };
        while !loop_stop.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((client, _)) => {
                    if loop_connections.load(Ordering::SeqCst) >= FORWARD_MAX_CONNECTIONS {
                        drop(client);
                        continue;
                    }
                    let id = CONN_SEQ.fetch_add(1, Ordering::SeqCst);
                    loop_connections.fetch_add(1, Ordering::SeqCst);
                    let (count, pool) = (loop_connections.clone(), loop_streams.clone());
                    std::thread::spawn(move || forward_connection(client, target, id, count, pool));
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(_) => break,
            }
        }
    });
    Ok(Forwarder { port, connections, stop, streams })
}

// 对应原版 core/directUpnp.ts createMapping
#[derive(Clone)]
struct Mapping {
    gateway: Gateway,
    port: u16,
    description: String,
    active: Arc<AtomicBool>,
    gate: Arc<Mutex<()>>,
}

fn mapping_key(port: u16) -> Vec<(&'static str, String)> {
    vec![
        ("NewRemoteHost", String::new()),
        ("NewExternalPort", port.to_string()),
        ("NewProtocol", "TCP".to_string()),
    ]
}

fn mapping_values(port: u16, local_address: &str, description: &str) -> Vec<(&'static str, String)> {
    let mut values = mapping_key(port);
    values.push(("NewInternalPort", port.to_string()));
    values.push(("NewInternalClient", local_address.to_string()));
    values.push(("NewEnabled", "1".to_string()));
    values.push(("NewPortMappingDescription", description.to_string()));
    values.push(("NewLeaseDuration", "300".to_string()));
    values
}

fn create_mapping(gateway: &Gateway, port: u16, description: &str) -> Result<Mapping, String> {
    let key = mapping_key(port);
    match gateway_action(gateway, "GetSpecificPortMappingEntry", &key) {
        Ok(_) => return Err("该端口已存在路由器映射，请重新开启房间以使用其他端口".into()),
        Err(err) => {
            if err.code != "714" {
                return Err(err.message);
            }
        }
    }
    let values = mapping_values(port, &gateway.local_address, description);
    gateway_action(gateway, "AddPortMapping", &values).map_err(|err| err.message)?;
    Ok(Mapping {
        gateway: gateway.clone(),
        port,
        description: description.to_string(),
        active: Arc::new(AtomicBool::new(true)),
        gate: Arc::new(Mutex::new(())),
    })
}

fn mapping_owned(mapping: &Mapping) -> Result<bool, String> {
    let key = mapping_key(mapping.port);
    let data = gateway_action(&mapping.gateway, "GetSpecificPortMappingEntry", &key).map_err(|err| err.message)?;
    Ok(xml_value(&data, "NewInternalClient")? == mapping.gateway.local_address
        && xml_value(&data, "NewInternalPort")? == mapping.port.to_string()
        && xml_value(&data, "NewPortMappingDescription")? == mapping.description)
}

fn mapping_renew(mapping: &Mapping) -> Result<(), String> {
    let _guard = mapping.gate.lock().unwrap();
    if !mapping.active.load(Ordering::SeqCst) {
        return Ok(());
    }
    if !mapping_owned(mapping)? {
        return Err("UPnP 映射归属已改变".into());
    }
    if !mapping.active.load(Ordering::SeqCst) {
        return Ok(());
    }
    let values = mapping_values(mapping.port, &mapping.gateway.local_address, &mapping.description);
    gateway_action(&mapping.gateway, "AddPortMapping", &values).map_err(|err| err.message)?;
    Ok(())
}

fn mapping_remove(mapping: &Mapping) -> Result<(), String> {
    mapping.active.store(false, Ordering::SeqCst);
    let _guard = mapping.gate.lock().unwrap();
    if mapping_owned(mapping)? {
        let key = mapping_key(mapping.port);
        gateway_action(&mapping.gateway, "DeletePortMapping", &key).map_err(|err| err.message)?;
    }
    Ok(())
}

fn throw_if_aborted(abort: &AtomicBool) -> Result<(), String> {
    if abort.load(Ordering::SeqCst) {
        Err("房间创建已取消".into())
    } else {
        Ok(())
    }
}

fn find_instance(app: &AppHandle, version_id: &str, folder: &str) -> Option<Value> {
    let target = crate::installer::path_identity(folder);
    crate::versions::list_installed(&crate::settings::get(app), true)
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .find(|item| {
            item["id"].as_str() == Some(version_id)
                && crate::installer::path_identity(item["folder"].as_str().unwrap_or("")) == target
        })
}

fn host_build(
    request: &Value,
    abort: &AtomicBool,
    forwarder: &Forwarder,
    ipv6_available: bool,
    mappings: &Mutex<Vec<Mapping>>,
) -> Result<(Vec<Value>, Vec<String>), String> {
    let network = NETWORK.lock().unwrap().clone().unwrap_or(json!({ "addresses": [], "gateways": [], "messages": [] }));
    let mut endpoints: Vec<Value> = network["addresses"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter(|item| item["kind"].as_str() != Some("ipv6") || ipv6_available)
        .take(HOST_ENDPOINT_LIMIT)
        .map(|item| json!({ "host": item["address"], "port": forwarder.port, "kind": item["kind"] }))
        .collect();
    let mut messages: Vec<String> = network["messages"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|item| item.as_str().map(str::to_string))
        .collect();
    messages.extend(
        network["gateways"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter(|item| {
                let diagnosis = item["diagnosis"].as_str().unwrap_or("");
                diagnosis.contains("CGNAT") || diagnosis.contains("上级 NAT")
            })
            .filter_map(|item| item["diagnosis"].as_str().map(str::to_string)),
    );
    let owner = format!("KAMUCL-{}", crate::accounts::uuid_v4().chars().take(12).collect::<String>());
    if request["useUpnp"] == true {
        let candidates: Vec<Gateway> = GATEWAYS
            .lock()
            .unwrap()
            .iter()
            .filter(|gateway| ipv4_scope(&gateway.external_address) == "public")
            .take(HOST_UPNP_GATEWAY_LIMIT)
            .cloned()
            .collect();
        for gateway in candidates {
            throw_if_aborted(abort)?;
            match create_mapping(&gateway, forwarder.port, &owner) {
                Ok(mapping) => {
                    endpoints.push(json!({ "host": gateway.external_address, "port": forwarder.port, "kind": "ipv4" }));
                    messages.push("UPnP 临时映射已建立（5 分钟租约，房间开启期间自动续租）。".to_string());
                    mappings.lock().unwrap().push(mapping);
                }
                Err(err) => messages.push(err),
            }
        }
    }
    let public_address = request["publicAddress"].as_str().unwrap_or("").trim().to_string();
    if !public_address.is_empty() {
        if ipv4_scope(&public_address) != "public" {
            return Err("手动公网 IPv4 地址格式无效".into());
        }
        endpoints.push(json!({ "host": public_address, "port": forwarder.port, "kind": "ipv4" }));
        messages.push(format!(
            "手动 IPv4：请将路由器 TCP {} 映射到本机同端口，未验证此规则是否存在。",
            forwarder.port
        ));
    }
    throw_if_aborted(abort)?;
    Ok((endpoints, messages))
}

fn close_room(forwarder: &Forwarder, mappings: &Mutex<Vec<Mapping>>) {
    forwarder.close();
    let list = std::mem::take(&mut *mappings.lock().unwrap());
    for mapping in list.iter() {
        let _ = mapping_remove(mapping);
    }
}

// 对应原版 core/directConnect.ts startDirectHost / resolveDirectInvitation / prepareDirectJoin
fn start_direct_host(app: &AppHandle, request: &Value) -> Result<Value, String> {
    let abort = Arc::new(AtomicBool::new(false));
    let finished = Arc::new(AtomicBool::new(false));
    {
        if SESSION.lock().unwrap().is_some() {
            return Err("已有房间或正在创建房间，请先停止".into());
        }
        let mut op = OPERATION.lock().unwrap();
        if op.is_some() {
            return Err("已有房间或正在创建房间，请先停止".into());
        }
        *op = Some(Arc::new(Op { abort: abort.clone(), finished: finished.clone() }));
    }
    let result = host_inner(app, request, &abort);
    *OPERATION.lock().unwrap() = None;
    finished.store(true, Ordering::SeqCst);
    result
}

fn host_inner(app: &AppHandle, request: &Value, abort: &AtomicBool) -> Result<Value, String> {
    let version_id = request["versionId"].as_str().unwrap_or("");
    let folder = request["folder"].as_str().unwrap_or("");
    let version = match find_instance(app, version_id, folder) {
        Some(item) if item["incomplete"] != true => item,
        _ => return Err("所选实例不存在或不完整".into()),
    };
    let detected = latest_lan_port();
    let declared = match &request["port"] {
        Value::Null => detected.map(|port| json!(port)).unwrap_or(json!(0)),
        other => other.clone(),
    };
    let port = validate_port(&declared)?;
    if !probe_tcp("127.0.0.1", port, Some(abort)) {
        return Err("本机世界端口未开放。请在游戏内选择“对局域网开放”，然后刷新端口或手动输入。".into());
    }
    throw_if_aborted(abort)?;
    inspect_direct_network();
    throw_if_aborted(abort)?;
    let mut ipv6_available = true;
    let forwarder = match create_local_forwarder(port, "::") {
        Ok(forwarder) => forwarder,
        Err(err)
            if matches!(
                err.kind(),
                std::io::ErrorKind::Unsupported | std::io::ErrorKind::AddrNotAvailable
            ) =>
        {
            ipv6_available = false;
            create_local_forwarder(port, "0.0.0.0").map_err(|io_err| io_err.to_string())?
        }
        Err(err) => return Err(err.to_string()),
    };
    let mappings: Arc<Mutex<Vec<Mapping>>> = Arc::new(Mutex::new(Vec::new()));
    let (endpoints, messages) = match host_build(request, abort, &forwarder, ipv6_available, &mappings) {
        Ok(built) => built,
        Err(err) => {
            close_room(&forwarder, &mappings);
            return Err(err);
        }
    };
    let mut unique: Vec<Value> = Vec::new();
    for endpoint in endpoints {
        let address = endpoint_text(&endpoint);
        if unique.iter().any(|item| endpoint_text(item) == address) {
            continue;
        }
        unique.push(endpoint);
    }
    unique.truncate(HOST_UNIQUE_LIMIT);
    let mut messages = messages;
    if !unique.iter().any(|item| item["kind"].as_str() != Some("lan")) {
        messages.push("当前仅可供同一局域网加入，尚无公网房主路径。CGNAT、手机热点、校园网可能限制入站连接；你仍可以加入可达好友的世界。".to_string());
    }
    messages.push("开启房间后需保持 WUHUICL 和游戏运行；退出或关闭局域网世界会断开直连。".to_string());
    let mut invitation = Map::new();
    invitation.insert("format".into(), json!("KAMUCL-DIRECT"));
    invitation.insert("version".into(), json!(1));
    invitation.insert("name".into(), version["id"].clone());
    invitation.insert("minecraftVersion".into(), version["mcVersion"].clone());
    if !version["loader"].is_null() {
        invitation.insert("loader".into(), version["loader"].clone());
    }
    if !version["loaderVersion"].is_null() {
        invitation.insert("loaderVersion".into(), version["loaderVersion"].clone());
    }
    invitation.insert("endpoints".into(), json!(unique.clone()));
    invitation.insert("expiresAt".into(), json!(iso_at(now_millis() + INVITE_TTL_MS)));
    let mut state = Map::new();
    state.insert("active".into(), json!(true));
    state.insert("connections".into(), json!(0));
    state.insert("endpoints".into(), json!(unique.clone()));
    if !unique.is_empty() {
        state.insert("invite".into(), json!(encode_invitation(&Value::Object(invitation.clone()))));
    }
    state.insert("localPort".into(), json!(port));
    state.insert("exposedPort".into(), json!(forwarder.port));
    state.insert("messages".into(), json!(messages));
    state.insert("startedAt".into(), json!(iso_now()));
    let state_cell = Arc::new(Mutex::new(Value::Object(state)));
    let forwarder = Arc::new(forwarder);
    let (connections, stop) = (forwarder.connections.clone(), forwarder.stop.clone());
    *SESSION.lock().unwrap() = Some(Arc::new(Session {
        state: state_cell.clone(),
        connections,
        stop: stop.clone(),
        mappings: mappings.clone(),
        forwarder,
    }));
    let watch_mappings = mappings.clone();
    std::thread::spawn(move || {
        let mut renewed_at = now_millis();
        loop {
            let mut waited: u64 = 0;
            while waited < HOST_WATCHDOG_MS {
                if stop.load(Ordering::SeqCst) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(100));
                waited += 100;
            }
            if stop.load(Ordering::SeqCst) {
                return;
            }
            if !probe_tcp("127.0.0.1", port, None) {
                *LAST_MESSAGES.lock().unwrap() = vec!["本机世界已关闭，房间和端口映射已停止。".to_string()];
                let _ = stop_direct_host();
                return;
            }
            if now_millis() - renewed_at >= HOST_RENEW_MS {
                renewed_at = now_millis();
                let renew_list = watch_mappings.lock().unwrap().clone();
                for mapping in renew_list.iter() {
                    if let Err(err) = mapping_renew(mapping) {
                        let mut state = state_cell.lock().unwrap();
                        let exists = state["messages"]
                            .as_array()
                            .cloned()
                            .unwrap_or_default()
                            .iter()
                            .any(|item| item.as_str() == Some(err.as_str()));
                        if !exists {
                            if let Some(list) = state["messages"].as_array_mut() {
                                list.push(json!(format!("UPnP 续租失败：{err}，IPv4 邀请可能不可达，请重新开启房间。")));
                            }
                        }
                    }
                }
            }
        }
    });
    Ok(direct_state())
}

fn kind_rank(kind: &str) -> i32 {
    match kind {
        "ipv6" => 0,
        "ipv4" => 1,
        "lan" => 2,
        _ => -1,
    }
}

fn probe_endpoint(endpoint: Value) -> (Value, bool) {
    let host = endpoint["host"].as_str().unwrap_or("").to_string();
    let port = endpoint["port"].as_u64().unwrap_or(0) as u16;
    let online = probe_tcp(&host, port, None);
    (endpoint, online)
}

fn resolve_direct_invitation(input: &str) -> Result<Value, String> {
    let invitation = parse_invitation(input, now_millis())?;
    let mut ordered = invitation["endpoints"].as_array().cloned().unwrap_or_default();
    ordered.sort_by_key(|item| kind_rank(item["kind"].as_str().unwrap_or("")));
    let mut handles = Vec::new();
    for endpoint in ordered {
        handles.push(std::thread::spawn(move || probe_endpoint(endpoint)));
    }
    let mut replies: Vec<(Value, bool)> = Vec::new();
    for handle in handles {
        match handle.join() {
            Ok(reply) => replies.push(reply),
            Err(_) => {}
        }
    }
    let mut out = Map::new();
    out.insert("invitation".into(), invitation);
    if let Some((endpoint, _)) = replies.iter().find(|(_, online)| *online) {
        out.insert("endpoint".into(), endpoint.clone());
    }
    out.insert(
        "failures".into(),
        json!(replies
            .iter()
            .filter(|(_, online)| !*online)
            .map(|(endpoint, _)| format!("{}：连接超时或被拒绝", endpoint_text(endpoint)))
            .collect::<Vec<String>>()),
    );
    Ok(Value::Object(out))
}

fn supports_quick_play_multiplayer(version: &str) -> bool {
    static RELEASE: OnceLock<regex::Regex> = OnceLock::new();
    static SNAPSHOT: OnceLock<regex::Regex> = OnceLock::new();
    let trimmed = version.trim();
    let release = RELEASE.get_or_init(|| regex::Regex::new(r"^(\d+)\.(\d+)(?:\.(\d+))?(?:$|[-+])").unwrap());
    if let Some(caps) = release.captures(trimmed) {
        let major: i64 = caps[1].parse().unwrap_or(0);
        let minor: i64 = caps[2].parse().unwrap_or(0);
        return major > 1 || (major == 1 && minor >= 20);
    }
    let snapshot = SNAPSHOT.get_or_init(|| regex::Regex::new(r"(?i)^(\d{2})w(\d{2})([a-z])$").unwrap());
    let Some(caps) = snapshot.captures(trimmed) else {
        return false;
    };
    let year: i64 = caps[1].parse().unwrap_or(0);
    let week: i64 = caps[2].parse().unwrap_or(0);
    let revision = caps[3].to_lowercase();
    year > 23 || (year == 23 && (week > 14 || (week == 14 && revision.as_str() >= "a")))
}

fn text_or_empty(value: Option<&Value>) -> String {
    match value {
        Some(item) if item.is_string() => item.as_str().unwrap_or("").to_string(),
        _ => String::new(),
    }
}

fn truthy_string(value: Option<&Value>) -> Option<String> {
    let item = value.filter(|item| !item.is_null())?;
    let text = item.as_str()?.to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn prepare_direct_join(app: &AppHandle, input: &str, version_id: &str, folder: &str) -> Result<Value, String> {
    let result = resolve_direct_invitation(input)?;
    let Some(endpoint) = result.get("endpoint").cloned() else {
        return Err("没有可达的好友地址。请检查房主是否在线、双方 IPv6、路由器映射及入站防火墙。".into());
    };
    let invite = result["invitation"].clone();
    let version = match find_instance(app, version_id, folder) {
        Some(item) if item["incomplete"] != true => item,
        _ => return Err("关联实例不存在或未完整安装，请先下载兼容版本".into()),
    };
    let mismatched = version["mcVersion"] != invite["minecraftVersion"]
        || text_or_empty(version.get("loader")) != text_or_empty(invite.get("loader"))
        || truthy_string(invite.get("loaderVersion"))
            .map(|item| version["loaderVersion"] != Value::String(item))
            .unwrap_or(false);
    if mismatched {
        return Err("所选实例与房主的 Minecraft / Loader 版本不匹配，请选择兼容实例；MOD 列表还需双方自行确认一致。".into());
    }
    crate::gamefolders::set_active_game_folder(app, folder)?;
    Ok(json!({
        "versionId": version_id,
        "folder": folder,
        "address": endpoint_text(&endpoint),
        "directJoin": supports_quick_play_multiplayer(version["mcVersion"].as_str().unwrap_or(""))
    }))
}

pub fn invoke(app: &AppHandle, channel: &str, args: &[Value]) -> Option<Result<Value, String>> {
    match channel {
        "direct:overview" => Some(Ok(direct_overview(app))),
        "direct:state" => Some(Ok(direct_state())),
        "direct:stop" => Some(Ok(stop_direct_host())),
        "direct:host" => {
            let request = args.first().cloned().unwrap_or(json!({}));
            Some(start_direct_host(app, &request))
        }
        "direct:resolve" => Some(resolve_direct_invitation(args.first().and_then(|item| item.as_str()).unwrap_or(""))),
        "direct:prepareJoin" => Some(prepare_direct_join(
            app,
            args.first().and_then(|item| item.as_str()).unwrap_or(""),
            args.get(1).and_then(|item| item.as_str()).unwrap_or(""),
            args.get(2).and_then(|item| item.as_str()).unwrap_or(""),
        )),
        _ => None,
    }
}
