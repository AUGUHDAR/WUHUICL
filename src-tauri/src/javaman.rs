//! Java 管理子系统（对应原版 core/java.ts、javaScanUtils.ts、javaSources.ts（探测部分）、
//! javaCompatibility.ts（校验部分）、javaArchitecture.ts、javaProbeCache.ts、
//! javaRuntimeHealth.ts（健康校验）、javaMetadata.ts、javaPreparation.ts）。
//!
//! 通道：java:list / java:refresh / java:addCustom / java:pickAdd / java:hide / java:cancelScan
//!
//! 探测方式：spawn `<java> -Xms8m -Xmx32m -XshowSettings:properties -version`
//! （CREATE_NO_WINDOW），解析 java.version / os.arch / java.vendor。
//! 注册表扫描用 windows-registry（HKLM\SOFTWARE\JavaSoft + Eclipse Adoptium 等），
//! 与原版 reg.exe query /s 输出解析等价。

use crate::installer::{self, ProgressGuard};
use crate::settings;
use crate::tasks::{self, TaskRecord};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tauri::AppHandle;

const JAVA_EXE: &str = "java.exe";
/// Version/module probes do not run game code (javaScanUtils.JAVA_PROBE_VM_ARGS)
const JAVA_PROBE_VM_ARGS: &[&str] = &["-Xms8m", "-Xmx32m"];
/// java:list 5 分钟缓存（java.ts SCAN_TTL）
const SCAN_TTL_MS: u64 = 5 * 60 * 1000;
/// 持久扫描缓存 7 天（java.ts PERSISTENT_SCAN_TTL）
const PERSISTENT_SCAN_TTL_MS: u64 = 7 * 24 * 60 * 60 * 1000;
/// 探测缓存 7 天（javaProbeCache）
const PROBE_TTL_MS: u64 = 7 * 24 * 60 * 60 * 1000;
const PROBE_TIMEOUT_MS: u64 = 10_000;

#[derive(Clone)]
struct JavaCandidate {
    executable: String,
    source_detail: String,
}

struct ScanCache {
    time: u64,
    list: Vec<Value>,
    complete: bool,
}

static SCAN_CACHE: Mutex<Option<ScanCache>> = Mutex::new(None);
static ACTIVE_SCAN_TASK: Mutex<Option<String>> = Mutex::new(None);

// ---------------- invoke 入口 ----------------

/// 返回 None 表示本模块不认识该通道（由路由继续分发）；Some(result) 即最终结果。
pub fn invoke(app: &AppHandle, channel: &str, args: &[Value]) -> Option<Result<Value, String>> {
    match channel {
        "java:list" => Some(Ok(json!(list_java_summary(app)))),
        "java:addCustom" => Some(
            arg_str(args, 0)
                .ok_or_else(|| "缺少参数".to_string())
                .and_then(|p| add_custom_java(app, &p))
                .map(|_| Value::Null),
        ),
        "java:pickAdd" => Some(pick_add_java(app)),
        "java:hide" => Some(
            arg_str(args, 0)
                .ok_or_else(|| "缺少参数".to_string())
                .and_then(|p| hide_java(app, &p))
                .map(|_| Value::Null),
        ),
        "java:refresh" => Some(java_refresh(app, args)),
        "java:cancelScan" => Some(Ok(json!(cancel_scan()))),
        _ => None,
    }
}

fn arg_str(args: &[Value], i: usize) -> Option<String> {
    args.get(i)
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .filter(|s| !s.is_empty())
}

// ---------------- 路径工具（javaScanUtils） ----------------

fn path_key(value: &str) -> String {
    let resolved = if Path::new(value).is_absolute() {
        PathBuf::from(value)
    } else {
        std::env::current_dir().unwrap_or_default().join(value)
    };
    let mut normalized = resolved.to_string_lossy().to_string();
    if let Some(stripped) = normalized.strip_prefix("\\\\?\\") {
        normalized = stripped.to_string();
    }
    // 斜杠方向归一（用户手输可能是正斜杠，探测路径是反斜杠）
    let normalized = normalized.replace('/', "\\").to_lowercase();
    normalized
        .trim_end_matches(['\\', '/'])
        .to_string()
}

fn real_executable(value: &str) -> Option<String> {
    let p = Path::new(value);
    if !p.is_file() {
        return None;
    }
    let real = std::fs::canonicalize(p).ok()?;
    let s = real.to_string_lossy().to_string();
    Some(s.strip_prefix("\\\\?\\").map(str::to_string).unwrap_or(s))
}

/// 一个配置/注册表值既可能是 JAVA_HOME，也可能已经指向 java.exe
fn executable_paths(value: &str) -> Vec<String> {
    let clean = value.trim().trim_matches('"').to_string();
    if clean.is_empty() {
        return Vec::new();
    }
    let base = Path::new(&clean)
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if base == JAVA_EXE {
        return vec![clean];
    }
    if base == "bin" {
        return vec![join_exe(&clean)];
    }
    vec![join_exe(&format!("{}\\bin", clean))]
}

fn join_exe(dir: &str) -> String {
    format!("{}\\{}", dir.trim_end_matches(['\\', '/']), JAVA_EXE)
}

fn add_candidate(candidates: &mut HashMap<String, JavaCandidate>, value: Option<&str>, source_detail: &str) {
    let Some(value) = value else { return };
    for executable in executable_paths(value) {
        let key = path_key(&executable);
        candidates.entry(key).or_insert_with(|| JavaCandidate {
            executable,
            source_detail: source_detail.to_string(),
        });
    }
}

fn expand_windows_environment(value: &str) -> String {
    let re = regex::Regex::new(r"%([^%]+)%").unwrap();
    re.replace_all(value, |caps: &regex::Captures| {
        let name = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        std::env::vars()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v)
            .unwrap_or_else(|| caps.get(0).map(|m| m.as_str().to_string()).unwrap_or_default())
    })
    .to_string()
}

// ---------------- 探测（java -version 输出解析） ----------------

/// javaScanUtils.normalizeJavaArchitecture
fn normalize_java_architecture(value: &str) -> String {
    let arch = value.trim().to_lowercase().replace([' ', '_', '-'], "");
    if regex::Regex::new(r"^(amd64|x8664|x64)$").unwrap().is_match(&arch) {
        return "x64".into();
    }
    if regex::Regex::new(r"^(x86|i[3-6]86|i86pc)$").unwrap().is_match(&arch) {
        return "x86".into();
    }
    if regex::Regex::new(r"^(aarch64|arm64)$").unwrap().is_match(&arch) {
        return "arm64".into();
    }
    if regex::Regex::new(r"^(arm|arm32)$").unwrap().is_match(&arch) {
        return "arm32".into();
    }
    if value.trim().is_empty() {
        "unknown".into()
    } else {
        value.trim().to_string()
    }
}

