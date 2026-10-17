//! Pure mod archive metadata parsing and directory scanning helpers.

use crate::community::dependency_range;
use crate::net::Sha1;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::io::Read;
use std::path::Path;
use std::sync::OnceLock;

fn strip_bom(text: &str) -> &str {
    text.strip_prefix('\u{FEFF}').unwrap_or(text)
}

// ---------------- modIconIdentity.ts：CurseForge 指纹（MurmurHash2, seed 1, 去空白） ----------------

/// curseFingerprint()
pub fn curse_fingerprint(input: &[u8]) -> u32 {
    let mut length: u32 = 0;
    for &b in input {
        if !matches!(b, 9 | 10 | 13 | 32) {
            length = length.wrapping_add(1);
        }
    }
    const M: u32 = 0x5bd1e995;
    let mut h = 1u32.wrapping_sub(1).wrapping_add(length); // (1 ^ length) >>> 0
    let mut word: u32 = 0;
    let mut shift: u32 = 0;
    for &byte in input {
        if matches!(byte, 9 | 10 | 13 | 32) {
            continue;
        }
        word |= (byte as u32) << shift;
        if shift == 24 {
            word = word.wrapping_mul(M);
            word ^= word >> 24;
            word = word.wrapping_mul(M);
            h = h.wrapping_mul(M) ^ word;
            word = 0;
            shift = 0;
        } else {
            shift += 8;
        }
    }
    if shift != 0 {
        h ^= word;
        h = h.wrapping_mul(M);
    }
    h ^= h >> 13;
    h = h.wrapping_mul(M);
    h ^= h >> 15;
    h
}

// ---------------- modMetadata.ts：jar 内元数据解析 ----------------

const BUILTINS: [&str; 6] = [
    "minecraft",
    "forge",
    "neoforge",
    "fabricloader",
    "quilt_loader",
    "java",
];

pub(super) struct ArchiveEntry {
    bytes: Vec<u8>,
}

pub(super) fn zip_read_entry<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
    max_bytes: u64,
) -> Result<Option<ArchiveEntry>, String> {
    let mut file = match archive.by_name(name) {
        Ok(f) => f,
        Err(_) => return Ok(None),
    };
    if file.size() > max_bytes {
        return Err(format!("{name} 元数据过大"));
    }
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    Ok(Some(ArchiveEntry { bytes: buf }))
}

pub(super) fn zip_read_text<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
) -> Result<Option<String>, String> {
    match zip_read_entry(archive, name, 2 * 1024 * 1024)? {
        Some(e) => Ok(Some(
            String::from_utf8_lossy(&e.bytes)
                .strip_prefix('\u{FEFF}')
                .map(str::to_string)
                .unwrap_or_else(|| String::from_utf8_lossy(&e.bytes).into_owned()),
        )),
        None => Ok(None),
    }
}

