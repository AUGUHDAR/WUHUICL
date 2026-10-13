//! 社区资源子系统（对应原版 KAMUCL-1.1.21/src/main/core/ 的 community.ts、
//! community-zh.ts、mcmodSearch.ts、communityPaging.ts、curseforgeChannel.ts、
//! curseforgeKey.ts、downloadFileName.ts，以及 shared/communityPolicy.ts、
//! shared/modCompatibility.ts 的纯函数部分——mods.rs 复用本模块的 pub 函数）。
//!
//! 通道：community:search / community:project / community:files / community:download
//!
//! 与原版的差异（语义保持一致的前提下）：
//! 1. AbortSignal 取消模型 → tasks.rs TaskRecord.cancelled 轮询（检查点位于下载/循环边界）。
//! 2. mcmodSearch 的并发 2 请求槽与并发条目抓取 → 顺序执行（只影响耗时，不影响结果与文案）。
//! 3. mcmodSearch/community-zh 的 NFKC 归一化 → 手写等价子集（全角 ASCII 折叠 + 小写 +
//!    空白/装饰符剔除；中文关键词场景与 NFKC 结果一致，见 normalize_chinese_mod_keyword）。
//! 4. mcmod HTML 解析的正则反向引用/前瞻（Rust regex 不支持）→ 手写扫描器，匹配规则逐条保留。
//! 5. communitySearchPage 的 sourceCountReads 修订号竞态防护（并发页防陈旧快照）在同步线程
//!    模型下不会出现，保留 60s sourceCounts 缓存与「告警不入缓存」规则。
//! 6. kind=modpack 的后台安装走 [`set_modpack_install_hook`] 接线（modpacks.rs 移植后由
//!    集成方注册）；未注册时下载完成后即视为任务失败收尾并提示未接入。
//! 7. 元数据/搜索类 HTTP 复用 crate::net::client()（与原版 fetch/undici 共享 Agent 等价）。

use crate::download::{self, DownloadFileOpts, SmoothedSpeedEstimator};
use crate::installer::{self, ProgressGuard};
use crate::net;
use crate::tasks::{self, TaskRecord};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

const UA_SEARCH: &str = "WUHUICL/0.1.0";
const TIMEOUT_MS: u64 = 30_000;

// ---------------- 基础请求（community.ts fetchJson） ----------------

fn fetch_json(url: &str, headers: &[(&str, &str)], timeout_ms: u64) -> Result<Value, String> {
    let mut req = net::client().get(url).timeout(Duration::from_millis(timeout_ms));
    req = req.header("User-Agent", UA_SEARCH);
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let resp = req.send().map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(format!("HTTP {status}: {url}"));
    }
    resp.json::<Value>().map_err(|e| e.to_string())
}

/// x-www-form-urlencoded 百分号编码（URLSearchParams 等价；空格以 %20 传输，服务端等价解析）
pub fn enc(s: &str) -> String {
    let mut out = String::new();
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'*' => out.push(*b as char),
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn query_string(pairs: &[(&str, String)]) -> String {
    pairs.iter().map(|(k, v)| format!("{}={}", enc(k), enc(v))).collect::<Vec<_>>().join("&")
}

fn query_string_owned(pairs: &[(String, String)]) -> String {
    pairs.iter().map(|(k, v)| format!("{}={}", enc(k), enc(v))).collect::<Vec<_>>().join("&")
}

fn arg(args: &[Value], i: usize) -> Option<&Value> {
    args.get(i)
}

fn arg_str(args: &[Value], i: usize) -> Option<&str> {
    args.get(i).and_then(|v| v.as_str())
}

// ---------------- modCompatibility（shared/modCompatibility.ts，mods.rs 复用） ----------------

static LOADER_MAP: OnceLock<HashMap<String, &'static str>> = OnceLock::new();
fn loader_map() -> &'static HashMap<String, &'static str> {
    LOADER_MAP.get_or_init(|| {
        [
            ("neoforge", "neoforge"),
            ("neoforged", "neoforge"),
            ("forge", "forge"),
            ("fabric", "fabric"),
            ("fabricloader", "fabric"),
            ("quilt", "quilt"),
            ("quiltloader", "quilt"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect()
    })
}

/// normalizeLoader()
pub fn normalize_loader(value: Option<&str>) -> Option<&'static str> {
    let key: String = value.unwrap_or("").trim().to_lowercase().chars().filter(|c| !matches!(c, '-' | '_' | ' ')).collect();
    loader_map().get(&key).copied()
}

/// normalizeLoaderVersion()
pub fn normalize_loader_version(value: &str, loader: Option<&str>, mc: Option<&str>) -> String {
    static PREFIX: OnceLock<regex::Regex> = OnceLock::new();
    let re = PREFIX.get_or_init(|| regex::Regex::new(r"^(?:neoforge|forge|fabric(?:-loader)?|quilt(?:-loader)?)[\s:_-]+").unwrap());
    let mut version = re.replace(value.trim(), "").to_string();
    // /^v(?=\d)/i：仅在前缀 v 后跟数字时剥掉
    if version.len() > 1
        && matches!(version.as_bytes()[0], b'v' | b'V')
        && version[1..].chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false)
    {
        version = version[1..].to_string();
    }
    if loader == Some("forge") {
        if let Some(mc) = mc {
            if version.starts_with(&format!("{mc}-")) {
                version = version[mc.len() + 1..].to_string();
            }
        }
    }
    version
}

fn parse_semver_like(v: &str) -> Option<(Vec<u64>, Option<String>)> {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = RE.get_or_init(|| regex::Regex::new(r"^(\d+(?:\.\d+)*)(?:-([^+]*))?(?:\+.*)?$").unwrap());
    let c = re.captures(v.trim())?;
    let nums = c[1].split('.').map(|p| p.parse::<u64>().unwrap_or(0)).collect();
    Some((nums, c.get(2).map(|m| m.as_str().to_string())))
}

/// compareVersions()：数值段逐位比较；预发布段按原版规则（无预发布 > 有预发布，空预发布最早）
pub fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let (x, y) = (parse_semver_like(a), parse_semver_like(b));
    let (Some((xs, xp)), Some((ys, yp))) = (x, y) else {
        return a.cmp(b);
    };
    for i in 0..xs.len().max(ys.len()) {
        let d = xs.get(i).copied().unwrap_or(0) as i64 - ys.get(i).copied().unwrap_or(0) as i64;
        if d != 0 {
            return d.cmp(&0);
        }
    }
    match (&xp, &yp) {
        (None, None) => std::cmp::Ordering::Equal,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (Some(_), None) => std::cmp::Ordering::Less,
        (Some(p), Some(q)) => {
            if p.is_empty() || q.is_empty() {
                return match (p.is_empty(), q.is_empty()) {
                    (true, true) => std::cmp::Ordering::Equal,
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    (false, false) => std::cmp::Ordering::Equal,
                };
            }
            let xps: Vec<&str> = p.split(['.', '-']).collect();
            let yps: Vec<&str> = q.split(['.', '-']).collect();
            for i in 0..xps.len().max(yps.len()) {
                let (pv, qv) = (xps.get(i), yps.get(i));
                if pv == qv {
                    continue;
                }
                let Some(p) = pv else { return std::cmp::Ordering::Less };
                let Some(q) = qv else { return std::cmp::Ordering::Greater };
                let (pn, qn) = (p.parse::<u64>().ok(), q.parse::<u64>().ok());
                if let (Some(a), Some(b)) = (pn, qn) {
                    return a.cmp(&b);
                }
                if pn.is_some() != qn.is_some() {
                    return if pn.is_some() { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater };
                }
                return p.cmp(q);
            }
            std::cmp::Ordering::Equal
        }
    }
}

fn range_predicate(range: &str, version: &str) -> bool {
    if range == "*" {
        return true;
    }
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = RE.get_or_init(|| regex::Regex::new(r"^(>=|<=|>|<|=|~|\^)?([^\s,()[\]]+)$").unwrap());
    let Some(c) = re.captures(range) else { return false };
    let op = c.get(1).map(|m| m.as_str()).unwrap_or("=");
    let base = c[2].to_string();
    let parts: Vec<&str> = base.split('.').collect();
    let wild = parts.iter().position(|p| p.eq_ignore_ascii_case("x") || *p == "*");
    if let Some(w) = wild {
        if op != "=" {
            return false;
        }
        if !parts[w..].iter().all(|p| p.eq_ignore_ascii_case("x") || *p == "*") {
            return false;
        }
        let vparts: Vec<&str> = version.split('.').collect();
        return parts[..w].iter().enumerate().all(|(i, p)| vparts.get(i) == Some(p));
    }
    let cmp = compare_versions(version, &base);
    match op {
        "=" => cmp == std::cmp::Ordering::Equal,
        ">" => cmp == std::cmp::Ordering::Greater,
        ">=" => cmp != std::cmp::Ordering::Less,
        "<" => cmp == std::cmp::Ordering::Less,
        "<=" => cmp != std::cmp::Ordering::Greater,
        "~" | "^" => {
            static BASE_OK: OnceLock<regex::Regex> = OnceLock::new();
            let ok = BASE_OK.get_or_init(|| regex::Regex::new(r"^\d+(?:\.\d+)*(?:-[\w.-]+)?$").unwrap());
            if !ok.is_match(&base) {
                return false;
            }
            let nums_str = base.split('-').next().unwrap_or("");
            let nums: Vec<u64> = nums_str.split('.').map(|p| p.parse::<u64>().unwrap_or(0)).collect();
            let index = if op == "~" {
                nums.len().saturating_sub(1).min(1)
            } else {
                let first = nums.iter().position(|n| *n != 0).unwrap_or(nums.len().saturating_sub(1));
                first.max(0)
            };
            let mut upper: Vec<u64> = nums[..=index.min(nums.len().saturating_sub(1))].to_vec();
            if let Some(last) = upper.last_mut() {
                *last += 1;
            }
            cmp != std::cmp::Ordering::Less && compare_versions(version, &upper.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(".")) == std::cmp::Ordering::Less
        }
        _ => false,
    }
}

const HYPHEN_RANGE: &str = r"^(\d+(?:\.\d+)*)\s*-\s*(\d+(?:\.\d+)*)$";

/// matchesVersionRange()：Maven 区间并集 / Fabric-Quilt OR / AND 谓词 / CurseForge 连字符区间
pub fn matches_version_range(range: &str, version: &str) -> bool {
    let r = range.trim();
    if version.is_empty() || version == "未知" {
        return false;
    }
    if r.is_empty() || r == "*" {
        return true;
    }
    if r.contains("||") {
        return r.split("||").any(|p| !p.trim().is_empty() && matches_version_range(p, version));
    }
    if r.contains(" && ") {
        return r.split(" && ").all(|p| !p.trim().is_empty() && matches_version_range(p, version));
    }
    static HYPHEN: OnceLock<regex::Regex> = OnceLock::new();
    let hyphen = HYPHEN.get_or_init(|| regex::Regex::new(HYPHEN_RANGE).unwrap());
    if let Some(c) = hyphen.captures(r) {
        return compare_versions(version, &c[1]) != std::cmp::Ordering::Less
            && compare_versions(version, &c[2]) != std::cmp::Ordering::Greater;
    }
    if r.starts_with('[') || r.starts_with('(') {
        static INTERVAL: OnceLock<regex::Regex> = OnceLock::new();
        let re = INTERVAL.get_or_init(|| regex::Regex::new(r"[[(][^()[\]]*[)\]]").unwrap());
        let intervals: Vec<&str> = re.find_iter(r).map(|m| m.as_str()).collect();
        let joined: String = intervals.join(",").replace(' ', "");
        if joined != r.replace(' ', "") {
            return false;
        }
        let expand_bound = |value: &str, pick_lo: bool| -> String {
            if let Some(c) = hyphen.captures(value) {
                return if pick_lo { c[1].to_string() } else { c[2].to_string() };
            }
            value.to_string()
        };
        return intervals.iter().any(|interval| {
            let body = interval[1..interval.len() - 1].trim();
            if !body.contains(',') {
                return interval.starts_with('[') && interval.ends_with(']') && !body.is_empty()
                    && compare_versions(version, &expand_bound(body, true)) == std::cmp::Ordering::Equal;
            }
            let bounds: Vec<&str> = body.split(',').map(|s| s.trim()).collect();
            if bounds.len() != 2 || (bounds[0].is_empty() && bounds[1].is_empty()) {
                return false;
            }
            let lo = expand_bound(bounds[0], true);
            let hi = expand_bound(bounds[1], false);
            if !lo.is_empty() && !hi.is_empty() && compare_versions(&lo, &hi) == std::cmp::Ordering::Greater {
                return false;
            }
            let low = if lo.is_empty() { std::cmp::Ordering::Greater } else { compare_versions(version, &lo) };
            let high = if hi.is_empty() { std::cmp::Ordering::Less } else { compare_versions(version, &hi) };
            // 开区间上界「补丁延伸」宽容（如 [1.21,1.21.1) 纳入 1.21.x）
            let extends_by_patch = !lo.is_empty() && !hi.is_empty() && {
                let lp: Vec<&str> = lo.split('.').collect();
                let hp: Vec<&str> = hi.split('.').collect();
                hp.len() > lp.len() && hp[..lp.len()].join(".") == lp.join(".")
            };
            (low == std::cmp::Ordering::Greater || low == std::cmp::Ordering::Equal && interval.starts_with('['))
                && (high == std::cmp::Ordering::Less || high == std::cmp::Ordering::Equal && (interval.ends_with(']') || extends_by_patch))
        });
    }
    static OP_SPACE: OnceLock<regex::Regex> = OnceLock::new();
    let re = OP_SPACE.get_or_init(|| regex::Regex::new(r"(>=|<=|>|<|=|~|\^)\s+").unwrap());
    re.replace_all(r, "$1").split_whitespace().all(|part| range_predicate(part, version))
}

fn fabric_predicate(part: &str) -> String {
    static CARET: OnceLock<regex::Regex> = OnceLock::new();
    let re = CARET.get_or_init(|| regex::Regex::new(r"^(\^|~)(\d+(?:\.\d+)*)(?:-[^+]*)?(?:\+.*)?$").unwrap());
    if let Some(c) = re.captures(part) {
        let nums: Vec<u64> = c[2].split('.').map(|p| p.parse::<u64>().unwrap_or(0)).collect();
        let index = if &c[1] == "^" { 0 } else { nums.len().saturating_sub(1).min(1) };
        let mut upper: Vec<u64> = nums[..=index].to_vec();
        if let Some(last) = upper.last_mut() {
            *last += 1;
        }
        return format!(">={} <{}-", &part[1..], upper.iter().map(|n| n.to_string()).collect::<Vec<_>>().join("."));
    }
    static WILD: OnceLock<regex::Regex> = OnceLock::new();
    let re = WILD.get_or_init(|| regex::Regex::new(r"^(\d+(?:\.\d+)*)\.(?:x|\*)(?:\.(?:x|\*))*$").unwrap());
    if let Some(c) = re.captures(part) {
        let mut upper: Vec<u64> = c[1].split('.').map(|p| p.parse::<u64>().unwrap_or(0)).collect();
        if let Some(last) = upper.last_mut() {
            *last += 1;
        }
        return format!(">={}- <{}-", &c[1], upper.iter().map(|n| n.to_string()).collect::<Vec<_>>().join("."));
    }
    part.to_string()
}

fn range_terms(value: &Value, dialect: &str, depth: u32) -> Result<Vec<Vec<String>>, String> {
    if depth > 16 {
        return Err("range too deep".into());
    }
    match value {
        Value::String(v) => {
            let range = if dialect == "fabric" {
                static OP_SPACE: OnceLock<regex::Regex> = OnceLock::new();
                let re = OP_SPACE.get_or_init(|| regex::Regex::new(r"(>=|<=|>|<|=|~|\^)\s+").unwrap());
                re.replace_all(v.trim(), "$1").split_whitespace().map(|p| fabric_predicate(p)).collect::<Vec<_>>().join(" ")
            } else {
                v.trim().to_string()
            };
            let normalized = if dialect == "quilt" {
                static Q: OnceLock<regex::Regex> = OnceLock::new();
                let re = Q.get_or_init(|| regex::Regex::new(r"^\d+(?:\.\d+)*(?:-[\w.-]+)?(?:\+[\w.-]+)?$").unwrap());
                if re.is_match(&range) {
                    format!("^{range}")
                } else {
                    range
                }
            } else {
                range
            };
            Ok(normalized.split("||").map(|part| part.trim().split(" && ").map(str::to_string).collect()).collect())
        }
        Value::Null => Ok(vec![vec!["*".to_string()]]),
        Value::Array(any) => {
            if any.is_empty() || any.len() > 128 {
                return Err("invalid alternatives".into());
            }
            let mut result: Vec<Vec<String>> = Vec::new();
            for part in any {
                result.extend(range_terms(part, dialect, depth + 1)?);
            }
            if result.len() > 128 {
                return Err("range too large".into());
            }
            Ok(result)
        }
        Value::Object(obj) => {
            // any 优先于 all（与原版 terms() 判定顺序一致；any 数组长度非法直接报错）
            if let Some(Value::Array(any)) = obj.get("any") {
                if any.is_empty() || any.len() > 128 {
                    return Err("invalid alternatives".into());
                }
                let mut result: Vec<Vec<String>> = Vec::new();
                for part in any {
                    result.extend(range_terms(part, dialect, depth + 1)?);
                }
                if result.len() > 128 {
                    return Err("range too large".into());
                }
                return Ok(result);
            }
            if let Some(Value::Array(all)) = obj.get("all") {
                if !all.is_empty() && all.len() <= 128 {
                    let mut result: Vec<Vec<String>> = vec![vec![]];
                    for part in all {
                        let next = range_terms(part, dialect, depth + 1)?;
                        let mut merged: Vec<Vec<String>> = Vec::new();
                        for a in &result {
                            for b in &next {
                                let mut combined = a.clone();
                                combined.extend(b.clone());
                                merged.push(combined);
                            }
                        }
                        result = merged;
                        if result.len() > 128 {
                            return Err("range too large".into());
                        }
                    }
                    return Ok(result);
                }
            }
            Err("unsupported range".into())
        }
        _ => Err("unsupported range".into()),
    }
}