fn probe_property(output: &str, name: &str) -> Option<String> {
    let escaped = regex::escape(name);
    let re = regex::Regex::new(&format!(r"(?im)^\s*{}\s*=\s*(.+?)\s*$", escaped)).unwrap();
    re.captures(output).and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
}

/// javaScanUtils.parseJavaProbeOutput：major/version/is64Bit/architecture/vendor
fn parse_java_probe_output(output: &str) -> Option<Value> {
    let version = probe_property(output, "java.version")
        .or_else(|| {
            regex::Regex::new(r#"(?:openjdk|java) version\s+"([^"]+)""#)
                .unwrap()
                .captures(output)
                .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
        })?;
    let major = if version.starts_with("1.") {
        version.split('.').nth(1).and_then(|s| s.parse::<i64>().ok())?
    } else {
        regex::Regex::new(r"^(?:1\.)?(\d+)")
            .unwrap()
            .captures(&version)
            .and_then(|c| c.get(1))
            .and_then(|m| m.as_str().parse::<i64>().ok())?
    };
    if major <= 0 {
        return None;
    }
    let raw_arch = probe_property(output, "os.arch");
    let architecture = match raw_arch {
        Some(a) => normalize_java_architecture(&a),
        None => {
            if regex::Regex::new(r"64-Bit").unwrap().is_match(output) {
                "x64".into()
            } else {
                "x86".into()
            }
        }
    };
    let is64bit = architecture == "x64" || architecture == "arm64";
    let vendor = probe_property(output, "java.vendor")
        .or_else(|| probe_property(output, "java.vm.vendor"))
        .or_else(|| {
            let checks: &[(&str, &str)] = &[
                (r"temurin|adoptium", "Eclipse Adoptium"),
                (r"corretto", "Amazon Corretto"),
                (r"zulu|azul", "Azul Systems"),
                (r"liberica|bellsoft", "BellSoft"),
                (r"microsoft", "Microsoft"),
                (r"oracle", "Oracle"),
            ];
            checks
                .iter()
                .find(|(pat, _)| regex::Regex::new(pat).unwrap().is_match(output))
                .map(|(_, name)| name.to_string())
        });
    let mut info = json!({
        "major": major,
        "version": version,
        "is64Bit": is64bit,
        "architecture": architecture
    });
    if let Some(vendor) = vendor {
        info["vendor"] = json!(vendor);
    }
    Some(info)
}

/// spawn `<java> ... -version` 并限时收集输出（CREATE_NO_WINDOW；输出按 stderr\nstdout 合并）
fn run_probe(exe: &str, args: &[&str], timeout_ms: u64) -> Option<String> {
    let mut cmd = std::process::Command::new(exe);
    cmd.args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd.spawn().ok()?;
    let stdout = child.stdout.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>);
    let stderr = child.stderr.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>);
    let read_all = |stream: Option<Box<dyn std::io::Read + Send>>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut s) = stream {
                let _ = std::io::Read::read_to_end(&mut s, &mut buf);
            }
            buf
        })
    };
    let out_t = read_all(stdout);
    let err_t = read_all(stderr);
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if std::time::Instant::now() > deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(std::time::Duration::from_millis(40));
            }
            Err(_) => return None,
        }
    }
    let out = out_t.join().ok()?;
    let err = err_t.join().ok()?;
    let mut text = String::from_utf8_lossy(&err).to_string();
    text.push('\n');
    text.push_str(&String::from_utf8_lossy(&out));
    Some(text)
}

// ---------------- 探测缓存（javaProbeCache.ts） ----------------

fn probe_cache_file(app: &AppHandle) -> PathBuf {
    settings::user_data_dir(app).join("java-probe-cache.json")
}

/// 缓存戳：exe 与 ../release 的大小+mtime（文件变化即失效）
fn probe_stamp(exe: &str) -> String {
    let mut stamps: Vec<String> = Vec::new();
    let release_file = Path::new(exe)
        .parent()
        .map(|d| d.join("..").join("release"))
        .unwrap_or_default();
    for p in [PathBuf::from(exe), release_file] {
        match std::fs::metadata(&p) {
            Ok(m) => stamps.push(format!(
                "{}:{}",
                m.len(),
                m.modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0)
            )),
            Err(_) => stamps.push(String::new()),
        }
    }
    stamps.join("|")
}

fn with_probe_cache<T>(app: &AppHandle, f: impl FnOnce(&mut serde_json::Map<String, Value>) -> T) -> T {
    let mut guard = PROBE_CACHE.lock().unwrap();
    if guard.is_none() {
        let entries = std::fs::read(probe_cache_file(app))
            .ok()
            .and_then(|raw| serde_json::from_slice::<Value>(&raw).ok())
            .and_then(|j| {
                if j["version"] == 1 {
                    j.get("entries").and_then(|e| e.as_object()).cloned()
                } else {
                    None
                }
            })
            .unwrap_or_default();
        *guard = Some(entries);
    }
    f(guard.as_mut().unwrap())
}

static PROBE_CACHE: Mutex<Option<serde_json::Map<String, Value>>> = Mutex::new(None);

fn persist_probe_cache(app: &AppHandle, entries: &serde_json::Map<String, Value>) {
    let file = probe_cache_file(app);
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string(&json!({ "version": 1, "entries": entries })) {
        let _ = std::fs::write(file, text);
    }
}