/// TOML 子集解析器（mods.toml：表头/数组表/键值/字符串/整数/布尔/数组/多行字符串）
pub(super) fn parse_mods_toml(text: &str) -> Result<Value, String> {
    fn strip_comment(line: &str) -> String {
        let mut out = String::new();
        let mut in_basic = false;
        let mut in_literal = false;
        let mut escaped = false;
        for c in line.chars() {
            if in_basic {
                out.push(c);
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == '"' {
                    in_basic = false;
                }
                continue;
            }
            if in_literal {
                out.push(c);
                if c == '\'' {
                    in_literal = false;
                }
                continue;
            }
            match c {
                '"' => {
                    in_basic = true;
                    out.push(c);
                }
                '\'' => {
                    in_literal = true;
                    out.push(c);
                }
                '#' => break,
                _ => out.push(c),
            }
        }
        out
    }

    fn parse_scalar(s: &str) -> Value {
        let s = s.trim();
        if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
            let inner = &s[1..s.len() - 1];
            let mut out = String::new();
            let mut chars = inner.chars();
            while let Some(c) = chars.next() {
                if c == '\\' {
                    match chars.next() {
                        Some('n') => out.push('\n'),
                        Some('t') => out.push('\t'),
                        Some('r') => out.push('\r'),
                        Some('"') => out.push('"'),
                        Some('\\') => out.push('\\'),
                        Some('u') => {
                            let hex: String = chars.by_ref().take(4).collect();
                            if let Some(cp) =
                                u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32)
                            {
                                out.push(cp);
                            }
                        }
                        Some(other) => {
                            out.push('\\');
                            out.push(other);
                        }
                        None => out.push('\\'),
                    }
                } else {
                    out.push(c);
                }
            }
            return Value::String(out);
        }
        if s.len() >= 2 && s.starts_with('\'') && s.ends_with('\'') {
            return Value::String(s[1..s.len() - 1].to_string());
        }
        if s == "true" {
            return Value::Bool(true);
        }
        if s == "false" {
            return Value::Bool(false);
        }
        if let Ok(i) = s.parse::<i64>() {
            return json!(i);
        }
        if let Ok(f) = s.parse::<f64>() {
            return json!(f);
        }
        Value::String(s.to_string())
    }

    fn split_top_level(s: &str, sep: char) -> Vec<String> {
        let mut out = Vec::new();
        let mut depth = 0i32;
        let mut cur = String::new();
        let mut in_basic = false;
        let mut in_literal = false;
        let mut escaped = false;
        for c in s.chars() {
            if in_basic {
                cur.push(c);
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == '"' {
                    in_basic = false;
                }
                continue;
            }
            match c {
                '\'' => {
                    in_literal = !in_literal;
                    cur.push(c);
                }
                '"' => {
                    in_basic = true;
                    cur.push(c);
                }
                '[' | '{' => {
                    depth += 1;
                    cur.push(c);
                }
                ']' | '}' => {
                    depth -= 1;
                    cur.push(c);
                }
                c if c == sep && depth == 0 => {
                    out.push(cur.clone());
                    cur.clear();
                }
                _ => cur.push(c),
            }
        }
        if !cur.trim().is_empty() || out.is_empty() {
            out.push(cur);
        }
        out
    }

    fn parse_array(s: &str) -> Value {
        let inner = s.trim();
        let inner = &inner[1..inner.len().saturating_sub(1)];
        let mut items = Vec::new();
        for part in split_top_level(inner, ',') {
            let p = part.trim();
            if p.is_empty() {
                continue;
            }
            items.push(parse_scalar(p));
        }
        Value::Array(items)
    }

    fn parse_header_keys(inner: &str) -> Vec<String> {
        split_top_level(inner, '.')
            .into_iter()
            .map(|seg| {
                let seg = seg.trim();
                if seg.len() >= 2 && seg.starts_with('"') && seg.ends_with('"') {
                    seg[1..seg.len() - 1].to_string()
                } else if seg.len() >= 2 && seg.starts_with('\'') && seg.ends_with('\'') {
                    seg[1..seg.len() - 1].to_string()
                } else {
                    seg.to_string()
                }
            })
            .collect()
    }

    let mut root = serde_json::Map::new();
    enum Cursor {
        Table(Vec<String>),
        ArrayElem(Vec<String>),
    }
    let mut cursor = Cursor::Table(Vec::new());

    fn table_at<'a>(
        root: &'a mut serde_json::Map<String, Value>,
        path: &[String],
    ) -> &'a mut serde_json::Map<String, Value> {
        let mut node = root;
        for key in path {
            let entry = node.entry(key.clone()).or_insert_with(|| json!({}));
            if !entry.is_object() {
                *entry = json!({});
            }
            node = entry.as_object_mut().unwrap();
        }
        node
    }

    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0usize;
    while i < lines.len() {
        let raw = strip_comment(lines[i]);
        let line = raw.trim().to_string();
        i += 1;
        if line.is_empty() {
            continue;
        }
        if line.starts_with("[[") && line.ends_with("]]") {
            let path = parse_header_keys(&line[2..line.len() - 2]);
            let parent = &path[..path.len() - 1];
            let last = path[path.len() - 1].clone();
            let node = table_at(&mut root, parent);
            let arr = node.entry(last).or_insert_with(|| json!([]));
            if !arr.is_array() {
                *arr = json!([]);
            }
            arr.as_array_mut().unwrap().push(json!({}));
            cursor = Cursor::ArrayElem(path);
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            let path = parse_header_keys(&line[1..line.len() - 1]);
            table_at(&mut root, &path);
            cursor = Cursor::Table(path);
            continue;
        }
        // key = value（值可能跨行：数组 / 多行字符串）
        let Some(eq) = line.find('=') else { continue };
        let key_part = line[..eq].trim().to_string();
        let mut value_part = line[eq + 1..].trim().to_string();
        let key = {
            let segs = parse_header_keys(&key_part);
            segs.last().cloned().unwrap_or_default()
        };
        // 多行数组 / 多行字符串补齐
        let balance = |s: &str| -> i32 {
            let mut d = 0i32;
            let mut in_basic = false;
            let mut in_literal = false;
            let mut escaped = false;
            for c in s.chars() {
                if in_basic {
                    if escaped {
                        escaped = false;
                    } else if c == '\\' {
                        escaped = true;
                    } else if c == '"' {
                        in_basic = false;
                    }
                    continue;
                }
                match c {
                    '\'' => in_literal = !in_literal,
                    '"' => in_basic = true,
                    '[' => d += 1,
                    ']' => d -= 1,
                    _ => {}
                }
            }
            d
        };
        let mut quote3: Option<&str> = None;
        if value_part.starts_with("\"\"\"") {
            quote3 = Some("\"\"\"");
        } else if value_part.starts_with("'''") {
            quote3 = Some("'''");
        }
        if let Some(q) = quote3 {
            let mut acc = value_part[3..].to_string();
            while !acc.contains(q) && i < lines.len() {
                acc.push('\n');
                acc.push_str(strip_comment(lines[i]).trim_start());
                i += 1;
            }
            if let Some(pos) = acc.find(q) {
                acc.truncate(pos);
            }
            value_part = format!("{q}{acc}{q}");
        } else if value_part.starts_with('[') && balance(&value_part) > 0 {
            while balance(&value_part) > 0 && i < lines.len() {
                value_part.push('\n');
                value_part.push_str(strip_comment(lines[i]).trim_start());
                i += 1;
            }
        }
        let value = if value_part.starts_with('[') && value_part.ends_with(']') {
            parse_array(&value_part)
        } else {
            parse_scalar(&value_part)
        };
        match &cursor {
            Cursor::Table(path) => {
                table_at(&mut root, path).insert(key, value);
            }
            Cursor::ArrayElem(path) => {
                let parent = &path[..path.len() - 1];
                let last = path[path.len() - 1].clone();
                let node = table_at(&mut root, parent);
                if let Some(arr) = node.get_mut(&last).and_then(|v| v.as_array_mut()) {
                    if let Some(elem) = arr.last_mut().and_then(|v| v.as_object_mut()) {
                        elem.insert(key, value);
                    }
                }
            }
        }
    }
    Ok(Value::Object(root))
}