/// dependencyRange()：Fabric/Quilt 依赖声明 → Maven 风格范围串
pub fn dependency_range(value: &Value, dialect: &str) -> String {
    if value.is_null() {
        return String::new();
    }
    match range_terms(value, dialect, 0) {
        Ok(terms) => terms.iter().map(|term| term.join(" && ")).collect::<Vec<_>>().join(" || "),
        Err(_) => "!unsupported-range!".to_string(),
    }
}

/// modMatchesInstance()：mod 元数据（含 variants）与实例的兼容判定
pub fn mod_matches_instance(mod_info: &Value, instance: &Value) -> bool {
    let loader = normalize_loader(instance.get("loader").and_then(|v| v.as_str()));
    if mod_info.get("error").is_some()
        || instance.get("failed").and_then(|v| v.as_bool()).unwrap_or(false)
        || instance.get("incomplete").and_then(|v| v.as_bool()).unwrap_or(false)
        || loader.is_none()
    {
        return false;
    }
    let variants = mod_info.get("variants").and_then(|v| v.as_array()).filter(|a| !a.is_empty());
    let specs: Vec<&Value> = match variants {
        Some(list) => list.iter().filter(|s| loader == normalize_loader(s.get("loader").and_then(|v| v.as_str()))).collect(),
        None => vec![mod_info],
    };
    if specs.is_empty() {
        return false;
    }
    let mc = instance.get("mcVersion").and_then(|v| v.as_str()).unwrap_or("");
    let lv = instance.get("loaderVersion").and_then(|v| v.as_str()).unwrap_or("");
    specs.iter().all(|spec| {
        matches_version_range(spec.get("mcRange").and_then(|v| v.as_str()).unwrap_or(""), mc)
            && spec.get("loaderRange").and_then(|v| v.as_str()).map(|lr| {
                lr.is_empty() || matches_version_range(lr, &normalize_loader_version(lv, loader, Some(mc)))
            }).unwrap_or(true)
    })
}

/// modMismatchReasons()：与安装同一套语义检查的解释文案
pub fn mod_mismatch_reasons(mod_info: &Value, instance: &Value) -> Vec<String> {
    if let Some(err) = mod_info.get("error").and_then(|v| v.as_str()) {
        return vec![err.to_string()];
    }
    if instance.get("failed").and_then(|v| v.as_bool()).unwrap_or(false)
        || instance.get("incomplete").and_then(|v| v.as_bool()).unwrap_or(false)
    {
        return vec!["实例安装不完整或元数据无法读取".to_string()];
    }
    let loader = normalize_loader(instance.get("loader").and_then(|v| v.as_str()));
    let variants = mod_info.get("variants").and_then(|v| v.as_array()).filter(|a| !a.is_empty());
    let specs: Vec<&Value> = match variants {
        Some(list) => list.iter().filter(|s| loader.is_some() && loader == normalize_loader(s.get("loader").and_then(|v| v.as_str()))).collect(),
        None => vec![mod_info],
    };
    if specs.is_empty() {
        let declared = mod_info.get("loader").and_then(|v| v.as_str()).unwrap_or("已声明的加载器");
        return vec![format!("需要 {declared}，实例为 {}", loader.unwrap_or("纯净版/未知"))];
    }
    let mc = instance.get("mcVersion").and_then(|v| v.as_str()).unwrap_or("");
    let lv = instance.get("loaderVersion").and_then(|v| v.as_str()).unwrap_or("");
    let mut reasons: Vec<String> = Vec::new();
    for spec in specs {
        let mc_range = spec.get("mcRange").and_then(|v| v.as_str()).unwrap_or("");
        if !matches_version_range(mc_range, mc) {
            reasons.push(format!("Minecraft {mc} 不满足 {mc_range}（方括号含边界，圆括号不含边界）"));
        }
        if let Some(lr) = spec.get("loaderRange").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
            if !matches_version_range(lr, &normalize_loader_version(lv, loader, Some(mc))) {
                reasons.push(format!("{} {} 不满足 {}", loader.unwrap_or(""), if lv.is_empty() { "版本未知" } else { lv }, lr));
            }
        }
    }
    let mut seen = HashSet::new();
    reasons.retain(|r| seen.insert(r.clone()));
    reasons
}

// ---------------- communityPolicy（shared/communityPolicy.ts） ----------------

/// MODRINTH_RESOURCE_LOADERS
pub fn modrinth_resource_loaders(kind: &str) -> Option<&'static [&'static str]> {
    match kind {
        "resourcepack" => Some(&["minecraft"]),
        "shader" => Some(&["iris", "optifine", "canvas", "vanilla"]),
        "datapack" => Some(&["datapack"]),
        _ => None,
    }
}

/// usesCommunityLoader()
pub fn uses_community_loader(kind: Option<&str>) -> bool {
    match kind {
        None | Some("mod") | Some("modpack") => true,
        _ => false,
    }
}

/// CommunityFileFilter（mcVersion / loader / kind）
#[derive(Clone, Default, Debug)]
pub struct FileFilter {
    pub mc_version: Option<String>,
    pub loader: Option<String>,
    pub kind: Option<String>,
}

/// effectiveCommunityFilter()
pub fn effective_filter(filter: &FileFilter) -> FileFilter {
    let mc = filter.mc_version.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    let loader = if uses_community_loader(filter.kind.as_deref()) {
        filter.loader.as_deref().filter(|s| !s.is_empty()).map(str::to_string)
    } else {
        None
    };
    FileFilter { mc_version: mc, loader, kind: filter.kind.clone() }
}

pub fn filter_from_value(v: Option<&Value>) -> FileFilter {
    let v = v.filter(|v| v.is_object());
    FileFilter {
        mc_version: v.and_then(|v| v.get("mcVersion")).and_then(|x| x.as_str()).map(str::to_string),
        loader: v.and_then(|v| v.get("loader")).and_then(|x| x.as_str()).map(str::to_string),
        kind: v.and_then(|v| v.get("kind")).and_then(|x| x.as_str()).map(str::to_string),
    }
}

/// matchesCommunityFilter()
pub fn matches_community_filter(file: &Value, filter: &FileFilter) -> bool {
    let filter = effective_filter(filter);
    let resource_loaders = filter.kind.as_deref().and_then(modrinth_resource_loaders);
    if file.get("source").and_then(|v| v.as_str()) == Some("modrinth") {
        if let Some(rl) = resource_loaders {
            let has = file.get("loaders").and_then(|v| v.as_array()).map(|a| a.iter().any(|l| rl.contains(&l.as_str().unwrap_or("")))).unwrap_or(false);
            if !has {
                return false;
            }
        }
    }
    let mc_ok = match &filter.mc_version {
        None => true,
        Some(mc) => file.get("gameVersions").and_then(|v| v.as_array()).map(|a| {
            a.iter().any(|v| v.as_str().map(|s| compare_versions(s, mc) == std::cmp::Ordering::Equal).unwrap_or(false))
        }).unwrap_or(false),
    };
    if !mc_ok {
        return false;
    }
    match &filter.loader {
        None => true,
        Some(loader) => {
            let want = normalize_loader(Some(loader.as_str()));
            file.get("loaders").and_then(|v| v.as_array()).map(|a| {
                a.iter().any(|l| normalize_loader(l.as_str()) == want)
            }).unwrap_or(false)
        }
    }
}

/// communityFileMatchesInstance()
pub fn community_file_matches_instance(file: &Value, instance: &Value) -> bool {
    !instance.get("failed").and_then(|v| v.as_bool()).unwrap_or(false)
        && !instance.get("incomplete").and_then(|v| v.as_bool()).unwrap_or(false)
        && instance.get("loader").and_then(|v| v.as_str()).map(|s| !s.is_empty()).unwrap_or(false)
        && matches_community_filter(
            file,
            &FileFilter {
                mc_version: instance.get("mcVersion").and_then(|v| v.as_str()).map(str::to_string),
                loader: instance.get("loader").and_then(|v| v.as_str()).map(str::to_string),
                kind: None,
            },
        )
}

// ---------------- community-zh.ts：热门 MOD 中文名映射 ----------------

pub const MOD_ZH: &[(&str, &str)] = &[
    ("sodium", "钠"), ("sodium-extra", "钠·扩展"), ("iris", "鸢尾光影"), ("oculus", "Oculus 光影"),
    ("lithium", "锂"), ("starlight", "星光"), ("ferritecore", "铁氧体磁芯"), ("modernfix", "现代化修复"),
    ("memoryleakfix", "内存泄漏修复"), ("entityculling", "实体剔除"), ("moreculling", "更多剔除"),
    ("c2me-fabric", "C2ME 区块引擎"), ("noisium", "噪声优化"), ("cesium", "铯"), ("dynamic-fps", "动态FPS"),
    ("immediatelyfast", "即时渲染"), ("fastbench", "快速工作台"), ("fast-furnace", "快速熔炉"),
    ("clumps", "经验团合并"), ("get-it-together-drops", "掉落物合并"), ("krypton", "氪"),
    ("async-locator", "异步定位"), ("alternate-current", "交流电"), ("enhanced-block-entities", "增强方块实体"),
    ("entity-model-features", "实体模型特性"), ("entity-texture-features", "实体纹理特性"),
    ("distant-horizons", "遥远地平线"), ("bobby", "区块缓存 Bobby"), ("farsight", "远视"),
    ("vulkanmod", "Vulkan 渲染"), ("nvidium", "Nvidium"), ("spark", "Spark 性能分析"),
    ("observable", "性能观测"), ("cull-less-leaves", "树叶剔除优化"), ("smooth-chunk", "平滑区块加载"),
    ("threadtweak", "线程调整"), ("packet-fixer", "数据包修复"), ("connectivity", "连接优化"),
    ("badoptimizations", "BadOptimizations 优化"), ("very-many-players", "超多玩家优化"),
    ("jei", "JEI物品管理器"), ("rei", "REI 物品管理"), ("emi", "EMI 物品管理"),
    ("just-enough-resources", "JER 资源查询"), ("just-enough-professions", "JEP 职业查询"),
    ("wthit", "WTHIT 高亮显示"), ("waila", "WAILA 高亮显示"), ("jade", "Jade 高亮显示"),
    ("the-one-probe", "TOP 高亮显示"), ("modmenu", "模组菜单"), ("cloth-config", "Cloth 配置库"),
    ("configured", "配置界面"), ("catalogue", "模组目录"), ("betterf3", "更好的F3"),
    ("equipment-compare", "装备对比"), ("item-filters", "物品过滤器"), ("durability-tooltip", "耐久提示"),
    ("durability-viewer", "耐久查看"), ("enchantment-descriptions", "附魔描述"),
    ("shulker-box-tooltip", "潜影盒提示"), ("inventory-sorter", "物品栏整理"), ("mouse-tweaks", "鼠标手势"),
    ("crafting-tweaks", "合成辅助"), ("polymorph", "多态合成"), ("toast-control", "通知控制"),
    ("chat-heads", "聊天头像"), ("emojiful", "聊天表情"), ("no-chat-reports", "禁用聊天举报"),
    ("screenshots-to-clipboard", "截图到剪贴板"), ("edit-sign", "编辑告示牌"), ("freecam", "自由相机"),
    ("better-third-person", "更好的第三人称"), ("first-person-model", "第一人称模型"),
    ("not-enough-animations", "更多动画"), ("eating-animation", "进食动画"), ("swingthrough", "挥砍穿草"),
    ("skin-layers-3d", "3D 皮肤层"), ("customskinloader", "万用皮肤补丁"), ("emotecraft", "表情工艺"),
    ("gamma-utils", "伽马工具"), ("advancement-plaques", "进度牌匾"), ("better-advancements", "更好的进度"),
    ("trading-post", "交易站"), ("visual-workbench", "可视化工作台"), ("neat", "生物血条"),
    ("toast", "通知美化"), ("tips", "提示"),
    ("fabric-api", "Fabric API"), ("forge-config-api-port", "Forge 配置移植"),
    ("architectury-api", "Architectury"), ("puzzles-lib", "Puzzles Lib"), ("bookshelf", "Bookshelf 前置"),
    ("balm", "Balm 前置"), ("collective", "Collective 前置"), ("moonlight", "月光 Selene"),
    ("geckolib", "GeckoLib 动画库"), ("azurelib", "AzureLib"), ("playeranimator", "玩家动画库"),
    ("curios", "饰品栏"), ("trinkets", "饰品 Trinkets"), ("flywheel", "飞轮 Flywheel"),
    ("kotlin-for-forge", "Kotlin for Forge"), ("bclib", "BCLib"), ("terrablender", "地形混合器"),
    ("corgilib", "柯基库"), ("lithostitched", "缝合地形"), ("resourceful-lib", "Resourceful 库"),
    ("resourceful-config", "Resourceful 配置"), ("botarium", "Botarium"), ("team-reactor", "Reactor 库"),
    ("cupboard", "橱柜 Cupboard"), ("sheti-phian-core", "ShetiPhian 核心"), ("cofh-core", "CoFH 核心"),
    ("titanium", "钛 Titanium"), ("mcjtylib", "McJty 库"), ("obscure-api", "Obscure API"),
    ("attributeslib", "属性库"), ("placebo", "安慰剂 Placebo"), ("supermartijn642s-core-lib", "超级马丁核心库"),
    ("valhelsia-core", "Valhelsia 核心"),
    ("create", "机械动力"), ("create-fabric", "机械动力（Fabric）"), ("createaddition", "机械动力：附加"),
    ("create-stuff-additions", "机械动力附属"), ("steam-rails", "蒸汽铁轨"), ("farmers-delight", "农夫乐事"),
    ("croptopia", "丰收物语"), ("supplementaries", "锦致装饰"), ("ftb-quests", "FTB 任务"),
    ("ftb-teams", "FTB 团队"), ("ftb-library", "FTB 基础库"), ("ftb-chunks", "FTB 区块认领"),
    ("ftb-ultimine", "FTB 连锁挖矿"), ("ftb-backups", "FTB 备份"), ("ftb-essentials", "FTB 基础"),
    ("indium", "铟"), ("carry-on", "搬运"), ("appleskin", "苹果皮"), ("journeymap", "旅行地图"),
    ("xaeros-minimap", "Xaero的小地图"), ("xaeros-world-map", "Xaero的世界地图"), ("waystones", "传送石"),
    ("litematica", "投影"), ("worldedit", "创世神"), ("building-gadgets", "建筑小帮手"),
    ("chunk-loaders", "区块加载器"), ("terralith", "特拉石"), ("biomesoplenty", "超多生物群系"),
    ("oh-the-biomes-youll-go", "你将去的生物群系"), ("regions-unexplored", "未探索区域"),
    ("tectonic", "构造地形"), ("reterraforged", "重构地形"), ("continents", "大陆"),
    ("twilightforest", "暮色森林"), ("aether", "天境（以太）"), ("deep-aether", "深邃以太"),
    ("aether-redux", "以太重制"), ("ancient-aether", "远古以太"), ("blue-skies", "蔚蓝天空"),
    ("the-bumblezone", "大黄蜂领域"), ("the-undergarden", "地下花园"), ("better-end", "更好的末地"),
    ("better-nether", "更好的下界"), ("outer-end", "外域末地"), ("enderscape", "末地景观"),
    ("nullscape", "虚无景观"), ("starmute", "星寂"), ("quark", "夸克"), ("charm", "魅力 Charm"),
    ("mrcrayfishs-furniture", "蜡笔小新家具"), ("another-furniture", "另一个家具"),
    ("hand-crafted", "手工制作"), ("framed", "框架方块"), ("mcw-doors", "马克门窗"),
    ("mcw-windows", "马克窗户"), ("mcw-fences", "马克栅栏"), ("mcw-roofs", "马克屋顶"),
    ("mcw-bridges", "马克桥梁"), ("mcw-furniture", "马克家具"), ("decorative-blocks", "装饰方块"),
    ("chipped", "凿刻"), ("rechiseled", "再凿刻"), ("every-compat", "通用木制品兼容"),
    ("woodworks", "木工作品"), ("connected-glass", "连接玻璃"), ("tinted-glass", "染色玻璃"),
    ("mekanism", "通用机械"), ("applied-energistics-2", "应用能源2"), ("refined-storage", "精致存储"),
    ("extra-storage", "额外存储"), ("thermal-expansion", "热力膨胀"), ("thermal-foundation", "热力基础"),
    ("thermal-dynamics", "热力管道"), ("thermal-innovation", "热力革新"), ("thermal-cultivation", "热力农业"),
    ("thermal-locomotion", "热力运输"), ("redstone-arsenal", "红石兵工厂"), ("enderio", "末影接口"),
    ("industrial-foregoing", "工业先锋"), ("pneumaticcraft", "气动工艺"), ("immersive-engineering", "沉浸工程"),
    ("tinkers-construct", "匠魂"), ("botania", "植物魔法"), ("blood-magic", "血魔法"),
    ("powah", "电力满满 Powah"), ("big-reactors", "大反应堆"), ("extreme-reactors", "极限反应堆"),
    ("nuclearcraft", "核工艺"), ("draconic-evolution", "龙之进化"), ("projecte", "等价交换"),
    ("pipez", "管道 Pipez"), ("pretty-pipes", "精美管道"), ("laserio", "激光IO"),
    ("modular-routers", "模块化路由器"), ("xnet", "XNet 网络"), ("flux-networks", "通量网络"),
    ("cable-facades", "线缆伪装"), ("little-logistics", "小小物流"), ("rftools-base", "RF工具基础"),
    ("rftools-utility", "RF工具实用"), ("rftools-dimensions", "RF工具维度"), ("compact-machines", "微型空间"),
    ("gregtech", "格雷科技"), ("gtceu", "格雷科技现代版"), ("tech-reborn", "科技重生"),
    ("modern-industrialization", "现代工业化"), ("sophisticated-backpacks", "精妙背包"),
    ("sophisticated-storage", "精妙存储"), ("sophisticated-core", "精妙核心"), ("iron-chests", "更多箱子"),
    ("storage-drawers", "储物抽屉"), ("functional-storage", "功能性存储"), ("pedestals", "基座"),
    ("travelers-backpack", "旅行者背包"), ("backpacked", "背包客"), ("inmis", "更多背包"),
    ("packed-up", "打包背包"), ("useful-backpacks", "实用背包"), ("simple-voice-chat", "简单语音聊天"),
    ("open-parties-and-claims", "开放队伍与认领"), ("easy-villagers", "简单村民"),
    ("easy-piglins", "简单猪灵"), ("villager-names", "村民命名"), ("guard-villagers", "村民警卫"),
    ("more-villagers", "更多村民"), ("minecraft-comes-alive", "凡家物语"), ("minecolonies", "模拟殖民地"),
    ("structurize", "结构化"), ("lootr", "Lootr 战利品箱"), ("corpse", "尸体 Corpse"),
    ("gravestone", "墓碑"), ("artifacts", "奇珍异宝"), ("relics", "遗物 Relics"),
    ("enigmatic-legacy", "神秘遗产"), ("apotheosis", "神化"), ("patchouli", "帕秋莉手册"),
    ("ars-nouveau", "新生魔艺"), ("ars-elemental", "元素魔艺"), ("mana-and-artifice", "魔法艺术"),
    ("mahou-tsukai", "魔法使"), ("occultism", "神秘学"), ("irons-spellbooks", "铁之法术书"),
    ("natures-aura", "自然灵气"), ("astral-sorcery", "星辉魔法"), ("elementalcraft", "元素工艺"),
    ("forbidden-arcanus", "禁忌与奥秘"), ("evilcraft", "邪恶工艺"), ("vampirism", "吸血鬼"),
    ("werewolves", "狼人"), ("ice-and-fire", "冰与火之歌"), ("wyrmroost", "龙栖"),
    ("alexsmobs", "亚历克斯的生物"), ("mowziesmobs", "Mowzie的生物"), ("cataclysm", "大灾变"),
    ("deeperdarker", "深邃黑暗"), ("galosphere", "钟乳石洞"), ("creeper-overhaul", "苦力怕革新"),
    ("mutant-more", "突变生物"), ("naturalist", "博物学家"), ("critters-and-crawlers", "小动物与爬虫"),
    ("untamedwilds", "野性未驯"), ("exotic-birds", "珍禽"), ("friends-and-foes", "友与敌"),
    ("illage-and-spillage", "劫掠与蹂躏"), ("rotten-creatures", "腐尸生物"), ("graveyard", "墓地"),
    ("nyfsspiders", "Nyf的蜘蛛"), ("cave-dweller", "洞穴居民"), ("the-mimicer", "模仿者"),
    ("siren-head", "警笛头"), ("goat-man", "山羊人"), ("the-knocker", "敲门者"),
    ("herobrine-lives", "HIM复活"), ("mystical-agriculture", "神秘农业"), ("resourceful-bees", "资源蜜蜂"),
    ("productive-bees", "生产蜜蜂"), ("botany-pots", "植物盆栽"), ("botany-trees", "盆栽树木"),
    ("aquaculture", "水产养殖"), ("aquamirae", "深海幻影"), ("go-fish", "去钓鱼"),
    ("fishing-real", "真实钓鱼"), ("tide", "潮汐"), ("stardew-fishing", "星露谷钓鱼"),
    ("upgrade-aquatic", "碧海新生"), ("atmospheric", "悠然一派"), ("autumnity", "秋原"),
    ("environmental", "自然环境"), ("neapolitan", "那不勒斯风味"), ("savage-and-ravage", "野蛮与蹂躏"),
    ("upgraded-netherite", "下界合金升级"), ("endergetic", "末地拓展"), ("repurposed-structures", "结构再利用"),
    ("dungeon-crawl", "地牢爬行"), ("lost-cities", "失落城市"), ("sky-villages", "天空村庄"),
    ("towers-of-the-wild", "荒野之塔"), ("yungs-api", "Yung 的 API"), ("yungs-better-dungeons", "更好的地牢（Yung）"),
    ("yungs-better-mineshafts", "更好的矿井（Yung）"), ("yungs-better-strongholds", "更好的要塞（Yung）"),
    ("yungs-better-ocean-monuments", "更好的海底神殿（Yung）"), ("yungs-better-desert-temples", "更好的沙漠神殿（Yung）"),
    ("yungs-better-jungle-temples", "更好的丛林神庙（Yung）"), ("yungs-better-witch-huts", "更好的女巫小屋（Yung）"),
    ("awesome-dungeon", "Awesome 地牢"), ("spirit", "灵魂 Spirit"), ("simply-swords", "简单剑"),
    ("better-combat", "更好的战斗"), ("spartan-weaponry", "斯巴达武器"), ("spartan-shields", "斯巴达盾牌"),
    ("tetra", "Tetra 工匠"), ("silent-gear", "寂静装备"), ("silent-lib", "寂静库"),
    ("paragliders", "滑翔伞"), ("wall-jump", "蹬墙跳"), ("crawl", "爬行"), ("go-prone", "匍匐"),
    ("elytra-slot", "鞘翅槽"), ("backslot", "背部槽"), ("icarus", "伊卡洛斯之翼"),
    ("dragon-mounts-legacy", "龙坐骑遗产"), ("mythic-mounts", "神话坐骑"), ("dragon-survival", "龙之生存"),
    ("l2-hostility", "L2 敌意"), ("l2-complements", "L2 补充"), ("l2-library", "L2 库"),
    ("mine-and-slash", "挖矿与砍杀"), ("age-of-exile", "流放时代"), ("divine-rpg", "神圣RPG"),
    ("aoa3", "虚无世界"), ("cobblemon", "方块宝可梦"), ("pixelmon", "像素精灵宝可梦"),
    ("pokecube", "宝可梦方块"), ("digimobs", "数码宝贝"), ("galacticraft", "星系"),
    ("ad-astra", "星际 Ad Astra"), ("beyond-earth", "超越地球"), ("minecraft-transit-railway", "我的世界铁路"),
    ("immersive-railroading", "沉浸铁路"), ("ultimate-car-mod", "终极汽车"), ("immersive-aircraft", "沉浸飞机"),
    ("simple-planes", "简单飞机"), ("man-of-many-planes", "多飞机"), ("valkyrien-skies", "女武神之空"),
    ("clockwork", "发条 Clockwork"), ("eureka-ships", "尤里卡之船"), ("da-vincis-vessels", "达芬奇之船"),
    ("small-boats", "小船"), ("ex-nihilo", "无中生有"), ("sky-orchards", "天空果园"),
    ("stoneblock", "石头世界"), ("chunky", "区块预生成"), ("world-pre-generator", "世界预生成"),
    ("default-options", "默认选项"), ("default-world-type", "默认世界类型"), ("global-gamerules", "全局游戏规则"),
    ("enhanced-celestials", "增强天体"), ("blood-moon", "血月"), ("serene-seasons", "静谧四季"),
    ("tough-as-nails", "意志坚定"), ("thirst", "口渴"), ("dehydration", "脱水"),
    ("weather-storms-tornadoes", "天气风暴龙卷风"), ("hardcore-darkness", "硬核黑暗"),
    ("true-darkness", "真实黑暗"), ("nyctophobia", "黑夜恐惧"), ("survival-plus", "生存增强"),
    ("first-aid", "急救"), ("zombie-apocalypse", "僵尸启示录"), ("zombie-awareness", "僵尸警觉"),
    ("zombie-invasion", "僵尸入侵"), ("the-hordes", "尸潮"), ("security-craft", "安全工艺"),
    ("locks", "锁 Locks"), ("exposure", "曝光 Exposure"), ("joy-of-painting", "绘画之乐"),
    ("table-top-craft", "桌面游戏"), ("fantasy-furniture", "梦幻家具"), ("fantasy-dice", "梦幻骰子"),
    ("bingo", "宾果 Bingo"), ("mine-cards", "我的卡牌"), ("murder-mystery", "谋杀之谜"),
    ("hide-and-seek", "捉迷藏"), ("imposter", "内鬼 Imposter"), ("immersive-portals", "沉浸传送门"),
    ("dimension-doors", "维度之门"), ("pocket-dimensions", "口袋维度"), ("abyssalcraft", "深渊国度"),
    ("the-midnight", "午夜"), ("the-betweenlands", "交错次元"), ("the-erebus", "埃里伯斯"),
    ("atum", "阿图姆"), ("voidscape", "虚空之境"), ("ratlantis", "老鼠兰蒂斯"),
    ("fossils-and-archeology", "化石与考古"), ("jurassi-craft", "侏罗纪工艺"), ("rediscovered", "重新发现"),
    ("unearthed", "出土文物"), ("kubejs", "KubeJS"), ("crafttweaker", "CraftTweaker"),
    ("lootjs", "LootJS"), ("probejs", "ProbeJS"), ("contenttweaker", "内容调整"), ("jaopca", "JAOPCA"),
    ("emendatus-enigmatica", "神秘统一 EE"), ("almost-unified", "近乎统一"),
    ("allthemodium", "全能矿 Allthemodium"), ("alltheores", "全矿物 AllTheOres"),
    ("unobtainium", "不可得矿"), ("vibranium", "振金"), ("matter-overdrive", "物质超驱动"),
    ("sync", "克隆同步 Sync"), ("cyberware", "赛博组件"), ("presence-footsteps", "临场脚步声"),
    ("sound-physics-remastered", "声音物理"), ("ambient-sounds", "环境音效"),
    ("dynamic-surroundings", "动态环境"), ("better-foliage", "更好的树叶"), ("particle-rain", "粒子雨"),
    ("effective", "水花特效 Effective"), ("fancy-block-particles", "Fancy 方块粒子"),
    ("auroras", "极光"), ("better-clouds", "更好的云"), ("simple-clouds", "简单云"),
    ("nocubes", "平滑地形 NoCubes"),
];