/// 带缓存的探测：命中条件 = 文件存在 + 戳一致 + 7 天内 + 字段有效
pub fn probe(app: &AppHandle, exe: &str) -> Option<Value> {
    if !Path::new(exe).exists() {
        return None;
    }
    let stamp = probe_stamp(exe);
    let cached: Option<Value> = with_probe_cache(app, |entries| {
        entries.get(exe).and_then(|item| {
            let at = item["at"].as_u64().unwrap_or(0);
            let fresh = at > 0 && now_ms() - at < PROBE_TTL_MS;
            if fresh && item["stamp"].as_str() == Some(stamp.as_str()) {
                let mut info = item["info"].clone();
                if info["major"].is_number() && info["version"].is_string() {
                    info["path"] = json!(exe);
                    return Some(info);
                }
            }
            None
        })
    });
    if cached.is_some() {
        return cached;
    }
    let args: Vec<&str> = JAVA_PROBE_VM_ARGS
        .iter()
        .copied()
        .chain(["-XshowSettings:properties", "-version"])
        .collect();
    let output = run_probe(exe, &args, PROBE_TIMEOUT_MS)?;
    let parsed = parse_java_probe_output(&output)?;
    with_probe_cache(app, |entries| {
        entries.insert(
            exe.to_string(),
            json!({ "stamp": stamp, "at": now_ms(), "info": parsed }),
        );
        persist_probe_cache(app, entries);
    });
    let mut info = parsed;
    info["path"] = json!(exe);
    Some(info)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// ---------------- 候选收集（java.ts quickCandidates） ----------------

fn runtimes_dir(app: &AppHandle) -> PathBuf {
    Path::new(&installer::default_folder_path(app))
        .join(installer::DATA_DIR_NAME)
        .join("runtimes")
}

fn quick_candidates(app: &AppHandle, run_where: bool) -> Vec<JavaCandidate> {
    let mut map: HashMap<String, JavaCandidate> = HashMap::new();
    let s = settings::get(app);

    // 1. 用户指定 / 手动添加
    add_candidate(&mut map, s["javaPath"].as_str(), "当前配置");
    if let Some(list) = s["javaCustom"].as_array() {
        for exe in list {
            add_candidate(&mut map, exe.as_str(), "手动添加");
        }
    }

    // 2. JAVA_HOME
    add_candidate(&mut map, std::env::var("JAVA_HOME").ok().as_deref(), "JAVA_HOME");

    // 3. PATH
    let path_env = std::env::var("Path").or_else(|_| std::env::var("PATH")).unwrap_or_default();
    for item in path_env.split(';') {
        if !item.trim().is_empty() {
            add_candidate(&mut map, Some(item.trim()), "PATH");
        }
    }
    if run_where {
        if let Some(out) = run_probe("where.exe", &["java"], 10_000) {
            for line in out.lines() {
                add_candidate(&mut map, Some(line.trim()), "PATH");
            }
        }
    }

    // 4. 当前系统盘常见安装目录
    let system_drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
    for dn in ["Java", "Eclipse Adoptium", "Microsoft", "Zulu", "Amazon Corretto", "BellSoft\\Liberica", "JavaSoft\\JRE"] {
        let base = format!("{}\\Program Files\\{}", system_drive, dn);
        if let Ok(entries) = std::fs::read_dir(&base) {
            for sub in entries.flatten() {
                add_candidate(&mut map, Some(&sub.path().to_string_lossy()), "常见安装目录");
            }
        }
    }
    // 官方启动器运行时目录
    let rt_base = dirs::data_dir().unwrap_or_default().join(".minecraft").join("runtime");
    if let Ok(l1s) = std::fs::read_dir(&rt_base) {
        for l1 in l1s.flatten() {
            let l1p = l1.path();
            if let Ok(l2s) = std::fs::read_dir(&l1p) {
                for l2 in l2s.flatten() {
                    add_candidate(&mut map, Some(&l2.path().join(l1.file_name()).to_string_lossy()), "Minecraft 官方 Runtime");
                }
            }
        }
    }

    // 5. 启动器自管理 runtimes 目录（官方包层级不定：组件目录本身或其子目录下找得到 bin\java.exe 才算）
    if let Ok(entries) = std::fs::read_dir(runtimes_dir(app)) {
        for sub in entries.flatten() {
            let dir = sub.path();
            if !dir.is_dir() {
                continue;
            }
            let mut found = dir.join("bin").join(JAVA_EXE).is_file();
            if !found {
                if let Ok(l1s) = std::fs::read_dir(&dir) {
                    for l1 in l1s.flatten() {
                        if l1.path().join("bin").join(JAVA_EXE).is_file() {
                            add_candidate(&mut map, Some(&l1.path().to_string_lossy()), "WUHUICL Runtime");
                            found = true;
                        } else if let Ok(l2s) = std::fs::read_dir(l1.path()) {
                            for l2 in l2s.flatten() {
                                if l2.path().join("bin").join(JAVA_EXE).is_file() {
                                    add_candidate(&mut map, Some(&l2.path().to_string_lossy()), "WUHUICL Runtime");
                                    found = true;
                                }
                            }
                        }
                    }
                }
            }
            if found && dir.join("bin").join(JAVA_EXE).is_file() {
                add_candidate(&mut map, Some(&dir.to_string_lossy()), "WUHUICL Runtime");
            }
        }
    }

    map.into_values().collect()
}

// ---------------- 扫描缓存 ----------------

fn scan_cache_file(app: &AppHandle) -> PathBuf {
    settings::user_data_dir(app).join("java-scan-cache.json")
}

fn read_persistent_cache(app: &AppHandle) -> Option<(u64, Vec<Value>)> {
    let raw = std::fs::read(scan_cache_file(app)).ok()?;
    let parsed: Value = serde_json::from_slice(&raw).ok()?;
    if parsed["version"] != 1 {
        return None;
    }
    let time = parsed["scannedAt"].as_u64()?;
    let list: Vec<Value> = parsed["list"]
        .as_array()?
        .iter()
        .filter(|item| {
            item["path"].is_string()
                && item["major"].is_number()
                && item["version"].is_string()
                && item["is64Bit"].is_boolean()
                && Path::new(item["path"].as_str().unwrap_or("")).exists()
        })
        .cloned()
        .collect();
    Some((time, list))
}

fn write_persistent_cache(app: &AppHandle, list: &[Value]) {
    let file = scan_cache_file(app);
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let payload = json!({ "version": 1, "scannedAt": now_ms(), "list": list });
    if let Ok(text) = serde_json::to_string_pretty(&payload) {
        let _ = std::fs::write(file, text);
    }
}

fn cached_complete_list(app: &AppHandle, max_age: u64) -> Option<Vec<Value>> {
    {
        let cache = SCAN_CACHE.lock().unwrap();
        if let Some(c) = cache.as_ref() {
            if c.complete && now_ms() - c.time < max_age {
                return Some(c.list.clone());
            }
        }
    }
    let (time, list) = read_persistent_cache(app)?;
    if now_ms() - time >= max_age {
        return None;
    }
    let mut cache = SCAN_CACHE.lock().unwrap();
    *cache = Some(ScanCache { time, list: list.clone(), complete: true });
    Some(list)
}

fn sort_java(mut list: Vec<Value>) -> Vec<Value> {
    list.sort_by(|a, b| {
        let am = a["major"].as_i64().unwrap_or(0);
        let bm = b["major"].as_i64().unwrap_or(0);
        bm.cmp(&am)
            .then_with(|| b["is64Bit"].as_bool().unwrap_or(false).cmp(&a["is64Bit"].as_bool().unwrap_or(false)))
            .then_with(|| {
                a["path"]
                    .as_str()
                    .unwrap_or("")
                    .to_lowercase()
                    .cmp(&b["path"].as_str().unwrap_or("").to_lowercase())
            })
    });
    list
}

/// java.ts mergeCustom：合并手动添加 + 过滤隐藏
fn merge_custom(app: &AppHandle, list: Vec<Value>) -> Vec<Value> {
    let s = settings::get(app);
    let hidden: HashSet<String> = s["javaHidden"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|p| p.as_str())
                .flat_map(|p| [path_key(p), path_key(&real_executable(p).unwrap_or_else(|| p.to_string()))])
                .collect()
        })
        .unwrap_or_default();
    let auto: Vec<Value> = list
        .into_iter()
        .filter(|j| {
            let p = j["path"].as_str().unwrap_or("");
            !hidden.contains(&path_key(p))
                && !hidden.contains(&path_key(&real_executable(p).unwrap_or_else(|| p.to_string())))
        })
        .collect();
    let mut manual: Vec<Value> = Vec::new();
    if let Some(custom) = s["javaCustom"].as_array() {
        for p in custom.iter().filter_map(|v| v.as_str()) {
            let real = match real_executable(p) {
                Some(r) => r,
                None => continue,
            };
            if hidden.contains(&path_key(&real)) {
                continue;
            }
            if auto.iter().any(|j| path_key(j["path"].as_str().unwrap_or("")) == path_key(&real)) {
                continue;
            }
            if manual.iter().any(|j| path_key(j["path"].as_str().unwrap_or("")) == path_key(&real)) {
                continue;
            }
            if let Some(info) = probe(app, &real) {
                let mut item = info;
                item["source"] = json!("manual");
                item["sourceDetail"] = json!("手动添加");
                manual.push(item);
            }
        }
    }
    manual.extend(auto);
    sort_java(manual)
}