pub(super) fn dependency_requirements(deps: &Value, dialect: &str) -> Vec<Value> {
    deps.as_object()
        .map(|m| {
            m.iter()
                .filter(|(id, _)| !BUILTINS.contains(&id.as_str()))
                .map(|(id, range)| json!({ "id": id, "range": dependency_range(range, dialect) }))
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn join_ranges(list: &[Value], key: &str) -> String {
    let parts: Vec<String> = list
        .iter()
        .filter(|d| d.get("modId").and_then(|v| v.as_str()) == Some(key))
        .map(|d| {
            d.get("versionRange")
                .and_then(|v| v.as_str())
                .unwrap_or("*")
                .to_string()
        })
        .collect();
    parts.join(" && ")
}

/// parseModArchive()：fabric.mod.json / quilt.mod.json / neoforge.mods.toml / mods.toml /
/// mcmod.info + jarjar 嵌套 + 图标提取。错误写入 info.error（与原版一致不抛出）。
pub fn parse_mod_archive(
    data: &[u8],
    file_path: &str,
    file_name: &str,
    depth: u32,
    include_icons: bool,
) -> Value {
    let info = json!({
        "filePath": file_path,
        "fileName": file_name,
        "id": "",
        "name": "",
        "version": "",
        "loader": Value::Null,
        "mcRange": "",
        "dependencies": [],
    });
    let build_error = |msg: &str| -> Value {
        let mut e = info.clone();
        e["error"] = json!(msg);
        e
    };
    let mut archive = match zip::ZipArchive::new(std::io::Cursor::new(data)) {
        Ok(a) => a,
        Err(_) => return build_error("未找到有效的 MOD 元数据 / mod id"),
    };
    let outcome = (|| -> Result<Value, String> {
        let mut records: Vec<Value> = Vec::new(); // {loader,id,name,version,mcRange,loaderRange,requirements,icon?}
        let mut provides: Vec<(String, String)> = Vec::new();
        let mut nested: Vec<String> = Vec::new();
        let mut java_requirements: Vec<Value> = Vec::new();

        // ---- fabric.mod.json ----
        if let Some(fabric) = zip_read_text(&mut archive, "fabric.mod.json")? {
            let j: Value = serde_json::from_str(strip_bom(&fabric)).map_err(|e| e.to_string())?;
            let dep = j.get("depends").cloned().unwrap_or(json!({}));
            let environment = j.get("environment").and_then(|v| v.as_str()).unwrap_or("");
            if environment != "server" {
                if dep.get("java").is_some() && !dep.get("java").unwrap().is_null() {
                    java_requirements.push(json!({ "loader": "fabric", "source": format!("{file_name}: {}", j.get("id").and_then(|v| v.as_str()).unwrap_or("")), "range": dependency_range(dep.get("java").unwrap(), "fabric") }));
                }
                if let Some(breaks) = j.get("breaks").and_then(|b| b.get("java")) {
                    if !breaks.is_null() {
                        java_requirements.push(json!({ "loader": "fabric", "source": format!("{file_name}: {}", j.get("id").and_then(|v| v.as_str()).unwrap_or("")), "range": dependency_range(breaks, "fabric"), "exclude": true }));
                    }
                }
            }
            let id = j.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let name = j.get("name").and_then(|v| v.as_str()).unwrap_or(id);
            let version = match j.get("version") {
                Some(Value::String(s)) => s.clone(),
                Some(v) => v.to_string(),
                None => String::new(),
            };
            let icon = if let Some(i) = j.get("icon") {
                if let Some(s) = i.as_str() {
                    Some(s.to_string())
                } else if let Some(m) = i.as_object() {
                    m.values()
                        .next()
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                } else {
                    None
                }
            } else {
                None
            };
            records.push(json!({
                "loader": "fabric",
                "id": id,
                "name": name,
                "version": version,
                "mcRange": dependency_range(dep.get("minecraft").unwrap_or(&Value::Null), "fabric"),
                "loaderRange": dependency_range(dep.get("fabricloader").unwrap_or(&Value::Null), "fabric"),
                "requirements": dependency_requirements(&dep, "fabric"),
                "icon": icon,
            }));
            if let Some(p) = j.get("provides").and_then(|v| v.as_array()) {
                for pid in p {
                    if let Some(s) = pid.as_str() {
                        provides.push((s.to_string(), version.clone()));
                    }
                }
            }
            if let Some(jars) = j.get("jars").and_then(|v| v.as_array()) {
                for jar in jars {
                    if let Some(f) = jar.get("file").and_then(|v| v.as_str()) {
                        nested.push(f.to_string());
                    }
                }
            }
        }

        // ---- quilt.mod.json ----
        if let Some(quilt) = zip_read_text(&mut archive, "quilt.mod.json")? {
            let parsed: Value =
                serde_json::from_str(strip_bom(&quilt)).map_err(|e| e.to_string())?;
            let j = parsed.get("quilt_loader").cloned().unwrap_or(json!({}));
            let mut deps: serde_json::Map<String, Value> = serde_json::Map::new();
            if let Some(list) = j.get("depends").and_then(|v| v.as_array()) {
                for raw in list {
                    let d = if let Some(s) = raw.as_str() {
                        json!({ "id": s, "versions": "*" })
                    } else {
                        raw.clone()
                    };
                    if d.is_array() || d.get("unless").is_some() {
                        return Err("Quilt 条件前置需要手动检查；未自动安装".into());
                    }
                    if let Some(id) = d.get("id").and_then(|v| v.as_str()) {
                        if !d.get("optional").and_then(|v| v.as_bool()).unwrap_or(false)
                            && d.get("environment").and_then(|v| v.as_str()) != Some("server")
                        {
                            let key = id.split(':').next_back().unwrap_or(id).to_string();
                            let versions = d.get("versions").cloned().unwrap_or(json!("*"));
                            let merged = match deps.get(&key) {
                                None => versions,
                                Some(prev) => json!({ "all": [prev.clone(), versions] }),
                            };
                            deps.insert(key, merged);
                        }
                    }
                }
            }
            let deps_value = Value::Object(deps.clone());
            let id = j.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let name = j
                .pointer("/metadata/name")
                .and_then(|v| v.as_str())
                .unwrap_or(id);
            let version = match j.get("version") {
                Some(Value::String(s)) => s.clone(),
                Some(v) => v.to_string(),
                None => String::new(),
            };
            records.push(json!({
                "loader": "quilt",
                "id": id,
                "name": name,
                "version": version,
                "mcRange": dependency_range(deps_value.get("minecraft").unwrap_or(&Value::Null), "quilt"),
                "loaderRange": dependency_range(deps_value.get("quilt_loader").unwrap_or(&Value::Null), "quilt"),
                "requirements": dependency_requirements(&deps_value, "quilt"),
                "icon": j.pointer("/metadata/icon").and_then(|v| v.as_str()).map(str::to_string),
            }));
            if deps_value.get("java").is_some() {
                java_requirements.push(json!({ "loader": "quilt", "source": format!("{file_name}: {id}"), "range": dependency_range(deps_value.get("java").unwrap(), "quilt") }));
            }
            if let Some(p) = j.get("provides").and_then(|v| v.as_array()) {
                for pv in p {
                    let pid = match pv {
                        Value::String(s) => s.clone(),
                        other => other
                            .get("id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                    };
                    let ver = pv
                        .get("version")
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                        .unwrap_or_else(|| version.clone());
                    provides.push((pid.split(':').next_back().unwrap_or("").to_string(), ver));
                }
            }
            if let Some(jars) = j.get("jars").and_then(|v| v.as_array()) {
                for jar in jars {
                    match jar {
                        Value::String(s) => nested.push(s.clone()),
                        other => {
                            if let Some(f) = other.get("file").and_then(|v| v.as_str()) {
                                nested.push(f.to_string());
                            }
                        }
                    }
                }
            }
        }

        // ---- neoforge.mods.toml / mods.toml ----
        for file in ["META-INF/neoforge.mods.toml", "META-INF/mods.toml"] {
            let Some(text) = zip_read_text(&mut archive, file)? else {
                continue;
            };
            let j = parse_mods_toml(&text)?;
            let mods = j
                .get("mods")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            if mods.is_empty() {
                return Err(format!("{file} 缺少 [[mods]]"));
            }
            let mut all_deps: Vec<Value> = Vec::new();
            if let Some(dep_map) = j.get("dependencies").and_then(|v| v.as_object()) {
                for list in dep_map.values() {
                    if let Some(arr) = list.as_array() {
                        all_deps.extend(arr.iter().cloned());
                    } else {
                        all_deps.push(list.clone());
                    }
                }
            }
            let loader: &str = if file.contains("neoforge")
                || all_deps
                    .iter()
                    .any(|d| d.get("modId").and_then(|v| v.as_str()) == Some("neoforge"))
            {
                "neoforge"
            } else {
                "forge"
            };
            let is_required = |d: &Value| -> bool {
                d.get("mandatory").and_then(|v| v.as_bool()) != Some(false)
                    && d.get("type")
                        .and_then(|v| v.as_str())
                        .map(|t| t.is_empty() || t == "required")
                        .unwrap_or(true)
                    && d.get("side")
                        .and_then(|v| v.as_str())
                        .unwrap_or("BOTH")
                        .to_uppercase()
                        != "SERVER"
            };
            let _required: Vec<Value> = all_deps
                .iter()
                .filter(|d| is_required(d))
                .cloned()
                .collect();
            let manifest = zip_read_text(&mut archive, "META-INF/MANIFEST.MF")?.unwrap_or_default();
            let jar_version = regex::Regex::new(r"(?mi)^Implementation-Version:\s*(.+)$")
                .ok()
                .and_then(|re| re.captures(&manifest))
                .map(|c| c[1].trim().to_string())
                .unwrap_or_default();
            let mod_loader = j.get("modLoader").and_then(|v| v.as_str()).unwrap_or("");
            for m in &mods {
                let mod_id = m.get("modId").and_then(|v| v.as_str()).unwrap_or("");
                let own: Vec<Value> = j
                    .get("dependencies")
                    .and_then(|d| d.get(mod_id))
                    .and_then(|d| d.as_array())
                    .map(|arr| arr.iter().filter(|d| is_required(d)).cloned().collect())
                    .unwrap_or_default();
                let raw_version = match m.get("version") {
                    Some(Value::String(s)) => s.clone(),
                    Some(v) => v.to_string(),
                    None => String::new(),
                };
                let version = raw_version.replace("${file.jarVersion}", &jar_version);
                provides.push((mod_id.to_string(), version.clone()));
                if let Some(jr) = j
                    .pointer(&format!("/features/{mod_id}/java_version"))
                    .and_then(|v| v.as_str())
                {
                    java_requirements.push(json!({ "loader": loader, "source": format!("{file_name}: {mod_id}"), "range": jr }));
                }
                for d in own
                    .iter()
                    .filter(|d| d.get("modId").and_then(|v| v.as_str()) == Some("java"))
                {
                    java_requirements.push(json!({ "loader": loader, "source": format!("{file_name}: {mod_id}"), "range": d.get("versionRange").and_then(|v| v.as_str()).unwrap_or("*") }));
                }
                let loader_range = {
                    let own_ranges = join_ranges(&own, loader);
                    if !own_ranges.is_empty() {
                        own_ranges
                    } else if loader == "forge"
                        && (mod_loader == "javafml" || mod_loader == "lowcodefml")
                    {
                        j.get("loaderVersion")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string()
                    } else {
                        String::new()
                    }
                };
                records.push(json!({
                    "loader": loader,
                    "id": mod_id,
                    "name": m.get("displayName").and_then(|v| v.as_str()).unwrap_or(mod_id),
                    "version": version,
                    "mcRange": join_ranges(&own, "minecraft"),
                    "loaderRange": loader_range,
                    "requirements": own.iter().filter(|d| !d.get("modId").and_then(|v| v.as_str()).map(|id| BUILTINS.contains(&id)).unwrap_or(false)).map(|d| json!({"id": d.get("modId").and_then(|v| v.as_str()).unwrap_or(""), "range": d.get("versionRange").and_then(|v| v.as_str()).unwrap_or("*")})).collect::<Vec<Value>>(),
                    "icon": m.get("logoFile").and_then(|v| v.as_str()).or_else(|| j.get("logoFile").and_then(|v| v.as_str())).map(str::to_string),
                }));
            }
        }

        // ---- mcmod.info（legacy） ----
        if records.is_empty() {
            if let Some(legacy) = zip_read_text(&mut archive, "mcmod.info")? {
                let j: Value = serde_json::from_str(strip_bom(&legacy)).unwrap_or(Value::Null);
                let mods = if j.is_array() {
                    j.clone()
                } else {
                    j.get("modList").cloned().unwrap_or(json!([]))
                };
                if let Some(list) = mods.as_array() {
                    for m in list {
                        let req_src = m
                            .get("requiredMods")
                            .filter(|v| v.is_array())
                            .or_else(|| m.get("dependencies").filter(|v| v.is_array()))
                            .cloned()
                            .unwrap_or(json!([]));
                        let req: Vec<Value> = req_src
                            .as_array()
                            .unwrap()
                            .iter()
                            .filter_map(|s| s.as_str())
                            .map(|s| {
                                let stripped = regex::Regex::new(r"^required-(?:after|before):")
                                    .unwrap()
                                    .replace(s, "")
                                    .to_string();
                                match stripped.split_once('@') {
                                    Some((id, range)) => json!({ "id": id, "range": range }),
                                    None => json!({ "id": stripped, "range": "*" }),
                                }
                            })
                            .filter(|d: &Value| {
                                !d.get("id")
                                    .and_then(|v| v.as_str())
                                    .map(|id| BUILTINS.contains(&id))
                                    .unwrap_or(false)
                            })
                            .collect();
                        let id = m.get("modid").and_then(|v| v.as_str()).unwrap_or("");
                        records.push(json!({
                            "loader": "forge",
                            "id": id,
                            "name": m.get("name").and_then(|v| v.as_str()).unwrap_or(id),
                            "version": m.get("version").and_then(|v| v.as_str()).unwrap_or(""),
                            "mcRange": m.get("mcversion").and_then(|v| v.as_str()).unwrap_or(""),
                            "loaderRange": "",
                            "requirements": req,
                            "icon": Value::Null,
                        }));
                        provides.push((
                            id.to_string(),
                            m.get("version")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                        ));
                    }
                }
            }
        }

        if records.is_empty()
            || records[0]
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .is_empty()
        {
            return Err("未找到有效的 MOD 元数据 / mod id".into());
        }
        let first = records[0].clone();
        let mut out = json!({
            "filePath": file_path,
            "fileName": file_name,
            "id": first.get("id").cloned().unwrap_or(json!("")),
            "name": first.get("name").cloned().unwrap_or(json!("")),
            "version": first.get("version").cloned().unwrap_or(json!("")),
            "loader": first.get("loader").cloned().unwrap_or(Value::Null),
            "mcRange": first.get("mcRange").cloned().unwrap_or(json!("")),
            "dependencies": first.get("requirements").and_then(|r| r.as_array()).map(|a| a.iter().filter_map(|d| d.get("id").cloned()).collect::<Vec<Value>>()).unwrap_or_default(),
        });
        if let Some(lr) = first
            .get("loaderRange")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        {
            out["loaderRange"] = json!(lr);
        }
        if let Some(icon) = first.get("icon") {
            if icon.is_string() {
                out["_icon"] = icon.clone();
            }
        }
        out["requirements"] = first.get("requirements").cloned().unwrap_or(json!([]));
        out["variants"] = json!(records
            .iter()
            .map(|r| {
                let mut v = json!({
                    "loader": r.get("loader").cloned().unwrap_or(Value::Null),
                    "mcRange": r.get("mcRange").cloned().unwrap_or(json!("")),
                    "requirements": r.get("requirements").cloned().unwrap_or(json!([])),
                });
                if let Some(lr) = r
                    .get("loaderRange")
                    .and_then(|x| x.as_str())
                    .filter(|s| !s.is_empty())
                {
                    v["loaderRange"] = json!(lr);
                }
                v
            })
            .collect::<Vec<Value>>());
        for r in &records {
            provides.push((
                r.get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                r.get("version")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
            ));
        }
        // jarjar 嵌套 jar
        if let Some(jarjar) = zip_read_text(&mut archive, "META-INF/jarjar/metadata.json")? {
            if let Ok(j) = serde_json::from_str::<Value>(strip_bom(&jarjar)) {
                if let Some(jars) = j.get("jars").and_then(|v| v.as_array()) {
                    for jar in jars {
                        if let Some(p) = jar.get("path").and_then(|v| v.as_str()) {
                            nested.push(p.to_string());
                        }
                    }
                }
            }
        }
        if depth < 3 {
            let mut seen = HashSet::new();
            let unique: Vec<String> = nested
                .iter()
                .filter(|name| seen.insert((*name).clone()))
                .take(64)
                .cloned()
                .collect();
            for name in unique {
                let entry = match zip_read_entry(&mut archive, &name, 64 * 1024 * 1024) {
                    Ok(Some(e)) => e,
                    _ => continue,
                };
                // 嵌套归档只贡献依赖身份，不提取图标
                if let Ok(child) = std::thread::scope(|s| {
                    let handle = s.spawn(|| {
                        parse_mod_archive(&entry.bytes, file_path, &name, depth + 1, false)
                    });
                    handle.join()
                }) {
                    if let Some(provs) = child.get("provides").and_then(|v| v.as_array()) {
                        for p in provs {
                            if let (Some(id), Some(ver)) = (
                                p.get("id").and_then(|v| v.as_str()),
                                p.get("version").and_then(|v| v.as_str()),
                            ) {
                                provides.push((id.to_string(), ver.to_string()));
                            }
                        }
                    }
                    if let Some(jreqs) = child.get("javaRequirements").and_then(|v| v.as_array()) {
                        for r in jreqs {
                            let mut rr = r.clone();
                            if let Some(src) = r.get("source").and_then(|v| v.as_str()) {
                                rr["source"] = json!(format!("{file_name} > {src}"));
                            }
                            java_requirements.push(rr);
                        }
                    }
                }
            }
        }
        // provides 去重（按 id 保留首个，Map 语义）
        let mut seen = HashSet::new();
        let unique_provides: Vec<Value> = provides
            .into_iter()
            .filter(|(id, _)| !id.is_empty())
            .filter(|(id, _)| seen.insert(id.clone()))
            .map(|(id, version)| json!({ "id": id, "version": version }))
            .collect();
        out["provides"] = json!(unique_provides);
        if !java_requirements.is_empty() {
            out["javaRequirements"] = json!(java_requirements);
        }
        // 图标（≤512KB，dataURL png）
        if include_icons {
            if let Some(icon) = first.get("icon").and_then(|v| v.as_str()) {
                if let Ok(Some(entry)) = zip_read_entry(&mut archive, icon, 512 * 1024) {
                    use base64::Engine;
                    out["iconDataUrl"] = json!(format!(
                        "data:image/png;base64,{}",
                        base64::engine::general_purpose::STANDARD.encode(&entry.bytes)
                    ));
                }
            }
        }
        Ok(out)
    })();
    match outcome {
        Ok(v) => v,
        Err(e) => build_error(&e),
    }
}

/// parseModFile()：单文件解析；损坏 jar → 固定错误文案
pub fn parse_mod_file(file_path: &str) -> Value {
    let file_name = crate::installer::basename_of(file_path);
    match std::fs::read(file_path) {
        Ok(bytes) => parse_mod_archive(&bytes, file_path, &file_name, 0, true),
        Err(_) => json!({
            "filePath": file_path,
            "fileName": file_name,
            "id": "",
            "name": "",
            "version": "",
            "loader": Value::Null,
            "mcRange": "",
            "dependencies": [],
            "error": "文件损坏或不是有效的 jar 文件",
        }),
    }
}

/// expandJarPaths()：文件/文件夹（一层）展开为 .jar 列表；非 jar 逐个给出原因
pub fn expand_jar_paths(paths: &[String]) -> (Vec<String>, Vec<String>) {
    let mut files = Vec::new();
    let mut skipped = Vec::new();
    for p in paths {
        match std::fs::metadata(p) {
            Ok(st) if st.is_dir() => {
                let mut jars: Vec<String> = std::fs::read_dir(p)
                    .map(|rd| {
                        rd.flatten()
                            .map(|e| e.file_name().to_string_lossy().into_owned())
                            .filter(|n| n.to_lowercase().ends_with(".jar"))
                            .map(|n| Path::new(p).join(n).to_string_lossy().into_owned())
                            .collect()
                    })
                    .unwrap_or_default();
                jars.sort();
                if !jars.is_empty() {
                    files.extend(jars);
                } else {
                    skipped.push(format!(
                        "{}（文件夹内无 .jar）",
                        crate::installer::basename_of(p)
                    ));
                }
            }
            Ok(_) => {
                if p.to_lowercase().ends_with(".jar") {
                    files.push(p.clone());
                } else {
                    skipped.push(crate::installer::basename_of(p));
                }
            }
            Err(_) => skipped.push(crate::installer::basename_of(p)),
        }
    }
    (files, skipped)
}

// ---------------- modScan.ts / modScanWorker.ts ----------------

/// scanModDirectory()：analysis/icons 含图标，catalog 不含；hash=true 时附 sha1；
/// 传入 names 时同时计算 CurseForge 指纹并接受 .jar.disabled/.zip
pub fn scan_mod_directory(
    dir: &str,
    hash: bool,
    names: Option<&[String]>,
    purpose: &str,
) -> Result<Vec<Value>, String> {
    let include_icons = purpose != "catalog";
    let requested: Option<HashSet<String>> = names.map(|n| n.iter().cloned().collect());
    let entries = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.to_string()),
    };
    let mut result: Vec<Value> = Vec::new();
    for entry in entries.flatten() {
        let is_file = entry.file_type().map(|t| t.is_file()).unwrap_or(false);
        if !is_file {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        static JAR_ZIP: OnceLock<regex::Regex> = OnceLock::new();
        static JAR: OnceLock<regex::Regex> = OnceLock::new();
        let accept = match &requested {
            Some(set) => {
                if !set.contains(&name) {
                    false
                } else {
                    JAR_ZIP
                        .get_or_init(|| {
                            regex::Regex::new(r"(?i)\.(?:jar(?:\.disabled)?|zip)$").unwrap()
                        })
                        .is_match(&name)
                }
            }
            None => JAR
                .get_or_init(|| regex::Regex::new(r"(?i)\.jar$").unwrap())
                .is_match(&name),
        };
        if !accept {
            continue;
        }
        let file = entry.path();
        let file_str = file.to_string_lossy().into_owned();
        match std::fs::read(&file) {
            Ok(data) => {
                let mut info = parse_mod_archive(&data, &file_str, &name, 0, include_icons);
                if include_icons && info.get("iconDataUrl").is_none() {
                    if let Ok(bytes) = read_zip_entry(&data, "pack.png", 512 * 1024) {
                        use base64::Engine;
                        info["iconDataUrl"] = json!(format!(
                            "data:image/png;base64,{}",
                            base64::engine::general_purpose::STANDARD.encode(bytes)
                        ));
                    }
                }
                if hash {
                    let mut h = Sha1::new();
                    h.update(&data);
                    info["sha1"] = json!(hex::encode(h.finish()));
                }
                if names.is_some() {
                    info["fingerprint"] = json!(curse_fingerprint(&data) as u64);
                }
                result.push(info);
            }
            Err(_) => {
                result.push(json!({
                    "fileName": name,
                    "filePath": file_str,
                    "error": "文件损坏或不可读取",
                }));
            }
        }
    }
    Ok(result)
}

fn read_zip_entry(data: &[u8], name: &str, max_bytes: u64) -> Result<Vec<u8>, String> {
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(data)).map_err(|e| e.to_string())?;
    let mut file = archive.by_name(name).map_err(|e| e.to_string())?;
    if file.size() > max_bytes {
        return Err("过大".into());
    }
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    Ok(buf)
}

/// scanManagedModDirectory()：management 含 disabled 文件；查重/运行分析保持仅启用
pub fn scan_managed_mod_directory(dir: &str, purpose: &str) -> Result<Vec<Value>, String> {
    let names: Vec<String> = match std::fs::read_dir(dir) {
        Ok(rd) => rd
            .flatten()
            .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| {
                regex::Regex::new(r"(?i)\.jar(?:\.disabled)?$")
                    .unwrap()
                    .is_match(n)
            })
            .collect(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.to_string()),
    };
    scan_mod_directory(dir, true, Some(&names), purpose)
}