/// MOD_ZH_ALIASES
pub const MOD_ZH_ALIASES: &[(&str, &[&str])] = &[
    ("玉", &["jade"]),
    ("物品管理器", &["jei", "rei", "emi"]),
    ("物品查询", &["jei", "rei", "emi"]),
    ("机械动力前置", &["flywheel"]),
    ("小地图", &["xaeros-minimap", "journeymap"]),
    ("大地图", &["xaeros-world-map", "journeymap"]),
    ("暮色", &["twilightforest"]),
    ("万用皮肤", &["customskinloader"]),
    ("大气", &["atmospheric"]),
    ("秋意", &["autumnity"]),
    ("升级水域", &["upgrade-aquatic"]),
    ("末地扩展", &["endergetic"]),
    ("静谧季节", &["serene-seasons"]),
    ("季节", &["serene-seasons"]),
    ("静谧四季季节", &["serene-seasons"]),
    ("静谧四季/季节", &["serene-seasons"]),
];

fn zh_to_slugs() -> &'static HashMap<String, Vec<String>> {
    static M: OnceLock<HashMap<String, Vec<String>>> = OnceLock::new();
    M.get_or_init(|| {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        for (slug, zh) in MOD_ZH {
            map.entry(zh.to_lowercase()).or_default().push(slug.to_string());
        }
        map
    })
}

fn mod_zh_lookup(slug: &str) -> Option<&'static str> {
    // 少量条目，线性查找即可（也可 OnceLock HashMap，此处保持零拷贝）
    MOD_ZH.iter().find(|(s, _)| *s == slug).map(|(_, zh)| *zh)
}

fn nfkc_char(c: char) -> char {
    // 等价 NFKC 子集：全角 ASCII/标点折叠 + 常用映射（中文关键词场景与 NFKC 结果一致）
    let u = c as u32;
    if (0xFF01..=0xFF5E).contains(&u) {
        return char::from_u32(u - 0xFEE0).unwrap_or(c);
    }
    if u == 0x3000 {
        return ' ';
    }
    c
}

/// normalizeChineseModKeyword()
pub fn normalize_chinese_mod_keyword(value: &str) -> String {
    let mut s: String = value.chars().map(nfkc_char).collect();
    s = s.to_lowercase();
    s.retain(|c| !c.is_whitespace());
    let lower = s.to_lowercase();
    if lower.ends_with("模组") {
        s.truncate(s.len() - "模组".len());
    } else if lower.ends_with("模組") {
        s.truncate(s.len() - "模組".len());
    } else if s.len() >= 3 && s.is_char_boundary(s.len() - 3) && s[s.len() - 3..].eq_ignore_ascii_case("mod") {
        s.truncate(s.len() - 3);
    }
    s.retain(|c| !matches!(c, '·' | '：' | ':' | '(' | ')' | '（' | '）'));
    s
}

fn has_cjk(s: &str) -> bool {
    s.chars().any(|c| ('\u{4E00}'..='\u{9FFF}').contains(&c))
}

/// hasExactChineseModName()
pub fn has_exact_chinese_mod_name(keyword: &str) -> bool {
    let key = normalize_chinese_mod_keyword(keyword);
    if key.is_empty() {
        return false;
    }
    MOD_ZH_ALIASES.iter().any(|(name, _)| normalize_chinese_mod_keyword(name) == key)
        || zh_to_slugs().keys().any(|name| normalize_chinese_mod_keyword(name) == key)
}

/// chineseModSearchTerms()：精确名优先，子串命中按名称长度升序，最多 5 个 slug
pub fn chinese_mod_search_terms(keyword: &str) -> Vec<String> {
    if !has_cjk(keyword) {
        return Vec::new();
    }
    let key = normalize_chinese_mod_keyword(keyword);
    if key.is_empty() {
        return Vec::new();
    }
    let mut entries: Vec<(String, Vec<String>)> = MOD_ZH_ALIASES
        .iter()
        .map(|(name, slugs)| (name.to_string(), slugs.iter().map(|s| s.to_string()).collect()))
        .collect();
    for (name, slugs) in zh_to_slugs() {
        entries.push((name.clone(), slugs.clone()));
    }
    let exact: Vec<_> = entries.iter().filter(|(name, _)| normalize_chinese_mod_keyword(name) == key).collect();
    let candidates = if !exact.is_empty() {
        exact
    } else {
        let mut sub: Vec<_> = entries.iter().filter(|(name, _)| normalize_chinese_mod_keyword(name).contains(&key)).collect();
        sub.sort_by_key(|(name, _)| name.len());
        sub
    };
    let mut out: Vec<String> = Vec::new();
    for (_, slugs) in candidates {
        for slug in slugs {
            if !out.contains(slug) {
                out.push(slug.clone());
            }
        }
    }
    out.truncate(5);
    out
}

// ---------------- mcmodSearch.ts：MC 百科检索（HTML 抓取，仅取标题与明确外链） ----------------

const MCMOD_MAX_BYTES: usize = 2 * 1024 * 1024;
const MCMOD_MAX_ENTRIES: usize = 5;

#[derive(Clone, Debug)]
pub struct McmodProjectLink {
    pub source: String,
    pub slug: String,
}

#[derive(Clone, Debug, Default)]
pub struct McmodResult {
    pub entries: Vec<(String, String, String, Vec<McmodProjectLink>)>, // id, title, url, projects
    pub warnings: Vec<String>,
}

struct McmodState {
    cache: HashMap<String, (u64, McmodResult)>,
    order: VecDeque<String>,
    pending: HashMap<String, Arc<(Mutex<Option<Result<McmodResult, String>>>, Condvar)>>,
}

fn mcmod_state() -> &'static Mutex<McmodState> {
    static S: OnceLock<Mutex<McmodState>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(McmodState { cache: HashMap::new(), order: VecDeque::new(), pending: HashMap::new() }))
}

/// 并发上限 2 的请求槽（requestSlot 等价）
fn mcmod_slot() -> &'static Arc<(Mutex<u32>, Condvar)> {
    static S: OnceLock<Arc<(Mutex<u32>, Condvar)>> = OnceLock::new();
    S.get_or_init(|| Arc::new((Mutex::new(0), Condvar::new())))
}

struct McmodSlotGuard;
impl Drop for McmodSlotGuard {
    fn drop(&mut self) {
        let slot = mcmod_slot();
        let (count, cv) = (&slot.0, &slot.1);
        let mut c = count.lock().unwrap();
        *c = c.saturating_sub(1);
        cv.notify_one();
    }
}
fn mcmod_slot_enter() -> McmodSlotGuard {
    let slot = mcmod_slot();
    let (count, cv) = (&slot.0, &slot.1);
    let mut c = count.lock().unwrap();
    while *c >= 2 {
        c = cv.wait(c).unwrap();
    }
    *c += 1;
    McmodSlotGuard
}

/// HTML 文本提取（text()）
fn mcmod_text(html: &str) -> String {
    static TAGS: OnceLock<regex::Regex> = OnceLock::new();
    static ENTITY: OnceLock<regex::Regex> = OnceLock::new();
    static WS: OnceLock<regex::Regex> = OnceLock::new();
    let tags = TAGS.get_or_init(|| regex::Regex::new(r"<[^>]*>").unwrap());
    let mut s = tags.replace_all(html, "").to_string();
    let entity = ENTITY.get_or_init(|| regex::Regex::new(r"&#(x[0-9a-fA-F]+|\d+);").unwrap());
    s = entity
        .replace_all(&s, |c: &regex::Captures| {
            let v = &c[1];
            let point = if let Some(hex) = v.strip_prefix('x').or_else(|| v.strip_prefix('X')) {
                u32::from_str_radix(hex, 16).ok()
            } else {
                v.parse::<u32>().ok()
            };
            point.filter(|p| *p <= 0x10FFFF).and_then(char::from_u32).map(|c| c.to_string()).unwrap_or_default()
        })
        .to_string();
    for (from, to) in [("&amp;", "&"), ("&quot;", "\""), ("&apos;", "'"), ("&lt;", "<"), ("&gt;", ">"), ("&nbsp;", " "), ("&AMP;", "&")] {
        s = s.replace(from, to);
    }
    let ws = WS.get_or_init(|| regex::Regex::new(r"\s+").unwrap());
    ws.replace_all(&s, " ").trim().to_string()
}

/// anchors()：手写 <a href> 扫描（Rust regex 不支持反向引用）
fn mcmod_anchors(html: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let lower = html.to_lowercase();
    let mut cursor = 0usize;
    while let Some(a_start) = lower[cursor..].find("<a") {
        let tag_start = cursor + a_start;
        let Some(tag_end_rel) = lower[tag_start..].find('>') else { break };
        let tag = &html[tag_start..tag_start + tag_end_rel];
        let href = extract_attr(tag, "href");
        cursor = tag_start + tag_end_rel;
        let Some(close_rel) = lower[cursor..].find("</a") else { break };
        let inner = &html[cursor..cursor + close_rel];
        // 跳过 </a 后的收尾（\s*>）
        if let Some(gt) = lower[cursor + close_rel..].find('>') {
            cursor = cursor + close_rel + gt + 1;
        } else {
            break;
        }
        if let Some(href) = href {
            out.push((mcmod_text(&href), mcmod_text(inner)));
        }
    }
    out
}