/// java.ts scanJava：启动热路径的同步快速扫描（不遍历磁盘）
pub fn scan_quick(app: &AppHandle, refresh: bool) -> Vec<Value> {
    if !refresh {
        {
            let cache = SCAN_CACHE.lock().unwrap();
            if let Some(c) = cache.as_ref() {
                if now_ms() - c.time < SCAN_TTL_MS {
                    return merge_custom(app, c.list.clone());
                }
            }
        }
        if let Some(persisted) = cached_complete_list(app, PERSISTENT_SCAN_TTL_MS) {
            return merge_custom(app, persisted);
        }
    }
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<Value> = Vec::new();
    for candidate in quick_candidates(app, true) {
        let Some(real) = real_executable(&candidate.executable) else { continue };
        let key = path_key(&real);
        if seen.contains(&key) {
            continue;
        }
        seen.insert(key);
        if let Some(info) = probe(app, &real) {
            let mut item = info;
            item["path"] = json!(candidate.executable);
            item["source"] = json!("auto");
            item["sourceDetail"] = json!(candidate.source_detail);
            out.push(item);
        }
    }
    let list = sort_java(out);
    {
        let mut cache = SCAN_CACHE.lock().unwrap();
        *cache = Some(ScanCache { time: now_ms(), list: list.clone(), complete: false });
    }
    merge_custom(app, list)
}

/// java.ts listJavaSummary：4 线程并发探测（java:list）
fn list_java_summary(app: &AppHandle) -> Vec<Value> {
    let started = std::time::Instant::now();
    let s = settings::get(app);
    let mut candidates: Vec<JavaCandidate> = quick_candidates(app, false);
    if let Some((_, list)) = read_persistent_cache(app) {
        for info in list {
            candidates.push(JavaCandidate {
                executable: info["path"].as_str().unwrap_or("").to_string(),
                source_detail: info["sourceDetail"].as_str().unwrap_or("已缓存").to_string(),
            });
        }
    }
    let mut unique: HashMap<String, JavaCandidate> = HashMap::new();
    for c in candidates {
        if let Some(real) = real_executable(&c.executable) {
            let key = path_key(&real);
            unique.entry(key).or_insert(JavaCandidate { executable: real, source_detail: c.source_detail });
        }
    }
    let pending: Vec<JavaCandidate> = unique.into_values().collect();
    let cursor = Arc::new(AtomicUsize::new(0));
    let found: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let workers = pending.len().min(4).max(1);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let cursor = cursor.clone();
            let found = found.clone();
            let pending = &pending;
            scope.spawn(move || loop {
                let index = cursor.fetch_add(1, Ordering::SeqCst);
                if index >= pending.len() {
                    return;
                }
                let c = &pending[index];
                let info = probe(app, &c.executable);
                if let Some(info) = info {
                    let mut item = info;
                    item["source"] = json!(if c.source_detail == "手动添加" { "manual" } else { "auto" });
                    item["sourceDetail"] = json!(c.source_detail);
                    found.lock().unwrap().push(item);
                }
            });
        }
    });
    let hidden: HashSet<String> = s["javaHidden"]
        .as_array()
        .map(|a| a.iter().filter_map(|p| p.as_str()).map(path_key).collect())
        .unwrap_or_default();
    let list: Vec<Value> = found
        .lock()
        .unwrap()
        .iter()
        .filter(|j| !hidden.contains(&path_key(j["path"].as_str().unwrap_or(""))))
        .cloned()
        .collect();
    let _ = started;
    sort_java(list)
}

// ---------------- 注册表扫描（java.ts registryJavaHomes） ----------------

const REGISTRY_JAVA_KEYS: &[(&str, bool)] = &[
    ("SOFTWARE\\JavaSoft", false),
    ("SOFTWARE\\WOW6432Node\\JavaSoft", false),
    ("SOFTWARE\\JavaSoft", true), // HKCU
    ("SOFTWARE\\Eclipse Adoptium", false),
    ("SOFTWARE\\WOW6432Node\\Eclipse Adoptium", false),
    ("SOFTWARE\\Microsoft\\JDK", false),
    ("SOFTWARE\\WOW6432Node\\Microsoft\\JDK", false),
    ("SOFTWARE\\Azul Systems", false),
    ("SOFTWARE\\BellSoft", false),
    ("SOFTWARE\\Amazon Corretto", false),
];

fn registry_value_strings(key: &windows_registry::Key, out: &mut Vec<String>, depth: usize) {
    if depth > 4 {
        return;
    }
    if let Ok(values) = key.values() {
        for (name, value) in values {
            let lname = name.to_lowercase();
            if matches!(lname.as_str(), "javahome" | "java_home" | "path" | "home" | "installationpath") {
                if let Ok(s) = String::try_from(value) {
                    let expanded = expand_windows_environment(&s);
                    if regex::Regex::new(r"^[a-z]:\\").unwrap().is_match(&expanded.to_lowercase()) {
                        out.push(expanded);
                    }
                }
            }
        }
    }
    if let Ok(keys) = key.keys() {
        for sub in keys {
            if let Ok(sub_key) = key.open(&sub) {
                registry_value_strings(&sub_key, out, depth + 1);
            }
        }
    }
}