fn extract_attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_lowercase();
    let key = format!("{name}=");
    let mut idx = 0usize;
    loop {
        let found = lower[idx..].find(&key)?;
        let value_start = idx + found + key.len();
        let quote = tag[value_start..].chars().next()?;
        if quote != '"' && quote != '\'' {
            idx = value_start;
            continue;
        }
        let rest = &tag[value_start + 1..];
        let end = rest.find(quote)?;
        return Some(rest[..end].to_string());
    }
}

/// mcmodNameRank()：斜杠分隔名称是百科标题的显式命名，不做模糊同义推断
pub fn mcmod_name_rank(title: &str, keyword: &str) -> i32 {
    static BRACKET: OnceLock<regex::Regex> = OnceLock::new();
    static PAREN: OnceLock<regex::Regex> = OnceLock::new();
    let bracket = BRACKET.get_or_init(|| regex::Regex::new(r"^\[[^\]]+\]\s*").unwrap());
    let paren = PAREN.get_or_init(|| regex::Regex::new(r"\s+\([^一-鿿]*\)\s*$").unwrap());
    let name = paren.replace(bracket.replace(title, "").trim(), "").trim().to_string();
    let key = normalize_chinese_mod_keyword(keyword);
    if key.is_empty() {
        return i32::MAX;
    }
    let main = normalize_chinese_mod_keyword(&name);
    if main == key {
        return 0;
    }
    if name.split(['／', '/']).any(|part| normalize_chinese_mod_keyword(part) == key) {
        return 1;
    }
    if main.contains(&key) {
        2
    } else {
        i32::MAX
    }
}

/// parseMcmodEntryTitle()
fn parse_mcmod_entry_title(html: &str) -> Option<String> {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = RE.get_or_init(|| regex::Regex::new(r#"(?is)<div\b[^>]*class=["'][^"']*\bclass-title\b[^"']*["'][^>]*>([\s\S]*?)<h3\b[^>]*>([\s\S]*?)</h3>"#).unwrap());
    re.captures(html).map(|c| mcmod_text(&c[2]))
}

/// parseMcmodSearch()：只解析结果列表；导航/正文/推荐位不产生项目身份
fn parse_mcmod_search(html: &str, keyword: &str) -> Result<Vec<(String, String, String)>, String> {
    let list = find_section(html, r#"<div\b[^>]*class=["'][^"']*\bsearch-result-list\b[^"']*["'][^>]*>"#, &[
        r#"<div\b[^>]*class=["'][^"']*search-result-pages"#,
        r"<footer\b",
    ])
    .ok_or_else(|| "MC百科搜索页面格式变化或访问受限".to_string())?;
    let key = normalize_chinese_mod_keyword(keyword);
    static ITEM: OnceLock<regex::Regex> = OnceLock::new();
    static BODY: OnceLock<regex::Regex> = OnceLock::new();
    static CLASS_ID: OnceLock<regex::Regex> = OnceLock::new();
    let item_re = ITEM.get_or_init(|| regex::Regex::new(r#"(?i)<div\b[^>]*class=["']result-item["'][^>]*>"#).unwrap());
    let body_re = BODY.get_or_init(|| regex::Regex::new(r#"(?i)<div\b[^>]*class=["']body["']"#).unwrap());
    let id_re = CLASS_ID.get_or_init(|| regex::Regex::new(r"^/class/(\d+)\.html$").unwrap());
    let mut found: Vec<(String, String, String)> = Vec::new();
    let mut segments: Vec<&str> = Vec::new();
    let mut pos = 0usize;
    while let Some(m) = item_re.find_at(&list, pos) {
        let start = m.end();
        let next = item_re.find_at(&list, start).map(|n| n.start()).unwrap_or(list.len());
        segments.push(&list[start..next]);
        pos = next;
    }
    for item in segments {
        let head = body_re.find(item).map(|m| &item[..m.start()]).unwrap_or(item);
        for (href, title) in mcmod_anchors(head) {
            let Ok(url) = url_join("https://www.mcmod.cn", &href) else { continue };
            if url.scheme != "https:" || url.host != "www.mcmod.cn" || !url.username.is_empty() || !url.password.is_empty() {
                continue;
            }
            let Some(idc) = id_re.captures(&url.path) else { continue };
            let id = idc[1].to_string();
            if title.is_empty() {
                continue;
            }
            static BRACKET: OnceLock<regex::Regex> = OnceLock::new();
            static PAREN: OnceLock<regex::Regex> = OnceLock::new();
            let bracket = BRACKET.get_or_init(|| regex::Regex::new(r"^\[[^\]]+\]\s*").unwrap());
            let paren = PAREN.get_or_init(|| regex::Regex::new(r"\s+\([^一-鿿]*\)\s*$").unwrap());
            let name = paren.replace(bracket.replace(&title, "").trim(), "").trim().to_string();
            if mcmod_name_rank(&name, &key) == i32::MAX {
                continue;
            }
            found.push((id.clone(), name, format!("https://www.mcmod.cn/class/{id}.html")));
        }
    }
    let mut unique: Vec<(String, String, String)> = Vec::new();
    let mut seen = HashSet::new();
    for f in found {
        if seen.insert(f.0.clone()) {
            unique.push(f);
        }
    }
    let exact: Vec<_> = unique.iter().filter(|(_, t, _)| mcmod_name_rank(t, &key) < 2).collect();
    let base = if exact.is_empty() { unique.iter().collect() } else { exact };
    let mut out: Vec<(String, String, String)> = base.into_iter().cloned().collect();
    out.sort_by_key(|(_, t, _)| mcmod_name_rank(t, &key));
    Ok(out)
}

/// 提取起止标记之间的区段（模拟前瞻捕获）
fn find_section<'a>(html: &'a str, start_re: &str, end_res: &[&str]) -> Option<&'a str> {
    static CACHE: OnceLock<Mutex<HashMap<String, regex::Regex>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let get = |pattern: &str| -> regex::Regex {
        cache.lock().unwrap().entry(pattern.to_string()).or_insert_with(|| regex::Regex::new(&format!("(?i){pattern}")).unwrap()).clone()
    };
    let m = get(start_re).find(html)?;
    let start = m.end();
    let mut end = html.len();
    for pat in end_res {
        if let Some(e) = get(pat).find_at(html, start) {
            end = end.min(e.start());
        }
    }
    Some(&html[start..end])
}

struct ParsedUrl {
    scheme: String,
    host: String,
    path: String,
    username: String,
    password: String,
    port: String,
}

fn url_join(base: &str, href: &str) -> Result<ParsedUrl, String> {
    let full = if href.contains("://") { href.to_string() } else { format!("{base}{href}") };
    parse_url(&full)
}

fn parse_url(raw: &str) -> Result<ParsedUrl, String> {
    let (scheme, rest) = raw.split_once("://").ok_or_else(|| "url".to_string())?;
    let (authority, path_query) = match rest.find(['/', '?', '#']) {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let (userinfo, hostport) = match authority.rsplit_once('@') {
        Some((u, h)) => (u, h),
        None => ("", authority),
    };
    let (username, password) = match userinfo.split_once(':') {
        Some((u, p)) => (u.to_string(), p.to_string()),
        None => (userinfo.to_string(), String::new()),
    };
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) => (h.to_string(), p.to_string()),
        _ => (hostport.to_string(), String::new()),
    };
    let path = path_query.split(['?', '#']).next().unwrap_or("").to_string();
    Ok(ParsedUrl { scheme: format!("{scheme}:"), host, path, username, password, port })
}

/// parseMcmodProjects()：条目相关链接列表中的来源项目（link.mcmod.cn 跳转解包）
fn parse_mcmod_projects(html: &str) -> Vec<McmodProjectLink> {
    let Some(links) = find_section(html, r#"<ul\b[^>]*class=["'][^"']*\bcommon-link-icon-frame\b[^"']*["'][^>]*>"#, &[r"</ul>"]) else {
        return Vec::new();
    };
    let mut result: Vec<McmodProjectLink> = Vec::new();
    for (href, _) in mcmod_anchors(links) {
        let Ok(mut url) = url_join("https://www.mcmod.cn", &href) else { continue };
        if url.scheme != "https:" || !url.username.is_empty() || !url.password.is_empty() {
            continue;
        }
        if url.host == "link.mcmod.cn" && url.path.starts_with("/target/") {
            let encoded = &url.path["/target/".len()..];
            if encoded.len() > 2048 || !encoded.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '_' | '=' | '-')) {
                continue;
            }
            use base64::Engine;
            let decoded = base64::engine::general_purpose::STANDARD.decode(encoded)
                .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(encoded))
                .or_else(|_| base64::engine::general_purpose::STANDARD.decode(format!("{}{}", encoded, "=".repeat((4 - encoded.len() % 4) % 4))))
                .ok();
            let Some(bytes) = decoded else { continue };
            let Ok(target) = String::from_utf8(bytes) else { continue };
            let Ok(next) = parse_url(&target) else { continue };
            url = next;
            if url.scheme != "https:" || !url.username.is_empty() || !url.password.is_empty() || (!url.port.is_empty() && url.port != "443") {
                continue;
            }
        }
        let source = if url.host == "modrinth.com" || url.host == "www.modrinth.com" {
            "modrinth"
        } else if url.host == "curseforge.com" || url.host == "www.curseforge.com" {
            "curseforge"
        } else {
            continue;
        };
        static MR: OnceLock<regex::Regex> = OnceLock::new();
        static CF: OnceLock<regex::Regex> = OnceLock::new();
        let slug = if source == "modrinth" {
            MR.get_or_init(|| regex::Regex::new(r"^/mod/([a-zA-Z0-9_-]+)/?$").unwrap()).captures(&url.path).map(|c| c[1].to_string())
        } else {
            CF.get_or_init(|| regex::Regex::new(r"^/minecraft/(?:mc-mods|mods)/([a-zA-Z0-9_-]+)/?$").unwrap()).captures(&url.path).map(|c| c[1].to_string())
        };
        if let Some(slug) = slug {
            result.push(McmodProjectLink { source: source.to_string(), slug: slug.to_lowercase() });
        }
    }
    let mut unique: Vec<McmodProjectLink> = Vec::new();
    let mut seen = HashSet::new();
    for link in result {
        if seen.insert(format!("{}:{}", link.source, link.slug)) {
            unique.push(link);
        }
    }
    unique.truncate(8);
    unique
}

/// MC 百科 HTML 抓取（readHtml：禁重定向、content-type 校验、2MB 上限、10s 超时）
fn mcmod_read_html(url: &str) -> Result<String, String> {
    static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    let client = CLIENT.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .user_agent("WUHUICL (+https://github.com/AUGUHDAR/WUHUICL)")
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .expect("mcmod client")
    });
    let resp = client
        .get(url)
        .header("Accept", "text/html")
        .timeout(Duration::from_secs(10))
        .send()
        .map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(format!("MC百科 HTTP {status}"));
    }
    let ctype = resp.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    if !ctype.contains("text/html") {
        return Err("MC百科未返回可识别的搜索页面".into());
    }
    if let Some(len) = resp.content_length() {
        if len as usize > MCMOD_MAX_BYTES {
            return Err("MC百科页面超出读取限制".into());
        }
    }
    let body = resp.bytes().map_err(|e| e.to_string())?;
    if body.len() > MCMOD_MAX_BYTES {
        return Err("MC百科页面超出读取限制".into());
    }
    String::from_utf8(body.to_vec()).map_err(|e| e.to_string())
}

/// lookupMcmod()：一次搜索 + 至多一个追加结果页 + 5 个条目页；6h 缓存 64 条；空结果不缓存
pub fn lookup_mcmod(keyword: &str) -> McmodResult {
    let key = normalize_chinese_mod_keyword(keyword);
    if !has_cjk(&key) || key.chars().count() < 2 || key.chars().count() > 80 {
        return McmodResult::default();
    }
    let now_ms = crate::download::now_ms();
    let slot;
    {
        let mut state = mcmod_state().lock().unwrap();
        if let Some((at, result)) = state.cache.get(&key) {
            if now_ms - at < 6 * 60 * 60_000 {
                return result.clone();
            }
        }
        if let Some(entry) = state.pending.get(&key).cloned() {
            drop(state);
            let (result_lock, cv) = &*entry;
            let mut guard = result_lock.lock().unwrap();
            let deadline = Instant::now() + Duration::from_secs(120);
            loop {
                if let Some(result) = guard.as_ref() {
                    return result.clone().unwrap_or_default();
                }
                if Instant::now() >= deadline {
                    return McmodResult::default();
                }
                let (g, _timeout) = cv.wait_timeout(guard, Duration::from_secs(10)).unwrap();
                guard = g;
            }
        }
        let entry = Arc::new((Mutex::new(None), Condvar::new()));
        state.pending.insert(key.clone(), entry.clone());
        slot = entry;
    }
    let result = run_mcmod_lookup(keyword);
    {
        let mut state = mcmod_state().lock().unwrap();
        state.pending.remove(&key);
        if let Ok(r) = &result {
            if !r.entries.is_empty() && r.warnings.is_empty() {
                if state.cache.len() >= 64 {
                    if let Some(old) = state.order.pop_front() {
                        state.cache.remove(&old);
                    }
                }
                state.order.push_back(key.clone());
                state.cache.insert(key.clone(), (crate::download::now_ms(), r.clone()));
            }
        }
        *slot.0.lock().unwrap() = Some(result.clone());
        slot.1.notify_all();
    }
    result.unwrap_or_default()
}

fn run_mcmod_lookup(keyword: &str) -> Result<McmodResult, String> {
    let _guard = mcmod_slot_enter();
    let trimmed = keyword.trim();
    static SUFFIX: OnceLock<regex::Regex> = OnceLock::new();
    let re = SUFFIX.get_or_init(|| regex::Regex::new(r"(?i)\s*(?:模组|模組|mod)$").unwrap());
    let key_param = re.replace(trimmed, "").to_string();
    let mut params = vec![("key", key_param), ("filter", "1".to_string())];
    let html = mcmod_read_html(&format!("https://search.mcmod.cn/s?{}", query_string(&params)))?;
    let mut candidates = parse_mcmod_search(&html, keyword)?;
    let has_exact = candidates.iter().any(|(_, t, _)| mcmod_name_rank(t, &normalize_chinese_mod_keyword(keyword)) < 2);
    let mut limited = false;
    if !has_exact && html.contains("data-page=\"2\"") {
        params.push(("page", "2".to_string()));
        let next = mcmod_read_html(&format!("https://search.mcmod.cn/s?{}", query_string(&params)))?;
        let mut merged = candidates;
        for c in parse_mcmod_search(&next, keyword)? {
            if !merged.iter().any(|m| m.0 == c.0) {
                merged.push(c);
            }
        }
        limited = next.contains("data-page=\"3\"");
        candidates = merged;
    }
    let key_norm = normalize_chinese_mod_keyword(keyword);
    let matched: Vec<_> = candidates.iter().filter(|(_, t, _)| mcmod_name_rank(t, &key_norm) < 2).cloned().collect();
    if !matched.is_empty() {
        candidates = matched;
    }
    limited = limited || candidates.len() > MCMOD_MAX_ENTRIES;
    candidates.truncate(MCMOD_MAX_ENTRIES);
    let mut entries: Vec<(String, String, String, Vec<McmodProjectLink>)> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    for (id, title, url) in &candidates {
        match mcmod_read_html(url) {
            Ok(page) => {
                let Some(entry_title) = parse_mcmod_entry_title(&page) else {
                    entries.push((id.clone(), title.clone(), url.clone(), parse_mcmod_projects(&page)));
                    continue;
                };
                let title_rank = mcmod_name_rank(title, &key_norm);
                let page_rank = mcmod_name_rank(&entry_title, &key_norm);
                if page_rank == i32::MAX || (title_rank < 2 && page_rank >= 2) {
                    warnings.push(format!("MC百科“{title}”的条目名称已变化，未关联其他同名项目；请核对完整名称。"));
                    continue;
                }
                entries.push((id.clone(), entry_title, url.clone(), parse_mcmod_projects(&page)));
            }
            Err(_) => warnings.push(format!("MC百科“{title}”的来源链接读取失败，未自动关联项目；可重试。")),
        }
    }
    if limited {
        warnings.push("MC百科中文检索仅关联前两页中的最多 5 个条目；请使用完整中文名缩小范围。".into());
    }
    Ok(McmodResult { entries, warnings })
}

// ---------------- Modrinth（community.ts mrFetch/mrSearch/mrFiles） ----------------

const MR_BASES: [&str; 2] = ["https://api.modrinth.com/v2", "https://mod.mcimirror.top/modrinth/v2"];

/// 主备互备请求 Modrinth
pub fn mr_fetch(path: &str) -> Result<Value, String> {
    let mut last_err = String::new();
    for base in MR_BASES {
        match fetch_json(&format!("{base}{path}"), &[], TIMEOUT_MS) {
            Ok(v) => return Ok(v),
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

/// mrPost()：POST 主备互备（mirror=bmclapi 时主备对调，与 modUpdates.mrPost 同源策略）
pub fn mr_post(app: &tauri::AppHandle, path: &str, body: &Value, ua: &str, timeout_ms: u64) -> Result<Value, String> {
    let mirror = crate::settings::get(app)
        .get("mirror")
        .and_then(|v| v.as_str())
        .unwrap_or("official")
        .to_string();
    let mut bases: Vec<&str> = MR_BASES.to_vec();
    if mirror == "bmclapi" {
        bases.reverse();
    }
    let mut last_err = String::new();
    for base in bases {
        let req = net::client()
            .post(format!("{base}{path}"))
            .timeout(Duration::from_millis(timeout_ms))
            .header("User-Agent", ua)
            .header("Content-Type", "application/json")
            .body(body.to_string());
        match req.send() {
            Ok(resp) => {
                let status = resp.status().as_u16();
                if !(200..300).contains(&status) {
                    last_err = format!("HTTP {status}");
                    continue;
                }
                match resp.json::<Value>() {
                    Ok(v) => return Ok(v),
                    Err(e) => last_err = e.to_string(),
                }
            }
            Err(e) => last_err = e.to_string(),
        }
    }
    Err(last_err)
}

/// String(value ?? '')：数值 id → 十进制串（String(123) 语义）
pub fn value_to_string_id(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

fn mr_project_type(kind: &str) -> &'static str {
    match kind {
        "mod" => "mod",
        "modpack" => "modpack",
        "resourcepack" => "resourcepack",
        "shader" => "shader",
        "datapack" => "datapack",
        _ => "mod",
    }
}

fn mr_sort_index(sort: Option<&str>) -> &'static str {
    match sort {
        Some("downloads") => "downloads",
        Some("newest") => "newest",
        _ => "relevance",
    }
}

struct Query {
    keyword: String,
    kind: String,
    source: String,
    mc_version: Option<String>,
    loader: Option<String>,
    sort: Option<String>,
    offset: u64,
    limit: u64,
}

impl Query {
    fn from_value(v: &Value) -> Query {
        Query {
            keyword: v.get("keyword").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            kind: v.get("kind").and_then(|x| x.as_str()).unwrap_or("mod").to_string(),
            source: v.get("source").and_then(|x| x.as_str()).unwrap_or("all").to_string(),
            mc_version: v.get("mcVersion").and_then(|x| x.as_str()).map(str::to_string),
            loader: v.get("loader").and_then(|x| x.as_str()).map(str::to_string),
            sort: v.get("sort").and_then(|x| x.as_str()).map(str::to_string),
            offset: v.get("offset").and_then(|x| x.as_i64()).unwrap_or(0).max(0) as u64,
            limit: v.get("limit").and_then(|x| x.as_i64()).unwrap_or(20).max(1) as u64,
        }
    }

    fn clone_with(&self, keyword: Option<&str>, offset: u64, limit: u64) -> Query {
        Query {
            keyword: keyword.map(str::to_string).unwrap_or_else(|| self.keyword.clone()),
            kind: self.kind.clone(),
            source: self.source.clone(),
            mc_version: self.mc_version.clone(),
            loader: self.loader.clone(),
            sort: self.sort.clone(),
            offset,
            limit,
        }
    }

    /// aliasCatalogs / sourceCounts 缓存键（JSON.stringify({...q, source, offset:0, limit:0}) 等价）
    fn cache_key(&self, source: &str, mcmod: bool) -> String {
        let mut obj = serde_json::Map::new();
        obj.insert("keyword".into(), json!(self.keyword));
        obj.insert("kind".into(), json!(self.kind));
        obj.insert("source".into(), json!(source));
        obj.insert("mcVersion".into(), json!(self.mc_version));
        obj.insert("loader".into(), json!(self.loader));
        obj.insert("sort".into(), json!(self.sort));
        obj.insert("offset".into(), json!(0));
        obj.insert("limit".into(), json!(0));
        if mcmod {
            obj.insert("mcmod".into(), json!(true));
        }
        Value::Object(obj).to_string()
    }
}

/// mrSearch()
fn mr_search(q: &Query) -> Result<Page, String> {
    let mut facets: Vec<Vec<String>> = vec![vec![format!("project_type:{}", mr_project_type(&q.kind))]];
    if let Some(mc) = &q.mc_version {
        facets.push(vec![format!("versions:{mc}")]);
    }
    if let Some(loader) = &q.loader {
        if uses_community_loader(Some(&q.kind)) {
            facets.push(vec![format!("categories:{loader}")]);
        }
    }
    let params = query_string(&[
        ("query", q.keyword.clone()),
        ("limit", q.limit.to_string()),
        ("offset", q.offset.to_string()),
        ("index", mr_sort_index(q.sort.as_deref()).to_string()),
        ("facets", serde_json::to_string(&facets).unwrap_or_default()),
    ]);
    let data = mr_fetch(&format!("/search?{params}"))?;
    let total = data
        .get("total_hits")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| "Modrinth 未返回结果总数，请重试".to_string())?;
    let items = data
        .get("hits")
        .and_then(|v| v.as_array())
        .map(|hits| {
            hits.iter()
                .map(|h| {
                    json!({
                        "source": "modrinth",
                        "projectId": h.get("project_id").and_then(|v| v.as_str()).unwrap_or(""),
                        "slug": h.get("slug").and_then(|v| v.as_str()).unwrap_or(""),
                        "title": h.get("title").and_then(|v| v.as_str()).unwrap_or(""),
                        "author": h.get("author").and_then(|v| v.as_str()).unwrap_or(""),
                        "description": h.get("description").and_then(|v| v.as_str()).unwrap_or(""),
                        "iconUrl": h.get("icon_url").and_then(|v| v.as_str()).unwrap_or(""),
                        "downloads": h.get("downloads").and_then(|v| v.as_u64()).unwrap_or(0),
                        "updatedAt": h.get("date_modified").and_then(|v| v.as_str()).unwrap_or(""),
                        "categories": h.get("categories").cloned().unwrap_or(Value::Array(Vec::new())),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(Page { items, total, offset: q.offset, limit: q.limit, warnings: Vec::new() })
}

struct MrVersionFile<'a> {
    filename: Option<&'a str>,
    url: Option<&'a str>,
    sha1: Option<&'a str>,
    size: u64,
    primary: bool,
}

/// mapMrVersions()
fn map_mr_versions(arr: &[Value], project_id: Option<&str>, retain_unavailable: bool) -> Vec<Value> {
    let mut out = Vec::new();
    for v in arr {
        let files = v.get("files").and_then(|f| f.as_array()).cloned().unwrap_or_default();
        let mapped: Vec<MrVersionFile<'_>> = files
            .iter()
            .map(|f| MrVersionFile {
                filename: f.get("filename").and_then(|x| x.as_str()),
                url: f.get("url").and_then(|x| x.as_str()),
                sha1: f.get("hashes").and_then(|h| h.get("sha1")).and_then(|x| x.as_str()),
                size: f.get("size").and_then(|x| x.as_u64()).unwrap_or(0),
                primary: f.get("primary").and_then(|x| x.as_bool()).unwrap_or(false),
            })
            .collect();
        let f = mapped.iter().find(|x| x.primary).or_else(|| mapped.first());
        if (f.map(|x| x.url.is_none() || x.filename.is_none()).unwrap_or(true)) && !retain_unavailable {
            continue;
        }
        let mut item = serde_json::Map::new();
        item.insert("source".into(), json!("modrinth"));
        item.insert(
            "projectId".into(),
            json!(v.get("project_id").and_then(|x| x.as_str()).or(project_id).unwrap_or("")),
        );
        let deps: Vec<Value> = v
            .get("dependencies")
            .and_then(|d| d.as_array())
            .map(|list| {
                list.iter()
                    .map(|d| {
                        let mut o = serde_json::Map::new();
                        if let Some(p) = d.get("project_id").and_then(|x| x.as_str()) {
                            o.insert("projectId".into(), json!(p));
                        }
                        if let Some(fid) = d.get("version_id").and_then(|x| x.as_str()) {
                            o.insert("fileId".into(), json!(fid));
                        }
                        o.insert("required".into(), json!(d.get("dependency_type").and_then(|x| x.as_str()) == Some("required")));
                        Value::Object(o)
                    })
                    .collect()
            })
            .unwrap_or_default();
        item.insert("dependencies".into(), Value::Array(deps));
        item.insert(
            "fileId".into(),
            json!(v.get("id").and_then(|x| x.as_str()).or(f.and_then(|x| x.filename)).unwrap_or("")),
        );
        item.insert("fileName".into(), json!(f.and_then(|x| x.filename).unwrap_or("")));
        item.insert(
            "version".into(),
            json!(v.get("version_number").and_then(|x| x.as_str()).or(f.and_then(|x| x.filename)).unwrap_or("")),
        );
        item.insert("url".into(), json!(f.and_then(|x| x.url).unwrap_or("")));
        if let Some(sha1) = f.and_then(|x| x.sha1) {
            item.insert("sha1".into(), json!(sha1));
        }
        item.insert("size".into(), json!(f.map(|x| x.size).unwrap_or(0)));
        let vt = v.get("version_type").and_then(|x| x.as_str()).unwrap_or("");
        item.insert("releaseType".into(), json!(if vt == "beta" { "beta" } else if vt == "alpha" { "alpha" } else { "release" }));
        item.insert("gameVersions".into(), v.get("game_versions").cloned().unwrap_or(json!([])));
        item.insert("loaders".into(), v.get("loaders").cloned().unwrap_or(json!([])));
        item.insert("date".into(), json!(v.get("date_published").and_then(|x| x.as_str()).unwrap_or("")));
        out.push(Value::Object(item));
    }
    out
}

/// mrFiles()：项目版本列表（可按 mc 版本 / loader / kind 过滤；retainUnavailable 供收藏校验用）
pub fn mr_files(project_id: &str, filter: Option<&FileFilter>, retain_unavailable: bool) -> Result<Vec<Value>, String> {
    let mut params: Vec<(String, String)> = Vec::new();
    if let Some(mc) = filter.and_then(|f| f.mc_version.as_deref()).filter(|s| !s.is_empty()) {
        params.push(("game_versions".into(), serde_json::to_string(&[mc]).unwrap_or_default()));
    }
    let kind = filter.and_then(|f| f.kind.as_deref());
    let loaders: Option<Vec<String>> = kind.and_then(modrinth_resource_loaders).map(|ls| ls.iter().map(|s| s.to_string()).collect()).or_else(|| {
        filter.and_then(|f| f.loader.as_deref()).filter(|s| !s.is_empty()).map(|l| vec![l.to_string()])
    });
    if let Some(ls) = loaders {
        params.push(("loaders".into(), serde_json::to_string(&ls).unwrap_or_default()));
    }
    let qs = if params.is_empty() {
        String::new()
    } else {
        format!("?{}", params.iter().map(|(k, v)| format!("{}={}", enc(k), enc(v))).collect::<Vec<_>>().join("&"))
    };
    let arr = mr_fetch(&format!("/project/{}/version{}", enc(project_id), qs))?;
    let arr = arr.as_array().cloned().unwrap_or_default();
    Ok(map_mr_versions(&arr, Some(project_id), retain_unavailable))
}

// ---------------- CurseForge（官方 API 优先，MCIM 镜像兜底；curseforgeChannel.ts） ----------------

pub const CF_OFFICIAL: &str = "https://api.curseforge.com/v1";
pub const CF_MIRROR: &str = "https://mod.mcimirror.top/curseforge/v1";

pub struct CfChannel {
    pub base: &'static str,
    pub official: bool,
    pub key: String,
}

/// cfChannel()：环境变量 → settings.curseforgeApiKey → 内置公开 key
pub fn cf_channel(app: &tauri::AppHandle) -> CfChannel {
    let env = std::env::var("KAMUCL_CF_API_KEY").ok().map(|s| s.trim().to_string()).unwrap_or_default();
    let setting = crate::settings::get(app)
        .get("curseforgeApiKey")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let builtin = "$2a$10$m36VLjTaHEqxr/hO3kMDE.XCDEG90rSu3iGKkoPsj0KdCPWXOASXG";
    let key = if !env.is_empty() {
        env
    } else if !setting.is_empty() {
        setting
    } else {
        builtin.to_string()
    };
    if !key.is_empty() {
        CfChannel { base: CF_OFFICIAL, official: true, key }
    } else {
        CfChannel { base: CF_MIRROR, official: false, key: String::new() }
    }
}

fn cf_class_id(kind: &str) -> u32 {
    match kind {
        "mod" => 6,
        "modpack" => 4471,
        "resourcepack" => 12,
        "shader" => 6552,
        "datapack" => 6945,
        _ => 6,
    }
}

fn cf_loader_type(loader: &str) -> u32 {
    match loader {
        "forge" => 1,
        "fabric" => 4,
        "quilt" => 5,
        "neoforge" => 6,
        _ => 0,
    }
}

const LOADER_NAMES: [&str; 4] = ["forge", "fabric", "quilt", "neoforge"];

/// cfFetch()
/// modpacks.rs 等：CurseForge API 通道
pub fn cf_fetch_public(app: &tauri::AppHandle, path: &str) -> Result<Value, String> {
    cf_fetch(app, path)
}

fn cf_fetch(app: &tauri::AppHandle, path: &str) -> Result<Value, String> {
    let ch = cf_channel(app);
    if ch.official {
        fetch_json(&format!("{}{}", ch.base, path), &[("x-api-key", &ch.key)], TIMEOUT_MS)
    } else {
        fetch_json(&format!("{}{}", ch.base, path), &[], TIMEOUT_MS)
    }
}

fn cf_sort_field(sort: Option<&str>) -> u32 {
    match sort {
        Some("downloads") => 6,
        Some("newest") => 3,
        _ => 2,
    }
}

/// cfSearch()
fn cf_search(app: &tauri::AppHandle, q: &Query) -> Result<Page, String> {
    let mut params: Vec<(String, String)> = vec![
        ("gameId".into(), "432".into()),
        ("classId".into(), cf_class_id(&q.kind).to_string()),
        ("searchFilter".into(), q.keyword.clone()),
        ("index".into(), q.offset.to_string()),
        ("pageSize".into(), q.limit.to_string()),
        ("sortField".into(), cf_sort_field(q.sort.as_deref()).to_string()),
        ("sortOrder".into(), "desc".into()),
    ];
    if let Some(mc) = &q.mc_version {
        params.push(("gameVersion".into(), mc.clone()));
    }
    if let Some(loader) = &q.loader {
        if uses_community_loader(Some(&q.kind)) {
            params.push(("modLoaderType".into(), cf_loader_type(loader).to_string()));
        }
    }
    let data = cf_fetch(app, &format!("/mods/search?{}", query_string_owned(&params)))?;
    let total_count = data
        .get("pagination")
        .and_then(|p| p.get("totalCount"))
        .and_then(|v| v.as_u64())
        .ok_or_else(|| "CurseForge 未返回结果总数，请重试".to_string())?;
    let items = data
        .get("data")
        .and_then(|v| v.as_array())
        .map(|list| {
            list.iter()
                .map(|m| {
                    let categories: Vec<String> = m
                        .get("categories")
                        .and_then(|c| c.as_array())
                        .map(|a| a.iter().filter_map(|c| c.get("name").and_then(|n| n.as_str()).map(str::to_string)).collect())
                        .unwrap_or_default();
                    json!({
                        "source": "curseforge",
                        "projectId": value_to_string_id(m.get("id")),
                        "slug": m.get("slug").and_then(|v| v.as_str()).unwrap_or(""),
                        "title": m.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                        "author": m.get("authors").and_then(|a| a.as_array()).and_then(|a| a.first()).and_then(|a| a.get("name")).and_then(|v| v.as_str()).unwrap_or(""),
                        "description": m.get("summary").and_then(|v| v.as_str()).unwrap_or(""),
                        "iconUrl": m.get("logo").and_then(|l| l.get("thumbnailUrl")).and_then(|v| v.as_str()).unwrap_or(""),
                        "downloads": m.get("downloadCount").and_then(|v| v.as_u64()).unwrap_or(0),
                        "updatedAt": m.get("dateModified").and_then(|v| v.as_str()).unwrap_or(""),
                        "categories": categories,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    // CurseForge 只允许访问前 10,000 个结果。
    let total = total_count.min(10_000);
    Ok(Page { items, total, offset: q.offset, limit: q.limit, warnings: Vec::new() })
}

/// mapCfFiles()
fn map_cf_files(files: &[Value], project_id: &str) -> Vec<Value> {
    files
        .iter()
        .map(|f| {
            let id = value_to_string_id(f.get("id"));
            let num_id = f.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            let game_versions: Vec<String> = f
                .get("gameVersions")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            let file_name = f.get("fileName").and_then(|v| v.as_str()).unwrap_or("").to_string();
            // downloadUrl 缺失时按 ForgeCDN 规则拼地址（media 403 / edge 可用）
            let fallback_url = if num_id > 0 && !file_name.is_empty() {
                format!("https://edge.forgecdn.net/files/{}/{}/{}", num_id / 1000, num_id % 1000, enc(&file_name))
            } else {
                String::new()
            };
            let sha1 = f
                .get("hashes")
                .and_then(|h| h.as_array())
                .and_then(|a| a.iter().find(|h| h.get("algo").and_then(|v| v.as_u64()) == Some(1)))
                .and_then(|h| h.get("value"))
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let loaders: Vec<String> = game_versions
                .iter()
                .map(|g| g.to_lowercase())
                .filter(|g| LOADER_NAMES.contains(&g.as_str()))
                .collect();
            let mut item = serde_json::Map::new();
            let mod_id = value_to_string_id(f.get("modId"));
            let cf_project = if mod_id.is_empty() { project_id.to_string() } else { mod_id };
            item.insert("source".into(), json!("curseforge"));
            item.insert("projectId".into(), json!(cf_project));
            let deps: Vec<Value> = f
                .get("dependencies")
                .and_then(|d| d.as_array())
                .map(|list| {
                    list.iter()
                        .map(|d| {
                            json!({
                                "projectId": d.get("modId").map(|v| v.to_string()).unwrap_or_default(),
                                "required": d.get("relationType").and_then(|v| v.as_u64()) == Some(3),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            item.insert("dependencies".into(), Value::Array(deps));
            item.insert("fileId".into(), json!(id));
            item.insert("fileName".into(), json!(if file_name.is_empty() { id.clone() } else { file_name.clone() }));
            let display_name = f.get("displayName").and_then(|v| v.as_str()).unwrap_or("");
            item.insert(
                "version".into(),
                json!(if display_name.is_empty() { if file_name.is_empty() { id.clone() } else { file_name.clone() } } else { display_name.to_string() }),
            );
            let url = f.get("downloadUrl").and_then(|v| v.as_str()).unwrap_or("");
            item.insert("url".into(), json!(if url.is_empty() { fallback_url } else { url.to_string() }));
            if let Some(sha1) = sha1 {
                item.insert("sha1".into(), json!(sha1));
            }
            item.insert("size".into(), json!(f.get("fileLength").and_then(|v| v.as_u64()).unwrap_or(0)));
            let rt = f.get("releaseType").and_then(|v| v.as_u64()).unwrap_or(0);
            item.insert("releaseType".into(), json!(if rt == 2 { "beta" } else if rt == 3 { "alpha" } else { "release" }));
            item.insert("gameVersions".into(), json!(game_versions));
            item.insert("loaders".into(), json!(loaders));
            item.insert("date".into(), json!(f.get("fileDate").and_then(|v| v.as_str()).unwrap_or("")));
            Value::Object(item)
        })
        .collect()
}

/// cfFiles()：分页拉全量文件（pageSize 50）
fn cf_files(app: &tauri::AppHandle, project_id: &str, filter: Option<&FileFilter>) -> Result<Vec<Value>, String> {
    let mut params: Vec<(String, String)> = vec![("pageSize".into(), "50".into())];
    if let Some(mc) = filter.and_then(|f| f.mc_version.as_deref()).filter(|s| !s.is_empty()) {
        params.push(("gameVersion".into(), mc.to_string()));
    }
    if let Some(loader) = filter.and_then(|f| f.loader.as_deref()).filter(|s| !s.is_empty()) {
        params.push(("modLoaderType".into(), cf_loader_type(loader).to_string()));
    }
    let mut all: Vec<Value> = Vec::new();
    let mut index: u64 = 0;
    loop {
        params.retain(|(k, _)| k != "index");
        params.push(("index".into(), index.to_string()));
        let data = cf_fetch(app, &format!("/mods/{}/files?{}", enc(project_id), query_string_owned(&params)))?;
        let page: Vec<Value> = data.get("data").and_then(|v| v.as_array()).cloned().unwrap_or_default();
        let total = data.get("pagination").and_then(|p| p.get("totalCount")).and_then(|v| v.as_u64());
        let page_len = page.len();
        all.extend(page);
        if page_len < 50 || all.len() as u64 >= total.unwrap_or(u64::MAX) {
            break;
        }
        if index >= 9950 {
            return Err("项目版本过多，请先选择 Minecraft 版本 / Loader 后重试".into());
        }
        index += 50;
    }
    Ok(map_cf_files(&all, project_id))
}

// ---------------- 分页（communityPaging.ts） ----------------

#[derive(Debug, Clone, Copy, PartialEq)]
struct Slot {
    source: &'static str,
    index: u64,
}

/// communityPageSlots()：两源交错组成一条结果序列；一源耗尽后继续另一源
fn community_page_slots(mr_total: u64, cf_total: u64, offset: u64, limit: u64) -> Vec<Slot> {
    let paired = mr_total.min(cf_total);
    let remainder: &'static str = if mr_total > paired { "modrinth" } else { "curseforge" };
    let mut slots = Vec::new();
    let end = (offset + limit).min(mr_total + cf_total);
    let mut i = offset;
    while i < end {
        if i < paired * 2 {
            slots.push(Slot { source: if i % 2 == 1 { "curseforge" } else { "modrinth" }, index: i / 2 });
        } else {
            slots.push(Slot { source: remainder, index: paired + i - paired * 2 });
        }
        i += 1;
    }
    slots
}

// ---------------- 搜索编排（community.ts providerSearch/communitySearchPage） ----------------

struct Page {
    items: Vec<Value>,
    total: u64,
    offset: u64,
    limit: u64,
    warnings: Vec<String>,
}

impl Page {
    fn to_json(self) -> Value {
        let mut obj = serde_json::Map::new();
        obj.insert("items".into(), Value::Array(self.items));
        obj.insert("total".into(), json!(self.total));
        obj.insert("offset".into(), json!(self.offset));
        obj.insert("limit".into(), json!(self.limit));
        if !self.warnings.is_empty() {
            obj.insert("warnings".into(), json!(self.warnings));
        }
        Value::Object(obj)
    }
}

fn dedup_warnings(list: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    list.into_iter().filter(|w| seen.insert(w.clone())).collect()
}

fn dedup_items(items: Vec<Value>) -> Vec<Value> {
    // Map 语义：同键后写覆盖（保留最后一条）且保持首次出现的顺序
    let mut out: Vec<Value> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for item in items {
        let key = format!(
            "{}:{}",
            item.get("source").and_then(|v| v.as_str()).unwrap_or(""),
            item.get("projectId").and_then(|v| v.as_str()).unwrap_or("")
        );
        match index.get(&key) {
            Some(&i) => out[i] = item,
            None => {
                index.insert(key, out.len());
                out.push(item);
            }
        }
    }
    out
}

struct AliasCatalog {
    at: u64,
    items: Vec<Value>,
    warnings: Vec<String>,
}

fn alias_catalogs() -> &'static Mutex<(HashMap<String, AliasCatalog>, VecDeque<String>)> {
    static M: OnceLock<Mutex<(HashMap<String, AliasCatalog>, VecDeque<String>)>> = OnceLock::new();
    M.get_or_init(|| Mutex::new((HashMap::new(), VecDeque::new())))
}

fn alias_cache_get(key: &str) -> Option<AliasCatalog> {
    let (map, _) = &mut *alias_catalogs().lock().unwrap();
    let entry = map.get(key)?;
    if crate::download::now_ms() - entry.at < 60_000 {
        Some(AliasCatalog { at: entry.at, items: entry.items.clone(), warnings: entry.warnings.clone() })
    } else {
        None
    }
}

fn alias_cache_put(key: String, items: Vec<Value>, warnings: Vec<String>) {
    let (map, order) = &mut *alias_catalogs().lock().unwrap();
    if map.len() >= 12 {
        if let Some(old) = order.pop_front() {
            map.remove(&old);
        }
    }
    order.push_back(key.clone());
    map.insert(key, AliasCatalog { at: crate::download::now_ms(), items, warnings });
}

/// rawProviderSearch()
fn raw_provider_search(app: &tauri::AppHandle, source: &str, q: &Query) -> Result<Page, String> {
    if source == "modrinth" {
        mr_search(q)
    } else {
        cf_search(app, q)
    }
}

/// providerSearch()：中文关键词 → 百科身份桥接 + 内置别名；其余直查
fn provider_search(app: &tauri::AppHandle, source: &str, q: &Query) -> Result<Page, String> {
    let chinese = q.kind == "mod" && has_cjk(&q.keyword);
    let mut builtin: Option<Page> = None;
    if chinese && has_exact_chinese_mod_name(&q.keyword) {
        let b = builtin_provider_search(app, source, q)?;
        if b.total != 0 {
            return Ok(b);
        }
        builtin = Some(b);
    }
    if chinese {
        let key = q.cache_key(source, true);
        if let Some(catalog) = alias_cache_get(&key) {
            let total = catalog.items.len() as u64;
            let items: Vec<Value> = catalog
                .items
                .into_iter()
                .skip(q.offset as usize)
                .take(q.limit as usize)
                .collect();
            return Ok(Page { items, total, offset: q.offset, limit: q.limit, warnings: catalog.warnings });
        }
        // 注意：total 取目录长度（此处已截取 items，先记录长度再截取）
        let encyclopedia = lookup_mcmod(&q.keyword);
        if encyclopedia.entries.is_empty() {
            let page = match builtin {
                Some(b) => b,
                None => builtin_provider_search(app, source, q)?,
            };
            let mut warnings = page.warnings.clone();
            warnings.extend(encyclopedia.warnings.clone());
            return Ok(Page { items: page.items, total: page.total, offset: page.offset, limit: page.limit, warnings });
        }
        // MC 百科只提供身份桥接，不替代下载仓库：保留原关键词结果
        let original = raw_provider_search(app, source, &q.clone_with(None, 0, 50));
        let mut items: Vec<Value> = Vec::new();
        let mut warnings: Vec<String> = Vec::new();
        let mut provider_succeeded = original.is_ok();
        match &original {
            Ok(page) => {
                items.extend(page.items.iter().cloned());
                warnings.extend(page.warnings.iter().cloned());
            }
            Err(_) => warnings.push("原中文关键词查询失败，当前仅显示已核对来源标识的百科关联项目；可重试。".into()),
        }
        warnings.extend(encyclopedia.warnings.iter().cloned());
        let mut linked: Vec<(String, String)> = Vec::new(); // slug -> title
        for (id, title, _url, projects) in &encyclopedia.entries {
            let _ = id;
            for link in projects {
                if link.source == source {
                    linked.push((link.slug.clone(), title.clone()));
                }
            }
        }
        // dedup by slug, keep last（Map 语义）
        let mut by_slug: HashMap<String, String> = HashMap::new();
        for (slug, title) in linked {
            by_slug.insert(slug, title);
        }
        let identities: Vec<(String, String)> = by_slug.into_iter().collect();
        if identities.len() > 10 {
            warnings.push("百科条目关联的来源项目较多，本次仅核对前 10 项；请使用完整中文名缩小范围。".into());
        }
        let identities: Vec<_> = identities.into_iter().take(10).collect();
        if identities.is_empty() {
            warnings.push(format!(
                "MC百科条目未提供 {} 项目链接，已保留原中文结果；可切换来源。",
                if source == "modrinth" { "Modrinth" } else { "CurseForge" }
            ));
        }
        let mut original_total: u64 = 0;
        if let Ok(page) = &original {
            original_total = page.total;
        }
        let original_err = original.err();
        for (slug, title) in &identities {
            match raw_provider_search(app, source, &q.clone_with(Some(slug.as_str()), 0, 50)) {
                Ok(page) => {
                    provider_succeeded = true;
                    for item in page.items {
                        let item_slug = item.get("slug").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
                        if item_slug == slug.to_lowercase() {
                            let mut it = item.clone();
                            let original_title = it.get("originalTitle").and_then(|v| v.as_str()).map(str::to_string)
                                .unwrap_or_else(|| it.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string());
                            if let Some(obj) = it.as_object_mut() {
                                obj.insert("originalTitle".into(), json!(original_title));
                                obj.insert("title".into(), json!(format!("{title} | {original_title}")));
                            }
                            items.push(it);
                        }
                    }
                }
                Err(_) => warnings.push(format!(
                    "“{title}”的 {} 项目查询失败，可重试；未使用同名项目替代。",
                    if source == "modrinth" { "Modrinth" } else { "CurseForge" }
                )),
            }
        }
        if !provider_succeeded {
            return Err(original_err.unwrap_or_else(|| "社区来源查询失败，请稍后重试".into()));
        }
        if !encyclopedia.entries.is_empty() {
            warnings.push("MC百科中文名称关联：仅使用百科条目明确链接且当前版本／加载器筛选匹配的来源项目。".into());
        }
        if original_total > 50 {
            warnings.push("原中文关键词匹配较多，本次百科关联合并前 50 项；请缩小关键词查看其他结果。".into());
        }
        let items = dedup_items(items);
        let warnings = dedup_warnings(warnings);
        let total = items.len() as u64;
        // 百科不可用/失败查询不入缓存（空结果与失败可立即重试）；缓存保存全量列表
        let cacheable = encyclopedia.warnings.is_empty() && !warnings.iter().any(|w| w.contains("查询失败"));
        if cacheable {
            alias_cache_put(key, items.clone(), warnings.clone());
        }
        let paged: Vec<Value> = items.into_iter().skip(q.offset as usize).take(q.limit as usize).collect();
        return Ok(Page { items: paged, total, offset: q.offset, limit: q.limit, warnings });
    }
    builtin_provider_search(app, source, q)
}

/// builtinProviderSearch()：中文别名检索（别名仅补充来源标识完全匹配的项目）
fn builtin_provider_search(app: &tauri::AppHandle, source: &str, q: &Query) -> Result<Page, String> {
    let terms = if q.kind == "mod" { chinese_mod_search_terms(&q.keyword) } else { Vec::new() };
    if terms.is_empty() {
        return raw_provider_search(app, source, q);
    }
    let key = q.cache_key(source, false);
    if let Some(catalog) = alias_cache_get(&key) {
        let total = catalog.items.len() as u64;
        let items: Vec<Value> = catalog.items.into_iter().skip(q.offset as usize).take(q.limit as usize).collect();
        return Ok(Page { items, total, offset: q.offset, limit: q.limit, warnings: catalog.warnings });
    }
    let mut searches: Vec<String> = vec![q.keyword.clone()];
    for t in &terms {
        if !searches.contains(t) {
            searches.push(t.clone());
        }
    }
    let mut successes = 0usize;
    let mut pages: Vec<Page> = Vec::new();
    let mut failures: Vec<String> = Vec::new();
    for keyword in &searches {
        match raw_provider_search(app, source, &q.clone_with(Some(keyword.as_str()), 0, 50)) {
            Ok(first) => {
                successes += 1;
                let mut warnings: Vec<String> = Vec::new();
                let mut items = first.items.clone();
                if first.total > 50 {
                    match raw_provider_search(app, source, &q.clone_with(Some(keyword.as_str()), 50, 50)) {
                        Ok(second) => items.extend(second.items),
                        Err(_) => warnings.push(format!("“{keyword}”的后续结果查询失败，已保留已读取项目；可重试。")),
                    }
                }
                if first.total > 100 {
                    warnings.push(format!("“{keyword}”匹配较多，本次中文别名检索仅核对前 100 项；可用具体英文名搜索完整结果。"));
                }
                let filtered = if keyword == &q.keyword {
                    items
                } else {
                    items
                        .into_iter()
                        .filter(|item| item.get("slug").and_then(|v| v.as_str()).unwrap_or("").to_lowercase() == keyword.to_lowercase())
                        .collect()
                };
                pages.push(Page { items: filtered, total: first.total, offset: first.offset, limit: first.limit, warnings });
            }
            Err(e) => {
                failures.push(e);
                pages.push(Page {
                    items: Vec::new(),
                    total: 0,
                    offset: 0,
                    limit: 0,
                    warnings: vec![if keyword == &q.keyword {
                        "原中文关键词查询失败，当前仅显示已核对来源标识的别名项目；可重试。".into()
                    } else {
                        format!("中文别名“{keyword}”查询失败，已保留其他已读取结果；可重试。")
                    }],
                });
            }
        }
    }
    if successes == 0 {
        return Err(failures.first().cloned().unwrap_or_else(|| "社区来源查询失败，请稍后重试".into()));
    }
    let items = dedup_items(pages.iter().flat_map(|p| p.items.iter().cloned()).collect());
    let header = format!(
        "中文别名检索：{} → {}；保留原中文结果，仅补充来源标识完全匹配的项目。",
        q.keyword,
        terms.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("、")
    );
    let mut warnings = vec![header];
    warnings.extend(pages.iter().flat_map(|p| p.warnings.iter().cloned()));
    let warnings = dedup_warnings(warnings);
    let total = items.len() as u64;
    if !warnings.iter().any(|w| w.contains("查询失败")) {
        alias_cache_put(key, items.clone(), warnings.clone());
    }
    let paged: Vec<Value> = items.into_iter().skip(q.offset as usize).take(q.limit as usize).collect();
    Ok(Page { items: paged, total, offset: q.offset, limit: q.limit, warnings })
}

/// withZhTitle()：slug 命中映射表时给标题加中文名前缀
fn with_zh_title(list: Vec<Value>) -> Vec<Value> {
    list.into_iter()
        .map(|r| {
            let title = r.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let had_original = r.get("originalTitle").and_then(|v| v.as_str()).is_some();
            let original = r.get("originalTitle").and_then(|v| v.as_str()).unwrap_or(&title).to_string();
            let slug = r.get("slug").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let mut out = r;
            if let Some(obj) = out.as_object_mut() {
                obj.insert("originalTitle".into(), json!(original));
                if let Some(zh) = mod_zh_lookup(&slug) {
                    if !title.starts_with(zh) && !(had_original && title != original) {
                        obj.insert("title".into(), json!(format!("{zh} | {title}")));
                    }
                }
            }
            out
        })
        .collect()
}

struct SourceCounts {
    map: HashMap<String, (u64, u64)>, // key -> (total, at)
}

fn source_counts() -> &'static Mutex<SourceCounts> {
    static M: OnceLock<Mutex<SourceCounts>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(SourceCounts { map: HashMap::new() }))
}

fn covers(pages: &HashMap<&'static str, (u64, u64, Vec<Value>)>, source: &str, own: &[Slot]) -> bool {
    if own.is_empty() {
        return true;
    }
    match pages.get(source) {
        Some((offset, limit, _)) => own[0].index >= *offset && own[own.len() - 1].index < offset + limit,
        None => false,
    }
}

/// communitySearchPage()：单源直查；all = 双源计数 + 交错分页 + 存活源重规划
pub fn community_search_page(app: &tauri::AppHandle, input: &Value) -> Result<Value, String> {
    let mut q = Query::from_value(input);
    let filter = effective_filter(&FileFilter {
        mc_version: q.mc_version.clone(),
        loader: q.loader.clone(),
        kind: Some(q.kind.clone()),
    });
    q.mc_version = filter.mc_version;
    q.loader = filter.loader;
    q.keyword = q.keyword.trim().to_string();
    q.limit = q.limit.clamp(1, 50);
    if q.source != "all" {
        let page = provider_search(app, &q.source, &q)?;
        let mut out = page.to_json();
        if let Some(obj) = out.as_object_mut() {
            let items = obj.remove("items").unwrap_or(json!([]));
            obj.insert("items".into(), Value::Array(with_zh_title(items.as_array().cloned().unwrap_or_default())));
        }
        return Ok(out);
    }
    let sources: [&'static str; 2] = ["modrinth", "curseforge"];
    let mut warnings: Vec<String> = Vec::new();
    let mut totals: HashMap<&'static str, u64> = HashMap::new();
    let mut first_failure: Option<String> = None;
    for source in sources {
        let key = q.cache_key(source, false);
        let mut used_cache = false;
        if q.offset > 0 {
            let cached = {
                let sc = source_counts().lock().unwrap();
                sc.map.get(&key).filter(|(_, at)| crate::download::now_ms() - *at < 60_000).map(|(t, _)| *t)
            };
            if let Some(t) = cached {
                totals.insert(source, t);
                used_cache = true;
            }
        }
        if used_cache {
            continue;
        }
        source_counts().lock().unwrap().map.remove(&key);
        let page = provider_search(app, source, &q.clone_with(None, 0, 1));
        match page {
            Ok(p) => {
                warnings.extend(p.warnings.iter().cloned());
                totals.insert(source, p.total);
                if p.warnings.is_empty() {
                    source_counts().lock().unwrap().map.insert(key, (p.total, crate::download::now_ms()));
                } else {
                    source_counts().lock().unwrap().map.remove(&key);
                }
            }
            Err(e) => {
                source_counts().lock().unwrap().map.remove(&key);
                if first_failure.is_none() {
                    first_failure = Some(e);
                }
            }
        }
    }
    let failed: Vec<&'static str> = sources.iter().copied().filter(|s| !totals.contains_key(s)).collect();
    if failed.len() == sources.len() {
        return Err(first_failure.unwrap_or_else(|| "社区来源查询失败，请稍后重试".into()));
    }
    for source in failed {
        warnings.push(format!(
            "{} 暂不可用，当前仅统计另一来源；可重试或切换来源。",
            if source == "modrinth" { "Modrinth" } else { "CurseForge" }
        ));
    }
    totals.entry("modrinth").or_insert(0);
    totals.entry("curseforge").or_insert(0);
    let mut pages: HashMap<&'static str, (u64, u64, Vec<Value>)> = HashMap::new();
    for _round in 0..=sources.len() {
        let planned = community_page_slots(totals["modrinth"], totals["curseforge"], q.offset, q.limit);
        let mut needed: Vec<(&'static str, Vec<Slot>)> = Vec::new();
        for source in sources {
            let own: Vec<Slot> = planned.iter().copied().filter(|s| s.source == source).collect();
            if !covers(&pages, source, &own) {
                needed.push((source, own));
            }
        }
        if needed.is_empty() {
            break;
        }
        for (source, own) in needed {
            let fetch_q = q.clone_with(None, own[0].index, own.len() as u64);
            match provider_search(app, source, &fetch_q) {
                Ok(page) => {
                    warnings.extend(page.warnings.iter().cloned());
                    totals.insert(source, page.total);
                    pages.insert(source, (own[0].index, own.len() as u64, page.items));
                }
                Err(e) => {
                    totals.insert(source, 0);
                    pages.remove(source);
                    warnings.push(format!(
                        "{} 结果读取失败，当前仅统计另一来源；可重试或切换来源。",
                        if source == "modrinth" { "Modrinth" } else { "CurseForge" }
                    ));
                    source_counts().lock().unwrap().map.remove(&q.cache_key(source, false));
                    if failed_all(&pages, &sources) {
                        return Err(e);
                    }
                }
            }
        }
    }
    let slots = community_page_slots(totals["modrinth"], totals["curseforge"], q.offset, q.limit);
    for source in sources {
        let own: Vec<Slot> = slots.iter().copied().filter(|s| s.source == source).collect();
        if !covers(&pages, source, &own) {
            return Err("社区来源结果在翻页时持续变化，请重新搜索后重试。".into());
        }
    }
    let items: Vec<Value> = slots
        .iter()
        .filter_map(|slot| {
            let page = pages.get(slot.source)?;
            page.2.get((slot.index - page.0) as usize).cloned()
        })
        .collect();
    let total = totals["modrinth"] + totals["curseforge"];
    let mut out = serde_json::Map::new();
    out.insert("items".into(), Value::Array(with_zh_title(items)));
    out.insert("total".into(), json!(total));
    out.insert("offset".into(), json!(q.offset));
    out.insert("limit".into(), json!(q.limit));
    let warnings = dedup_warnings(warnings);
    if !warnings.is_empty() {
        out.insert("warnings".into(), json!(warnings));
    }
    Ok(Value::Object(out))
}

fn failed_all(pages: &HashMap<&'static str, (u64, u64, Vec<Value>)>, sources: &[&'static str]) -> bool {
    sources.iter().all(|s| !pages.contains_key(*s))
}

// ---------------- 项目详情（communityProject / communityModProject） ----------------

fn project_text(value: Option<&Value>, limit: usize) -> Option<String> {
    let s = value.and_then(|v| v.as_str())?.trim();
    if s.is_empty() {
        return None;
    }
    Some(s.chars().take(limit).collect())
}

fn project_count(value: Option<&Value>) -> Option<u64> {
    value.and_then(|v| v.as_u64())
}

fn project_webpage(value: Option<&str>, source: &str) -> Option<String> {
    let raw = value?;
    let url = parse_url(raw).ok()?;
    if url.scheme != "https:" || !url.username.is_empty() || !url.password.is_empty() || !url.port.is_empty() {
        return None;
    }
    let allowed: &[&str] = if source == "modrinth" {
        &["modrinth.com", "www.modrinth.com"]
    } else {
        &["curseforge.com", "www.curseforge.com"]
    };
    if allowed.contains(&url.host.as_str()) {
        return Some(raw.to_string());
    }
    None
}

/// favoriteIconUrl()（shared/modFavorites.ts）：仅接受无凭证 https 远程图
pub fn favorite_icon_url(value: Option<&Value>) -> Option<String> {
    let s = value.and_then(|v| v.as_str())?;
    if s.len() > 2048 {
        return None;
    }
    let url = parse_url(s).ok()?;
    if url.scheme == "https:" && url.username.is_empty() && url.password.is_empty() {
        return Some(s.to_string());
    }
    None
}

/// communityProject()：MOD 项目元数据（收藏/下载页校验来源与项目类型）
pub fn community_project(app: &tauri::AppHandle, source: &str, project_id: &str, kind: &str) -> Result<Value, String> {
    if kind != "mod" {
        return Err("收藏详情仅支持 MOD 项目".into());
    }
    static MR_ID: OnceLock<regex::Regex> = OnceLock::new();
    static CF_ID: OnceLock<regex::Regex> = OnceLock::new();
    if source == "modrinth" {
        let id_ok = MR_ID.get_or_init(|| regex::Regex::new(r"^[a-zA-Z0-9_-]{1,100}$").unwrap()).is_match(project_id);
        if id_ok {
            let project = mr_fetch(&format!("/project/{}", enc(project_id)))?;
            let ptype = project.get("project_type").and_then(|v| v.as_str()).unwrap_or("");
            let id = project.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let title = project.get("title").and_then(|v| v.as_str()).unwrap_or("");
            if ptype != "mod"
                || id.is_empty()
                || !MR_ID.get_or_init(|| regex::Regex::new(r"^[a-zA-Z0-9_-]{1,100}$").unwrap()).is_match(id)
                || title.is_empty()
            {
                return Err("该 Modrinth 项目不是有效模组".into());
            }
            let slug = project_text(project.get("slug"), 100).unwrap_or_default();
            let license = project
                .get("license")
                .map(|l| l.get("name").filter(|v| v.as_str().map(|s| !s.is_empty()).unwrap_or(false)).or_else(|| l.get("id")).cloned())
                .unwrap_or(None);
            let categories: Vec<String> = project
                .get("categories")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|c| project_text(Some(c), 100)).take(30).collect())
                .unwrap_or_default();
            let webpage = project_webpage(Some(&format!("https://modrinth.com/mod/{}", if slug.is_empty() { id } else { &slug })), source);
            let mut obj = serde_json::Map::new();
            obj.insert("kind".into(), json!("mod"));
            obj.insert("source".into(), json!(source));
            obj.insert("projectId".into(), json!(id));
            obj.insert("title".into(), json!(title.chars().take(200).collect::<String>()));
            obj.insert("slug".into(), json!(slug));
            if let Some(icon) = favorite_icon_url(project.get("icon_url")) {
                obj.insert("iconUrl".into(), json!(icon));
            }
            if let Some(d) = project_text(project.get("description"), 4000) {
                obj.insert("description".into(), json!(d));
            }
            if let Some(l) = project_text(license.as_ref(), 200) {
                obj.insert("license".into(), json!(l));
            }
            obj.insert("categories".into(), json!(categories));
            if let Some(c) = project_count(project.get("downloads")) {
                obj.insert("downloads".into(), json!(c));
            }
            if let Some(c) = project_count(project.get("followers")) {
                obj.insert("followers".into(), json!(c));
            }
            if let Some(u) = project_text(project.get("updated"), 100) {
                obj.insert("updatedAt".into(), json!(u));
            }
            if let Some(w) = webpage {
                obj.insert("webpage".into(), json!(w));
            }
            return Ok(Value::Object(obj));
        }
    }
    if source == "curseforge" {
        let id_ok = CF_ID.get_or_init(|| regex::Regex::new(r"^\d{1,20}$").unwrap()).is_match(project_id);
        if id_ok {
            let result = cf_fetch(app, &format!("/mods/{project_id}"))?;
            let project = result.get("data").cloned().unwrap_or(Value::Null);
            let pid = value_to_string_id(project.get("id"));
            let game_id = project.get("gameId").and_then(|v| v.as_u64()).unwrap_or(0);
            let class_id = project.get("classId").and_then(|v| v.as_u64()).unwrap_or(0);
            let name = project.get("name").and_then(|v| v.as_str()).unwrap_or("");
            if pid.as_str() != project_id || game_id != 432 || class_id != u64::from(cf_class_id("mod")) || name.is_empty() {
                return Err("该 CurseForge 项目不是有效 Minecraft 模组".into());
            }
            let author = project
                .get("authors")
                .and_then(|a| a.as_array())
                .map(|a| {
                    let names: Vec<String> = a
                        .iter()
                        .filter_map(|x| project_text(Some(x), 100))
                        .collect();
                    project_text(Some(&Value::String(names.join(", "))), 500)
                })
                .unwrap_or(None);
            let categories: Vec<String> = project
                .get("categories")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|c| c.get("name").and_then(|n| n.as_str()).map(|s| s.chars().take(100).collect::<String>())).take(30).collect())
                .unwrap_or_default();
            let webpage = project_webpage(
                project.get("links").and_then(|l| l.get("websiteUrl")).and_then(|v| v.as_str()),
                source,
            );
            let mut obj = serde_json::Map::new();
            obj.insert("kind".into(), json!("mod"));
            obj.insert("source".into(), json!(source));
            obj.insert("projectId".into(), json!(project_id));
            obj.insert("title".into(), json!(name.chars().take(200).collect::<String>()));
            if let Some(icon) = favorite_icon_url(project.get("logo").and_then(|l| l.get("thumbnailUrl"))) {
                obj.insert("iconUrl".into(), json!(icon));
            }
            if let Some(s) = project_text(project.get("slug"), 100) {
                obj.insert("slug".into(), json!(s));
            }
            if let Some(d) = project_text(project.get("summary"), 4000) {
                obj.insert("description".into(), json!(d));
            }
            if let Some(a) = author {
                obj.insert("author".into(), json!(a));
            }
            obj.insert("categories".into(), json!(categories));
            if let Some(c) = project_count(project.get("downloadCount")) {
                obj.insert("downloads".into(), json!(c));
            }
            if let Some(u) = project_text(project.get("dateModified"), 100) {
                obj.insert("updatedAt".into(), json!(u));
            }
            if let Some(w) = webpage {
                obj.insert("webpage".into(), json!(w));
            }
            return Ok(Value::Object(obj));
        }
    }
    Err("请填写有效的来源项目 ID".into())
}

/// communityModProject()：收藏关联用的轻量身份
pub fn community_mod_project(app: &tauri::AppHandle, source: &str, project_id: &str) -> Result<Value, String> {
    let project = community_project(app, source, project_id, "mod")?;
    let mut obj = serde_json::Map::new();
    obj.insert("source".into(), json!(project.get("source").cloned().unwrap_or(json!(source))));
    obj.insert("projectId".into(), json!(project.get("projectId").cloned().unwrap_or(json!(project_id))));
    obj.insert("name".into(), json!(project.get("title").cloned().unwrap_or(json!(""))));
    if let Some(icon) = project.get("iconUrl").filter(|v| v.as_str().map(|s| !s.is_empty()).unwrap_or(false)) {
        obj.insert("iconUrl".into(), icon.clone());
    }
    Ok(Value::Object(obj))
}

/// curseForgeFilePage()：文件页网址（modpack:openFile 由 modpacks.rs 调用）
pub fn curse_forge_file_page(app: &tauri::AppHandle, project_id: u64, file_id: u64) -> Result<String, String> {
    if project_id == 0 || file_id == 0 {
        return Err("CurseForge 文件标识无效".into());
    }
    let ch = cf_channel(app);
    let sources: Vec<(&str, Option<&String>)> = vec![(CF_MIRROR, None), (ch.base, if ch.official { Some(&ch.key) } else { None })];
    for (base, key) in sources {
        let mut headers: Vec<(&str, &str)> = Vec::new();
        if let Some(k) = key {
            headers.push(("x-api-key", k));
        }
        let Ok(data) = fetch_json(&format!("{base}/mods/{project_id}"), &headers, 6_000) else { continue };
        let Some(web) = data.pointer("/data/links/websiteUrl").and_then(|v| v.as_str()) else { continue };
        let Ok(url) = parse_url(web) else { continue };
        if url.scheme != "https:" || !["curseforge.com", "www.curseforge.com"].contains(&url.host.as_str()) {
            continue;
        }
        let mut path = url.path.trim_end_matches('/').to_string();
        path.push_str(&format!("/download/{file_id}"));
        return Ok(format!("https://{}{}", url.host, path));
    }
    Err("无法读取 CurseForge 文件页面，请稍后重试".into())
}

// ---------------- 文件列表 / 精确文件 ----------------

/// communityFiles()：项目文件列表（新→旧）
pub fn community_files(app: &tauri::AppHandle, source: &str, project_id: &str, filter: Option<&FileFilter>) -> Result<Vec<Value>, String> {
    let project_id = project_id.to_string();
    let filter = effective_filter(&filter.cloned().unwrap_or_default());
    let mut files = if source == "modrinth" {
        mr_files(&project_id, Some(&filter), false)?
    } else {
        cf_files(app, &project_id, Some(&filter))?
    };
    files.retain(|f| matches_community_filter(f, &filter));
    files.sort_by(|a, b| {
        let da = a.get("date").and_then(|v| v.as_str()).unwrap_or("");
        let db = b.get("date").and_then(|v| v.as_str()).unwrap_or("");
        db.cmp(da)
    });
    Ok(files)
}

/// communityExactFile()：依赖解析始终使用精确仓库身份
pub fn community_exact_file(app: &tauri::AppHandle, source: &str, project_id: Option<&str>, file_id: &str) -> Result<Value, String> {
    let files = if source == "modrinth" {
        let version = mr_fetch(&format!("/version/{}", enc(file_id)))?;
        map_mr_versions(std::slice::from_ref(&version), project_id, false)
    } else {
        let data = cf_fetch(app, &format!("/mods/{}/files/{}", enc(project_id.unwrap_or("")), enc(file_id)))?;
        let file = data.get("data").cloned().unwrap_or(Value::Null);
        map_cf_files(std::slice::from_ref(&file), project_id.unwrap_or(""))
    };
    files.into_iter().next().ok_or_else(|| "依赖版本没有可下载文件".to_string())
}

/// communityFavoriteCandidates()：收藏版本查询（Modrinth 保留无直链文件以区分不兼容与不可靠）
pub fn community_favorite_candidates(app: &tauri::AppHandle, source: &str, project_id: &str, mc_version: &str, loader: &str) -> Result<Vec<Value>, String> {
    let filter = FileFilter { mc_version: Some(mc_version.to_string()), loader: Some(loader.to_string()), kind: Some("mod".into()) };
    let mut files = if source == "modrinth" {
        mr_files(project_id, Some(&filter), true)?
    } else {
        cf_files(app, project_id, Some(&filter))?
    };
    files.retain(|f| matches_community_filter(f, &filter));
    files.sort_by(|a, b| {
        let da = a.get("date").and_then(|v| v.as_str()).unwrap_or("");
        let db = b.get("date").and_then(|v| v.as_str()).unwrap_or("");
        db.cmp(da)
    });
    Ok(files)
}

/// communitySearch()（依赖查找数组接口）：communitySearchPage().items
pub fn community_search_items(app: &tauri::AppHandle, q: &Value) -> Result<Vec<Value>, String> {
    let page = community_search_page(app, q)?;
    Ok(page.get("items").and_then(|v| v.as_array()).cloned().unwrap_or_default())
}

// ---------------- downloadFileName.ts ----------------

const RESERVED_DEVICE: &str = r"^(?:con|prn|aux|nul|clock\$|conin\$|conout\$|com[1-9¹²³]|lpt[1-9¹²³])(?:\.|$)";
const MAX_NAME_BYTES: usize = 200;

fn trim_to_bytes(value: &str, budget: usize) -> String {
    let mut result = String::new();
    let mut bytes = 0usize;
    for c in value.chars() {
        let len = c.len_utf8();
        if bytes + len > budget {
            break;
        }
        result.push(c);
        bytes += len;
    }
    result
}

/// downloadFileName()：来源文件名是标签不是路径；非法字符替换 + 稳定后缀防覆盖
pub fn download_file_name(input: &str) -> Result<String, String> {
    let original = input;
    if original.contains('/') || original.contains('\\') || original.trim() == "." || original.trim() == ".." {
        return Err("下载文件名不能包含目录或路径跳转".into());
    }
    if original.is_empty() {
        return Ok("download.bin".into());
    }
    let mut safe: String = original
        .chars()
        .map(|c| if matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*' | '\u{0}'..='\u{1f}' | '\u{7f}') { '_' } else { c })
        .collect();
    while safe.ends_with('.') || safe.ends_with(' ') {
        safe.pop();
    }
    if safe.is_empty() {
        safe = "download".into();
    }
    static DEVICE: OnceLock<regex::Regex> = OnceLock::new();
    let device_re = DEVICE.get_or_init(|| regex::Regex::new(RESERVED_DEVICE).unwrap());
    let first_seg = safe.split('.').next().unwrap_or("").trim_end().to_string();
    if device_re.is_match(&first_seg) {
        safe = format!("_{safe}");
    }
    if safe == original && safe.len() <= MAX_NAME_BYTES {
        return Ok(safe);
    }
    use sha2::{Digest, Sha256};
    let digest = hex::encode(Sha256::digest(original.as_bytes()))[..12].to_string();
    // path.extname 语义：首个点之后整段（.hidden 无扩展名）
    let ext_os = match safe.find('.') {
        Some(i) if i > 0 => safe[i..].to_string(),
        _ => String::new(),
    };
    let extension = if ext_os.len() <= 32 { ext_os } else { String::new() };
    let suffix = format!("-{digest}{extension}");
    let stem = if extension.is_empty() { safe.clone() } else { safe[..safe.len() - extension.len()].to_string() };
    let mut stem = trim_to_bytes(&stem, MAX_NAME_BYTES.saturating_sub(suffix.len()));
    while stem.ends_with('.') || stem.ends_with(' ') {
        stem.pop();
    }
    if stem.is_empty() {
        stem = "download".into();
    }
    Ok(format!("{stem}{suffix}"))
}

// ---------------- 下载（communityDownload） ----------------

/// kind → 游戏目录下的子目录
fn kind_subdir(kind: &str) -> Option<&'static str> {
    match kind {
        "mod" => Some("mods"),
        "resourcepack" => Some("resourcepacks"),
        "shader" => Some("shaderpacks"),
        "datapack" => Some("datapacks"),
        _ => None,
    }
}

/// 任务进度发射器（对应 ipc.ts community:download 的 taskEmit：
/// ProgressEventGuard 归一化 + taskId/taskTitle/operationId 合并 + lastStage 记账）
struct DownloadEmit {
    app: tauri::AppHandle,
    guard: Mutex<ProgressGuard>,
    task_id: String,
    task_title: String,
    operation_id: Option<String>,
    last_stage: Arc<Mutex<String>>,
}

impl DownloadEmit {
    fn emit(&self, event: &Value) {
        let mut g = self.guard.lock().unwrap();
        let normalized = g.normalize(event);
        let stage = normalized.get("stage").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if stage != "error" && stage != "done" {
            *self.last_stage.lock().unwrap() = stage;
        }
        let mut payload = normalized;
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("taskId".into(), json!(self.task_id));
            obj.insert("taskTitle".into(), json!(self.task_title));
            if let Some(op) = &self.operation_id {
                obj.insert("operationId".into(), json!(op));
            }
        }
        tasks::emit_progress(&self.app, payload);
    }
}

/// 单文件传输 + 速度估计（对应 communityDownloadInFolder 的 transfer + dlProgress）
#[allow(clippy::too_many_arguments)]
fn transfer(
    app: &tauri::AppHandle,
    rec: &Arc<TaskRecord>,
    emit: &DownloadEmit,
    url: &str,
    sha1: Option<&str>,
    size: Option<u64>,
    dest: &Path,
    display_name: &str,
    modpack: bool,
) -> Result<(), String> {
    let mirror = download::mirror_from_settings(app);
    let est = Mutex::new(SmoothedSpeedEstimator::new());
    let bytes = AtomicU64::new(0);
    let start = Instant::now();
    let paused_flag = rec.paused.clone();
    let display = display_name.to_string();
    let progress = move |done: u64, total: u64, wire: u64| {
        bytes.fetch_add(wire, Ordering::SeqCst);
        let remaining = if total > done { Some(total - done) } else { None };
        let snap = est.lock().unwrap().sample(bytes.load(Ordering::SeqCst), remaining, start.elapsed().as_millis() as f64, paused_flag.load(Ordering::SeqCst));
        let frac = if total > 0 { done as f64 / total as f64 } else { 0.0 };
        let text = format!(
            "下载 {} {:.1}MB{}",
            display,
            done as f64 / 1024.0 / 1024.0,
            if total > 0 { format!("/{:.1}MB", total as f64 / 1024.0 / 1024.0) } else { String::new() }
        );
        let mut ev = json!({
            "stage": "download",
            "progress": frac,
            // 压缩包只是整合包任务的第一步；不能先报 100% 再开始安装。
            "overall": if modpack { frac * 0.1 } else { frac },
            "speed": snap.speed_bps,
            "bytesDone": done,
            "indeterminate": total == 0,
            "text": text,
        });
        if total > 0 {
            ev["bytesTotal"] = json!(total);
        }
        if let Some(eta) = snap.eta_seconds {
            ev["etaSeconds"] = json!(eta);
        }
        emit.emit(&ev);
    };
    let opts = DownloadFileOpts {
        sha1,
        sha512: None,
        sha256: None,
        size,
        alternatives: &[],
        reuse_dirs: &[],
        reuse_files: &[],
        mirror,
        max_attempts: 4,
        source_pool: None,
        progress: Some(&progress),
        on_wait: None,
    };
    download::download_file(app, rec, url, dest, &opts)
}

// ---------------- 整合包安装接线（modpacks.rs 集成点） ----------------

/// 整合包下载完成后的安装作业包。modpacks.rs 移植后由集成方通过
/// [`set_modpack_install_hook`] 注册真实安装实现；hook 必须在安装结束（无论成败）时
/// 调用一次 `finish`，由其负责临时目录清理、event:taskDone / event:installDone 推送
/// 与任务注销。对应原版 community.ts communityDownloadInFolder 中
/// `const { installModpack } = await import('./modpacks')` 的后台安装。
pub struct ModpackInstallJob {
    pub app: tauri::AppHandle,
    pub task_id: String,
    pub task_title: String,
    /// 下载完成的整合包临时文件（临时目录内的 pack 文件名已规范化）
    pub pack_path: PathBuf,
    /// 展示名（原始 fileName）
    pub display_name: String,
    /// target.folder 原样透传（None = 默认下载文件夹）
    pub target_folder: Option<String>,
    /// 安装进度发射器：已附 taskId/taskTitle/operationId，overall 自动映射 0.1+0.9x
    pub emit_install: Arc<dyn Fn(&Value) + Send + Sync>,
    /// 安装结束回调：Ok(新实例 versionId) / Err(错误文案，"已取消" 会被识别)
    pub finish: Box<dyn FnOnce(Result<String, String>) + Send>,
}

type ModpackHook = Arc<dyn Fn(ModpackInstallJob) + Send + Sync>;

static MODPACK_HOOK: Mutex<Option<ModpackHook>> = Mutex::new(None);

/// 注册整合包安装钩子（modpacks.rs 就绪后由集成方调用一次）
pub fn set_modpack_install_hook(hook: ModpackHook) {
    *MODPACK_HOOK.lock().unwrap() = Some(hook);
}

fn run_modpack_job(job: ModpackInstallJob) {
    let hook = MODPACK_HOOK.lock().unwrap().clone();
    match hook {
        Some(hook) => hook(job),
        None => {
            // 默认实现：整合包安装模块未接入——下载已成功但安装无法进行，
            // 以失败收尾避免任务悬挂（集成方注册钩子后此分支不再触达）。
            (job.finish)(Err("整合包安装流程尚未接入，请更新启动器后重试".into()));
        }
    }
}

/// communityDownloadInFolder()：下载落盘 + modpack 后台安装
fn community_download_in_folder(
    app: &tauri::AppHandle,
    rec: &Arc<TaskRecord>,
    emit: &DownloadEmit,
    file: &Value,
    target: &Value,
    folder: &str,
) -> Result<String, String> {
    let check_cancel = || -> Result<(), String> {
        if rec.cancelled.load(Ordering::SeqCst) {
            Err("已取消".into())
        } else {
            Ok(())
        }
    };
    check_cancel()?;
    let raw_name = file.get("fileName").and_then(|v| v.as_str()).unwrap_or("");
    let file_name = download_file_name(raw_name)?;
    let display_name = if raw_name.is_empty() { file_name.clone() } else { raw_name.to_string() };
    let kind = target.get("kind").and_then(|v| v.as_str()).unwrap_or("mod").to_string();
    let modpack = kind == "modpack";

    // CurseForge 受限文件（作者禁止直链，downloadUrl 为 null）：官方 API 现场解析真实下载地址
    let mut url = file.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if file.get("source").and_then(|v| v.as_str()) == Some("curseforge") && url.is_empty() {
        let project_id = file.get("projectId").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if project_id.is_empty() {
            return Err("缺少 CurseForge 项目 ID，请重新选择下载文件".into());
        }
        let ch = cf_channel(app);
        if !ch.official {
            return Err("该文件作者限制了直链下载，需要在设置页填入 CurseForge API Key 后才能下载".into());
        }
        let file_id = file.get("fileId").and_then(|v| v.as_str()).unwrap_or("");
        let data = fetch_json(
            &format!("{}/mods/{}/files/{}/download-url", ch.base, enc(&project_id), enc(file_id)),
            &[("x-api-key", &ch.key)],
            TIMEOUT_MS,
        )?;
        url = data
            .get("data")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "CurseForge 未返回下载地址".to_string())?
            .to_string();
    }

    let sha1 = file.get("sha1").and_then(|v| v.as_str()).map(str::to_string);
    let size = file.get("size").and_then(|v| v.as_u64()).filter(|s| *s > 0);
    let url2 = url.clone();

    if modpack {
        // 独占临时目录避免并发下载冲突，并约束 .part 清理范围
        let tmp_root = std::env::temp_dir().join(format!("kamucl-pack-{}", crate::accounts::uuid_v4()));
        std::fs::create_dir_all(&tmp_root).map_err(|e| e.to_string())?;
        let tmp_path = tmp_root.join(&file_name);
        let transferred = transfer(app, rec, emit, &url2, sha1.as_deref(), size, &tmp_path, &display_name, true)
            .and_then(|_| check_cancel());
        if let Err(e) = transferred {
            let _ = std::fs::remove_dir_all(&tmp_root);
            return Err(e);
        }
        let app2 = app.clone();
        let emit_shared: Arc<dyn Fn(&Value) + Send + Sync> = {
            let app2 = app2.clone();
            let task_id = rec.id.clone();
            let task_title = rec.title.clone();
            let operation_id = emit.operation_id.clone();
            let guard = Mutex::new(ProgressGuard::new());
            let last_stage = emit.last_stage.clone();
            Arc::new(move |event: &Value| {
                // 对应原版 installProgress：overall = 0.1 + (overall ?? progress) * 0.9
                let mut mapped = event.clone();
                if let Some(obj) = mapped.as_object_mut() {
                    let base = obj.get("overall").and_then(|v| v.as_f64()).or_else(|| obj.get("progress").and_then(|v| v.as_f64())).unwrap_or(0.0);
                    obj.insert("overall".into(), json!(0.1 + base * 0.9));
                }
                let mut g = guard.lock().unwrap();
                let normalized = g.normalize(&mapped);
                let stage = normalized.get("stage").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if stage != "error" && stage != "done" {
                    *last_stage.lock().unwrap() = stage;
                }
                let mut payload = normalized;
                if let Some(obj) = payload.as_object_mut() {
                    obj.insert("taskId".into(), json!(task_id));
                    obj.insert("taskTitle".into(), json!(task_title));
                    if let Some(op) = &operation_id {
                        obj.insert("operationId".into(), json!(op));
                    }
                }
                tasks::emit_progress(&app2, payload);
            })
        };
        let finish = {
            let app2 = app2.clone();
            let task_id = rec.id.clone();
            let last_stage = emit.last_stage.clone();
            let emit_ref = emit_shared.clone();
            Box::new(move |outcome: Result<String, String>| {
                // finally cleanup（对应原版 cleanup()，删除整个独占临时目录）
                let _ = std::fs::remove_dir_all(&tmp_root);
                let stage = last_stage.lock().unwrap().clone();
                match outcome {
                    Ok(version_id) => {
                        tasks::task_done(&app2, &task_id, true, None, false, None);
                        installer::emit_event(&app2, "event:installDone", &json!({
                            "versionId": version_id,
                            "ok": true,
                            "taskId": task_id,
                            "cancelled": false
                        }));
                    }
                    Err(e) => {
                        let cancelled = e == "已取消";
                        emit_ref(&json!({ "stage": "error", "progress": 0, "text": format!("整合包安装失败: {e}") }));
                        tasks::task_done(&app2, &task_id, false, Some(e.clone()), cancelled, if stage.is_empty() { None } else { Some(stage.clone()) });
                        let mut obj = serde_json::Map::new();
                        obj.insert("versionId".into(), json!(""));
                        obj.insert("ok".into(), json!(false));
                        obj.insert("error".into(), json!(e));
                        obj.insert("taskId".into(), json!(task_id));
                        obj.insert("cancelled".into(), json!(cancelled));
                        if !stage.is_empty() {
                            obj.insert("stage".into(), json!(stage));
                        }
                        installer::emit_event(&app2, "event:installDone", &Value::Object(obj));
                    }
                }
                tasks::finish_task(&task_id);
            }) as Box<dyn FnOnce(Result<String, String>) + Send>
        };
        let job = ModpackInstallJob {
            app: app2,
            task_id: rec.id.clone(),
            task_title: rec.title.clone(),
            pack_path: tmp_path,
            display_name,
            target_folder: target.get("folder").and_then(|v| v.as_str()).map(str::to_string),
            emit_install: emit_shared,
            finish,
        };
        std::thread::spawn(move || run_modpack_job(job));
        return Ok("整合包已开始安装".into());
    }

    // 与最终启动使用同一个目录解析器，避免资源被装进未参与启动的目录。
    let version_id = target.get("versionId").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let version_json = installer::read_version_json(app, folder, &version_id)?;
    let base = installer::instance_directory_state(app, folder, &version_id, &version_json)
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // 数据包：MC 只从 saves/<世界>/datapacks 加载——唯一存档直接投入，否则落 gameDir/datapacks 并提示
    if kind == "datapack" {
        let saves_dir = Path::new(&base).join("saves");
        let mut worlds: Vec<String> = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&saves_dir) {
            for entry in rd.flatten() {
                let ok = entry.file_type().map(|t| t.is_dir()).unwrap_or(false)
                    && entry.path().join("level.dat").exists();
                if ok {
                    worlds.push(entry.file_name().to_string_lossy().to_string());
                }
            }
        }
        if worlds.len() == 1 {
            let dest = saves_dir.join(&worlds[0]).join("datapacks").join(&file_name);
            transfer(app, rec, emit, &url2, sha1.as_deref(), size, &dest, &display_name, false)?;
            return Ok(dest.to_string_lossy().into_owned());
        }
        let dest = Path::new(&base).join("datapacks").join(&file_name);
        transfer(app, rec, emit, &url2, sha1.as_deref(), size, &dest, &display_name, false)?;
        return Ok(format!("{}（提示：请将文件移入存档 saves/<世界>/datapacks 后生效）", dest.to_string_lossy()));
    }

    let sub = kind_subdir(&kind).ok_or_else(|| format!("不支持的资源类型: {kind}"))?;
    let dest = Path::new(&base).join(sub).join(&file_name);
    transfer(app, rec, emit, &url2, sha1.as_deref(), size, &dest, &display_name, false)?;
    Ok(dest.to_string_lossy().into_owned())
}

/// communityDownload() 的 IPC 包装：registerTask + 冻结目录 + 同步等待结果
fn community_download_ipc(app: &tauri::AppHandle, args: &[Value]) -> Result<Value, String> {
    let file = arg(args, 0).cloned().unwrap_or(Value::Null);
    let target = arg(args, 1).cloned().unwrap_or(Value::Null);
    let operation_id = arg(args, 2)
        .and_then(|v| v.as_str())
        .filter(|s| regex::Regex::new(r"^[a-zA-Z0-9:_-]{1,96}$").unwrap().is_match(s))
        .map(str::to_string);
    let file_name = file.get("fileName").and_then(|v| v.as_str()).unwrap_or("资源");
    let rec = tasks::register_task(&format!("下载 {file_name}"), "download");
    let kind = target.get("kind").and_then(|v| v.as_str()).unwrap_or("mod").to_string();
    let version_id = target.get("versionId").and_then(|v| v.as_str()).unwrap_or("").to_string();

    // Freeze both the instance and shared directory before the first network await.
    let folder_provided = target.get("folder").is_some();
    let requested = if !folder_provided {
        if kind == "modpack" {
            installer::default_folder_path(app)
        } else {
            installer::folder_of_version(app, &version_id)
        }
    } else {
        target.get("folder").and_then(|v| v.as_str()).unwrap_or("").trim().to_string()
    };
    if requested.is_empty() {
        tasks::task_done(app, &rec.id, false, Some("目标游戏文件夹不能为空".into()), false, None);
        return Err("目标游戏文件夹不能为空".into());
    }
    let registered = crate::settings::get(app)
        .get("folders")
        .and_then(|f| f.as_array())
        .and_then(|a| a.iter().find(|f| f.get("path").and_then(|p| p.as_str()).map(|p| installer::same_path(p, &requested)).unwrap_or(false)))
        .and_then(|f| f.get("path").and_then(|p| p.as_str()))
        .map(str::to_string);
    if registered.is_none() && (kind == "modpack" || folder_provided) {
        tasks::task_done(app, &rec.id, false, Some("目标游戏文件夹未在 WUHUICL 中登记".into()), false, None);
        return Err("目标游戏文件夹未在 WUHUICL 中登记".into());
    }
    let folder = installer::canonical_path(registered.as_deref().unwrap_or(&requested)).to_string_lossy().into_owned();

    let emit = Arc::new(DownloadEmit {
        app: app.clone(),
        guard: Mutex::new(ProgressGuard::new()),
        task_id: rec.id.clone(),
        task_title: rec.title.clone(),
        operation_id,
        last_stage: Arc::new(Mutex::new(String::new())),
    });
    emit.emit(&json!({ "stage": "download", "progress": 0, "indeterminate": true, "text": "准备下载资源…" }));

    let app2 = app.clone();
    let rec2 = rec.clone();
    let emit2 = emit.clone();
    let is_modpack = kind == "modpack";
    let (tx, rx) = std::sync::mpsc::channel::<Result<String, String>>();
    std::thread::spawn(move || {
        let result = community_download_in_folder(&app2, &rec2, &emit2, &file, &target, &folder);
        let _ = tx.send(result);
    });
    let result = rx.recv().map_err(|_| "下载任务异常退出".to_string());
    match result {
        Ok(Ok(path)) => {
            if is_modpack {
                // 整合包：invoke 返回即完成受理；taskDone/installDone 由后台安装结束时触发
                return Ok(json!(path));
            }
            tasks::task_done(app, &rec.id, true, None, false, None);
            Ok(json!(path))
        }
        Ok(Err(e)) => {
            let cancelled = download::is_cancel_error(&e) || rec.cancelled.load(Ordering::SeqCst);
            let last_stage = emit.last_stage.lock().unwrap().clone();
            tasks::task_done(
                app,
                &rec.id,
                false,
                Some(if cancelled { "已取消".into() } else { e.clone() }),
                cancelled,
                if last_stage.is_empty() { None } else { Some(last_stage) },
            );
            Err(e)
        }
        Err(e) => Err(e),
    }
}

// ---------------- IPC 入口 ----------------

/// 统一入口：返回 None 表示本模块不认识该通道
pub fn invoke(app: &tauri::AppHandle, channel: &str, args: &[Value]) -> Option<Result<Value, String>> {
    match channel {
        "community:search" => Some(installer::run_blocking(|| community_search_page(app, args.first().unwrap_or(&Value::Null)))),
        "community:project" => Some(installer::run_blocking(|| {
            let source = arg_str(args, 0).unwrap_or("").to_string();
            let project_id = arg(args, 1).map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string())).unwrap_or_default();
            let kind = arg_str(args, 2).unwrap_or("mod").to_string();
            community_project(app, &source, &project_id, &kind)
        })),
        "community:files" => Some(installer::run_blocking(|| {
            let source = arg_str(args, 0).unwrap_or("").to_string();
            let project_id = arg(args, 1).map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string())).unwrap_or_default();
            let filter = filter_from_value(arg(args, 2));
            community_files(app, &source, &project_id, Some(&filter)).map(|list| json!(list))
        })),
        "community:download" => Some(community_download_ipc(app, args)),
        _ => None,
    }
}