fn registry_java_homes() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (sub_key, hkcu) in REGISTRY_JAVA_KEYS {
        let opened = if *hkcu {
            windows_registry::CURRENT_USER.open(sub_key)
        } else {
            windows_registry::LOCAL_MACHINE.open(sub_key)
        };
        if let Ok(key) = opened {
            registry_value_strings(&key, &mut out, 0);
        }
    }
    // 去重（大小写不敏感，保序）
    let mut seen: HashSet<String> = HashSet::new();
    let mut result = Vec::new();
    for item in out {
        let normalized = item.replace('/', "\\");
        if seen.insert(normalized.to_lowercase()) {
            result.push(normalized);
        }
    }
    result
}

// ---------------- 完整扫描（java.ts scanJavaInstallations） ----------------

struct ScanRoot {
    directory: String,
    label: String,
    max_depth: usize,
    max_directories: usize,
    shallow_depth: Option<usize>,
}

fn add_scan_root(roots: &mut HashMap<String, ScanRoot>, root: ScanRoot) {
    if !Path::new(&root.directory).is_dir() {
        return;
    }
    let key = path_key(&root.directory);
    match roots.get(&key) {
        Some(existing) if existing.max_depth >= root.max_depth => {}
        _ => {
            roots.insert(key, root);
        }
    }
}

/// javaScanUtils.shouldPruneJavaDirectory：大目录扫描永不进入
fn should_prune_java_directory(name: &str) -> bool {
    matches!(
        name.to_lowercase().as_str(),
        "$recycle.bin"
            | "system volume information"
            | "windows"
            | "winsxs"
            | "system32"
            | "syswow64"
            | "node_modules"
            | ".git"
            | "libraries"
            | "assets"
            | "versions"
            | "mods"
            | "saves"
            | "screenshots"
            | "logs"
            | "crash-reports"
            | "jmods"
            | "lib"
            | "legal"
            | "include"
            | "src"
            | "cache"
            | "caches"
            | "temp"
            | "tmp"
    )
}

fn fixed_windows_drives() -> Vec<String> {
    let mut drives = Vec::new();
    for c in b'C'..=b'Z' {
        let drive = format!("{}:\\", c as char);
        if Path::new(&drive).is_dir() {
            drives.push((c as char).to_string() + ":");
        }
    }
    if drives.is_empty() {
        drives.push(std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into()).to_uppercase());
    }
    drives
}

fn windows_scan_roots(drives: &[String]) -> Vec<ScanRoot> {
    let mut roots: HashMap<String, ScanRoot> = HashMap::new();
    let vendor_dirs = [
        "Java", "Eclipse Adoptium", "Microsoft", "Zulu", "Amazon Corretto", "BellSoft", "IBM", "Semeru",
        "JetBrains", "Android", "Minecraft Launcher", "PrismLauncher", "Modrinth App", "CurseForge",
    ];
    let root_hints = regex::Regex::new(
        r"^(?:java|jdk|jre)(?:[-_. ].*)?$|^(?:apps?|tools?|software|programs?|development|dev|minecraft|games?|launchers?|runtimes?)$",
    )
    .unwrap();
    for drive in drives {
        add_scan_root(
            &mut roots,
            ScanRoot {
                directory: format!("{}\\", drive),
                label: format!("{} 固定磁盘浅层扫描", drive),
                max_depth: 10,
                max_directories: 8000,
                shallow_depth: Some(2),
            },
        );
        for program_dir in ["Program Files", "Program Files (x86)"] {
            for vendor in vendor_dirs {
                add_scan_root(
                    &mut roots,
                    ScanRoot {
                        directory: format!("{}\\{}\\{}", drive, program_dir, vendor),
                        label: format!("{} 常见安装目录", drive),
                        max_depth: 7,
                        max_directories: 12000,
                        shallow_depth: None,
                    },
                );
            }
        }
        for name in ["Java", "JDK", "JRE", "Apps", "Tools", "Software", "Programs", "Development", "Dev", "Minecraft", "Games", "Launchers", "Runtimes"] {
            add_scan_root(
                &mut roots,
                ScanRoot {
                    directory: format!("{}\\{}", drive, name),
                    label: format!("{} 本地磁盘", drive),
                    max_depth: 6,
                    max_directories: 12000,
                    shallow_depth: None,
                },
            );
        }
        if let Ok(top) = std::fs::read_dir(format!("{}\\", drive)) {
            for item in top.flatten() {
                let name = item.file_name().to_string_lossy().to_string();
                if !root_hints.is_match(&name) {
                    continue;
                }
                add_scan_root(
                    &mut roots,
                    ScanRoot {
                        directory: item.path().to_string_lossy().to_string(),
                        label: format!("{} 本地磁盘", drive),
                        max_depth: 6,
                        max_directories: 12000,
                        shallow_depth: None,
                    },
                );
            }
        }
    }

    let user_home = dirs::home_dir().unwrap_or_default();
    let app_data = dirs::data_dir().unwrap_or_default();
    let local_app_data = std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| app_data.parent().map(|p| p.join("Local")).unwrap_or_default());
    let user_roots: Vec<(PathBuf, &str, usize)> = vec![
        (app_data.join(".minecraft").join("runtime"), "Minecraft 官方 Runtime", 8),
        (app_data.join("PrismLauncher"), "Prism Launcher Runtime", 7),
        (app_data.join("ModrinthApp"), "Modrinth Runtime", 7),
        (app_data.join("com.modrinth.theseus"), "Modrinth Runtime", 8),
        (local_app_data.join("Programs"), "用户程序目录", 6),
        (user_home.join(".jdks"), "IDE JDK", 5),
        (user_home.join(".gradle").join("jdks"), "Gradle JDK", 5),
        (user_home.join(".lunarclient"), "Lunar Client Runtime", 7),
        (user_home.join(".badlion"), "Badlion Runtime", 7),
    ];
    for (directory, label, max_depth) in user_roots {
        add_scan_root(
            &mut roots,
            ScanRoot {
                directory: directory.to_string_lossy().to_string(),
                label: label.to_string(),
                max_depth,
                max_directories: 16000,
                shallow_depth: None,
            },
        );
    }
    roots.into_values().collect()
}

/// java.ts discoverInRoot：BFS + java/jdk/jre 线索深挖
fn discover_in_root(root: &ScanRoot, candidates: &mut HashMap<String, JavaCandidate>, rec: &TaskRecord) -> usize {
    let mut queue: Vec<(String, usize, bool)> = vec![(root.directory.clone(), 0, false)];
    let deep_hint = regex::Regex::new(r"^(?:java|jdk|jre|jbr|runtime)(?:[-_. ].*)?$").unwrap();
    let mut cursor = 0;
    let mut visited = 0;
    while cursor < queue.len() && visited < root.max_directories {
        if rec.cancelled.load(Ordering::SeqCst) {
            return visited;
        }
        tasks::wait_while_paused(rec);
        let (current, depth, promoted) = queue[cursor].clone();
        cursor += 1;
        visited += 1;
        let entries = match std::fs::read_dir(&current) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            if rec.cancelled.load(Ordering::SeqCst) {
                return visited;
            }
            let full = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            let is_file = entry.file_type().map(|t| t.is_file()).unwrap_or(false);
            if is_file && name.to_lowercase() == JAVA_EXE {
                add_candidate(candidates, Some(&full.to_string_lossy()), &root.label);
                continue;
            }
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            let is_symlink = entry.file_type().map(|t| t.is_symlink()).unwrap_or(false);
            if !is_dir || is_symlink || should_prune_java_directory(&name) {
                continue;
            }
            let promoted = promoted || deep_hint.is_match(&name);
            let depth = depth + 1;
            let depth_limit = if promoted { root.max_depth } else { root.shallow_depth.unwrap_or(root.max_depth) };
            if depth > depth_limit {
                continue;
            }
            queue.push((full.to_string_lossy().to_string(), depth, promoted));
        }
    }
    visited
}

fn scan_progress(app: &AppHandle, emit: &(dyn Fn(&Value) + Send + Sync), progress: f64, text: &str) {
    emit(&json!({ "stage": "java-scan", "progress": progress, "overall": progress, "text": text, "indeterminate": false }));
    let _ = app;
}

/// java.ts scanJavaInstallations：注册表 + 环境变量 + 固定磁盘全量扫描
fn scan_java_installations(
    app: &AppHandle,
    rec: &TaskRecord,
    emit: &(dyn Fn(&Value) + Send + Sync),
    refresh: bool,
) -> Result<Vec<Value>, String> {
    let started = std::time::Instant::now();
    if !refresh {
        if let Some(cached) = cached_complete_list(app, PERSISTENT_SCAN_TTL_MS) {
            scan_progress(app, emit, 1.0, &format!("已从缓存载入 {} 个 Java", cached.len()));
            return Ok(merge_custom(app, cached));
        }
    }
    if rec.cancelled.load(Ordering::SeqCst) {
        return Err("已取消".into());
    }
    scan_progress(app, emit, 0.02, "正在读取 Java 配置、PATH 与注册表…");
    let mut candidates: HashMap<String, JavaCandidate> = HashMap::new();
    for candidate in quick_candidates(app, true) {
        add_candidate(&mut candidates, Some(&candidate.executable), &candidate.source_detail);
    }
    for home in registry_java_homes() {
        add_candidate(&mut candidates, Some(&home), "Windows 注册表");
    }
    if rec.cancelled.load(Ordering::SeqCst) {
        return Err("已取消".into());
    }
    let drives = fixed_windows_drives();
    let roots = windows_scan_roots(&drives);
    for (i, root) in roots.iter().enumerate() {
        scan_progress(
            app,
            emit,
            0.08 + (i as f64 / roots.len().max(1) as f64) * 0.52,
            &format!("正在扫描 {}：{}", root.label, root.directory),
        );
        discover_in_root(root, &mut candidates, rec);
    }
    if rec.cancelled.load(Ordering::SeqCst) {
        return Err("已取消".into());
    }

    // 去重到真实可执行文件
    let mut unique: HashMap<String, JavaCandidate> = HashMap::new();
    for candidate in candidates.values() {
        let Some(real) = real_executable(&candidate.executable) else { continue };
        let key = path_key(&real);
        unique.entry(key).or_insert(JavaCandidate {
            executable: real,
            source_detail: candidate.source_detail.clone(),
        });
    }
    let pending: Vec<JavaCandidate> = unique.into_values().collect();
    let found: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let cursor = Arc::new(AtomicUsize::new(0));
    let completed = Arc::new(AtomicUsize::new(0));
    let total = pending.len();
    let workers = total.min(4).max(1);
    std::thread::scope(|scope| -> Result<(), String> {
        for _ in 0..workers {
            let cursor = cursor.clone();
            let found = found.clone();
            let completed = completed.clone();
            let pending = &pending;
            scope.spawn(move || loop {
                if rec.cancelled.load(Ordering::SeqCst) {
                    return;
                }
                tasks::wait_while_paused(rec);
                let index = cursor.fetch_add(1, Ordering::SeqCst);
                if index >= pending.len() {
                    return;
                }
                let candidate = &pending[index];
                let info = probe(app, &candidate.executable);
                let done = completed.fetch_add(1, Ordering::SeqCst) + 1;
                let text = match &info {
                    Some(i) => format!(
                        "正在验证 Java {}/{}：Java {} {}",
                        done,
                        total,
                        i["major"].as_i64().unwrap_or(0),
                        i["architecture"].as_str().unwrap_or("")
                    ),
                    None => format!("正在验证 Java {}/{}", done, total),
                };
                scan_progress(app, emit, 0.62 + (done as f64 / total.max(1) as f64) * 0.37, &text);
                if let Some(i) = info {
                    let mut item = i;
                    item["path"] = json!(candidate.executable);
                    item["source"] = json!("auto");
                    item["sourceDetail"] = json!(candidate.source_detail);
                    found.lock().unwrap().push(item);
                }
            });
        }
        Ok(())
    })?;
    if rec.cancelled.load(Ordering::SeqCst) {
        return Err("已取消".into());
    }

    let list = sort_java(found.lock().unwrap().clone());
    {
        let mut cache = SCAN_CACHE.lock().unwrap();
        *cache = Some(ScanCache { time: now_ms(), list: list.clone(), complete: true });
    }
    write_persistent_cache(app, &list);
    // 用户点「重新扫描」= 要一份最新真相：曾被隐藏但磁盘上真实存在的 Java 自动解除隐藏
    if refresh {
        let s = settings::get(app);
        let hidden = s["javaHidden"].as_array().cloned().unwrap_or_default();
        if !hidden.is_empty() {
            let found_keys: HashSet<String> = list
                .iter()
                .filter_map(|j| j["path"].as_str())
                .map(path_key)
                .collect();
            let restore: Vec<Value> = hidden
                .iter()
                .filter(|p| p.as_str().map(|p| found_keys.contains(&path_key(p))).unwrap_or(false))
                .cloned()
                .collect();
            if !restore.is_empty() {
                let remaining: Vec<Value> = hidden
                    .into_iter()
                    .filter(|p| {
                        p.as_str()
                            .map(|p| !found_keys.contains(&path_key(p)))
                            .unwrap_or(true)
                    })
                    .collect();
                let _ = settings::save(app, json!({ "javaHidden": remaining }));
            }
        }
    }
    let _ = started;
    scan_progress(app, emit, 1.0, &format!("扫描完成，共找到 {} 个可用 Java", list.len()));
    Ok(merge_custom(app, list))
}

// ---------------- 通道实现 ----------------

/// java:refresh：注册任务 + 后台完整扫描（invoke 阻塞至完成，与原版 await 一致）
fn java_refresh(app: &AppHandle, args: &[Value]) -> Result<Value, String> {
    {
        let active = ACTIVE_SCAN_TASK.lock().unwrap();
        if active.is_some() {
            return Err("Java 扫描已在进行中".into());
        }
    }
    let refresh = args.first().map(|v| v != &Value::Bool(false)).unwrap_or(true);
    let rec = tasks::register_task("扫描本机 Java", "java");
    *ACTIVE_SCAN_TASK.lock().unwrap() = Some(rec.id.clone());
    let task_id = rec.id.clone();
    let task_title = rec.title.clone();
    let app2 = app.clone();
    let task_id2 = task_id.clone();
    let result = (|| -> Result<Vec<Value>, String> {
        // 进度归一化需可变状态；emit 会进入扫描工作线程，用 Mutex 保证 Fn+Send+Sync
        let guard = Mutex::new(ProgressGuard::new());
        let emit = move |event: &Value| {
            let mut g = guard.lock().unwrap();
            let mut e = g.normalize(event);
            if let Some(obj) = e.as_object_mut() {
                obj.insert("taskId".into(), json!(task_id2));
                obj.insert("taskTitle".into(), json!(task_title));
            }
            tasks::emit_progress(&app2, e);
        };
        scan_java_installations(app, &rec, &emit, refresh)
    })();
    *ACTIVE_SCAN_TASK.lock().unwrap() = None;
    match result {
        Ok(list) => {
            tasks::task_done(app, &task_id, true, None, false, None);
            Ok(json!(list))
        }
        Err(e) => {
            let cancelled = e == "已取消" || rec.cancelled.load(Ordering::SeqCst);
            tasks::task_done(app, &task_id, false, Some(e.clone()), cancelled, Some("java-scan".into()));
            Err(e)
        }
    }
}

/// java:cancelScan
fn cancel_scan() -> bool {
    let active = ACTIVE_SCAN_TASK.lock().unwrap().clone();
    match active {
        Some(task_id) => tasks::cancel_task(&task_id),
        None => false,
    }
}

/// java:addCustom（真实执行 -version 校验后加入 javaCustom）
fn add_custom_java(app: &AppHandle, java_path: &str) -> Result<(), String> {
    let real = real_executable(java_path);
    let info = real.as_ref().and_then(|r| probe(app, r));
    if info.is_none() {
        return Err("这不是有效的 Java（java -version 校验失败）".into());
    }
    let real = real.unwrap();
    let s = settings::get(app);
    let mut list: Vec<Value> = s["javaCustom"].as_array().cloned().unwrap_or_default();
    if !list.iter().any(|p| p.as_str().map(|p| path_key(p) == path_key(&real)).unwrap_or(false)) {
        list.push(json!(real));
    }
    // 若曾被隐藏则取消隐藏
    let hidden: Vec<Value> = s["javaHidden"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|p| p.as_str().map(|p| path_key(p) != path_key(&real)).unwrap_or(true))
        .collect();
    settings::save(app, json!({ "javaCustom": list, "javaHidden": hidden }))?;
    Ok(())
}

/// java:pickAdd（ipc.ts）：弹窗选 exe → 校验入库 → 返回最新列表
fn pick_add_java(app: &AppHandle) -> Result<Value, String> {
    use tauri_plugin_dialog::DialogExt;
    let picked = app
        .dialog()
        .file()
        .add_filter("Java 可执行文件", &["exe"])
        .add_filter("所有文件", &["*"])
        .blocking_pick_file();
    let Some(picked) = picked else { return Ok(Value::Null) };
    add_custom_java(app, &picked.to_string())?;
    Ok(json!(scan_quick(app, false)))
}

/// java:hide
fn hide_java(app: &AppHandle, java_path: &str) -> Result<(), String> {
    let s = settings::get(app);
    let mut hidden: Vec<Value> = s["javaHidden"].as_array().cloned().unwrap_or_default();
    let value = json!(java_path);
    if !hidden.contains(&value) {
        hidden.push(value);
    }
    settings::save(app, json!({ "javaHidden": hidden }))?;
    Ok(())
}

/// launch.rs selectJavaByMajor：need + 64位 + 架构精确匹配
pub fn select_by_major(list: &[Value], need: i64, architecture: &str) -> Option<Value> {
    list.iter()
        .find(|j| {
            j["major"].as_i64() == Some(need)
                && j["is64Bit"].as_bool().unwrap_or(false)
                && (architecture.is_empty() || j["architecture"].as_str().map(|a| a == architecture).unwrap_or(false))
        })
        .cloned()
}

// ---------------- Mojang 官方运行时自动下载（PCL SelectOrDownloadJava / 美西螈 jre.rs 同款） ----------------
//
// 索引：https://piston-meta.mojang.com/v1/products/java-runtime/<固定hash>/all.json
// （piston-meta 在 bmclapi 模式下经 mirror_url 自动改写；文件下载走 download_candidates 双源兜底）
// 组件映射（美西螈 jre.rs:736-745 口径）：8→jre-legacy、16→alpha、17→gamma、21→delta、25→epsilon；
// 未映射的主版本按最近下限兜底（如 22/23/24 → delta 的 21 也能跑大多数场景，probe 校验兜底）。

static JAVA_INSTALL_LOCK: Mutex<()> = Mutex::new(());

const JAVA_RUNTIME_INDEX_URL: &str =
    "https://piston-meta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json";

fn runtime_component(need: i64) -> &'static str {
    match need {
        8 => "jre-legacy",
        16 => "java-runtime-alpha",
        17 => "java-runtime-gamma",
        21 => "java-runtime-delta",
        25 => "java-runtime-epsilon",
        n if n >= 25 => "java-runtime-epsilon",
        n if n >= 21 => "java-runtime-delta",
        n if n >= 17 => "java-runtime-gamma",
        n if n >= 16 => "java-runtime-alpha",
        _ => "jre-legacy",
    }
}

/// 已安装运行时的 java.exe：组件目录本身或其两层子目录里找 bin\java.exe
fn runtime_java_exe(component_dir: &Path) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = vec![component_dir.join("bin").join(JAVA_EXE)];
    if let Ok(rd) = std::fs::read_dir(component_dir) {
        for l1 in rd.flatten() {
            let p1 = l1.path();
            if !p1.is_dir() {
                continue;
            }
            candidates.push(p1.join("bin").join(JAVA_EXE));
            if let Ok(rd2) = std::fs::read_dir(&p1) {
                for l2 in rd2.flatten() {
                    let p2 = l2.path();
                    if p2.is_dir() {
                        candidates.push(p2.join("bin").join(JAVA_EXE));
                    }
                }
            }
        }
    }
    candidates.into_iter().find(|c| c.is_file())
}

/// 自动管理下的 Java 缺失：下载 Mojang 官方运行时并返回 java.exe 路径。
/// 进度经 event:progress（stage=java）上报；全程持全局锁防并发重复下载。
pub fn ensure_runtime(app: &AppHandle, need: i64, arch: &str) -> Result<PathBuf, String> {
    if arch == "arm64" {
        return Err(format!("自动下载暂不支持 {} 的 Java {} 运行时，请手动安装", arch, need));
    }
    let _guard = JAVA_INSTALL_LOCK.lock().unwrap();
    let component = runtime_component(need);
    let component_dir = runtimes_dir(app).join(component);

    // 已装过：直接复用（.installing 残留视为半成品，删除重装）
    if let Some(exe) = runtime_java_exe(&component_dir) {
        return Ok(exe);
    }
    let _ = std::fs::remove_dir_all(&component_dir);
    let _ = std::fs::create_dir_all(runtimes_dir(app));

    let mirror = installer::mirror_pref(app).to_string();
    let platform = "windows-x64";

    // 1. 平台索引 → 组件 manifest
    let fetch = |url: &str| -> Result<Value, String> {
        let text = crate::net::get_text(url, 20_000)?;
        serde_json::from_str(&text).map_err(|e| e.to_string())
    };
    let index = fetch(&installer::mirror_url(JAVA_RUNTIME_INDEX_URL, &mirror))
        .or_else(|_| fetch(JAVA_RUNTIME_INDEX_URL))?;
    let entries = index
        .get(platform)
        .and_then(|p| p.get(component))
        .and_then(|c| c.as_array())
        .ok_or_else(|| format!("官方运行时索引缺少 {} / {}", platform, component))?;
    let manifest_url = entries
        .iter()
        .find_map(|e| e.get("manifest").and_then(|m| m.get("url")).and_then(|v| v.as_str()))
        .ok_or_else(|| format!("官方运行时索引中 {} 没有 manifest", component))?;

    // 2. manifest：现代格式 {"files": {相对路径: {type: file|directory|link, downloads: {raw|lzma}}}}
    //    （链接只解析运行时内部目标，防目录穿越）
    let manifest = fetch(&installer::mirror_url(manifest_url, &mirror)).or_else(|_| fetch(manifest_url))?;
    let files = manifest
        .get("files")
        .and_then(|f| f.as_object())
        .ok_or_else(|| "运行时 manifest 格式无效".to_string())?;

    emit_java_progress(app, 0.0, &format!("准备下载 Java {} 运行时", need));
    let staging = runtimes_dir(app).join(format!("{}.installing", component));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;

    let result = (|| -> Result<PathBuf, String> {
        let mut items: Vec<installer::DownloadItem> = Vec::new();
        for (rel, node) in files {
            if rel.is_empty() || rel.contains("..") {
                continue;
            }
            let dest = staging.join(rel.replace('/', "\\"));
            match node.get("type").and_then(|v| v.as_str()).unwrap_or("") {
                "directory" => {
                    let _ = std::fs::create_dir_all(&dest);
                }
                "file" => {
                    // 用 raw 原样包（lzma 包需解压；piston-data 域名在 bmclapi 模式下自动改写）
                    let raw = node.get("downloads").and_then(|d| d.get("raw"));
                    let Some(url) = raw.and_then(|r| r.get("url")).and_then(|v| v.as_str()) else {
                        continue;
                    };
                    items.push(installer::DownloadItem {
                        label: rel.clone(),
                        url: url.to_string(),
                        dest,
                        sha1: raw.and_then(|r| r.get("sha1")).and_then(|v| v.as_str()).map(str::to_string),
                        size: raw.and_then(|r| r.get("size")).and_then(|v| v.as_u64()),
                        reuse_files: vec![],
                        reuse_dirs: vec![],
                    });
                }
                "link" => {
                    let Some(target) = node.get("target").and_then(|v| v.as_str()) else { continue };
                    if target.contains("..") {
                        continue;
                    }
                    let from = staging.join(target.replace('/', "\\"));
                    if from.is_file() {
                        let _ = std::fs::create_dir_all(dest.parent().unwrap_or(&staging));
                        let _ = std::fs::hard_link(&from, &dest).or_else(|_| std::fs::copy(&from, &dest).map(|_| ()));
                    }
                }
                _ => {}
            }
        }
        if items.is_empty() {
            return Err("运行时 manifest 里没有可下载文件".into());
        }

        let rec = tasks::register_task(&format!("下载 Java {} 运行时", need), "java");
        let app2 = app.clone();
        let progress = move |d: usize, t: usize, _speed: f64, detail: &installer::DownloadDetail| {
            let fraction = detail
                .bytes_total
                .filter(|bt| *bt > 0)
                .map(|bt| detail.bytes_done as f64 / bt as f64)
                .unwrap_or_else(|| d as f64 / t.max(1) as f64);
            emit_java_progress(&app2, fraction, &format!("下载 Java {} 运行时 {}/{}", need, d, t.max(1)));
        };
        let dl = installer::download_all(app, Some(&rec), items, &mirror, &progress);
        tasks::task_done(app, &rec.id, dl.is_ok(), dl.as_ref().err().map(|e| e.clone()), false, None);
        dl?;

        // 3. 找 java.exe → 从 staging 就位
        let exe = runtime_java_exe(&staging).ok_or_else(|| "运行时解压完成但未找到 java.exe".to_string())?;
        std::fs::rename(&staging, &component_dir).map_err(|e| e.to_string())?;
        let final_exe = component_dir.join(exe.strip_prefix(&staging).unwrap_or(&exe));
        emit_java_progress(app, 1.0, &format!("Java {} 运行时就绪", need));
        Ok(final_exe)
    })();

    match result {
        Ok(p) => Ok(p),
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging);
            Err(format!("自动下载 Java {} 失败：{}", need, e))
        }
    }
}

fn emit_java_progress(app: &AppHandle, fraction: f64, text: &str) {
    tasks::emit_progress(
        app,
        json!({ "stage": "java", "progress": fraction.clamp(0.0, 1.0), "text": text }),
    );
}
