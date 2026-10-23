//! 默认游戏选项：对应原版 src/main/core/defaultGameOptions.ts
//! 合同形状 = shared/gameOptions.ts 的 DefaultGameOptions { enabled: boolean; values: Record<string, GameOptionValue> }

use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::OnceLock;

/// 选项定义表：从原版 shared/gameOptions.ts 的 GAME_OPTIONS 原样导出（89 项）
static TABLE_JSON: &str = include_str!("game_options.json");

fn defs() -> &'static [Value] {
    static TABLE: OnceLock<Vec<Value>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let all: Vec<Value> = serde_json::from_str(TABLE_JSON).expect("game_options.json 解析失败");
        // 对应原版 uniqueGameOptions：按 id 去重保留首项
        let mut seen: Vec<String> = Vec::new();
        all.into_iter()
            .filter(|d| {
                let id = d["id"].as_str().unwrap_or_default().to_string();
                if seen.contains(&id) {
                    false
                } else {
                    seen.push(id);
                    true
                }
            })
            .collect()
    })
}

fn find_def(id: &str) -> Option<&'static Value> {
    defs().iter().find(|d| d["id"].as_str() == Some(id))
}

/// 对应原版 validateGameOption(id, value)：抛错文案与原版一致
pub fn validate(id: &str, value: &Value) -> Result<(), String> {
    let d = match find_def(id) {
        Some(d) => d,
        None => return Err("未知游戏选项".into()),
    };
    if let Some(choices) = d.get("choices") {
        if !choices.as_array().map_or(false, |c| c.iter().any(|v| v == value)) {
            return Err("游戏选项值无效".into());
        }
    } else {
        let initial = &d["initial"];
        let same = match (initial, value) {
            (Value::Bool(_), Value::Bool(_)) => true,
            (Value::Number(_), Value::Number(_)) => true,
            (Value::String(_), Value::String(_)) => true,
            _ => false,
        };
        if !same {
            return Err("游戏选项值无效".into());
        }
    }
    if let Some(n) = value.as_f64() {
        if !value.as_f64().map_or(false, |v| v.is_finite()) {
            return Err("游戏选项超出有效范围".into());
        }
        let min = d["min"].as_f64();
        let max = d["max"].as_f64();
        if let Some(m) = min {
            if n < m {
                return Err("游戏选项超出有效范围".into());
            }
        }
        if let Some(m) = max {
            if n > m {
                return Err("游戏选项超出有效范围".into());
            }
        }
        if d["step"].as_f64() == Some(1.0) && n.fract() != 0.0 {
            return Err("游戏选项超出有效范围".into());
        }
    }
    Ok(())
}

fn file(app: &tauri::AppHandle) -> PathBuf {
    crate::settings::user_data_dir(app).join("default-game-options.json")
}

/// 对应原版 getDefaultGameOptions()
pub fn get(app: &tauri::AppHandle) -> Value {
    let raw: Value = match std::fs::read(file(app)) {
        Ok(b) => serde_json::from_slice(&b).unwrap_or(Value::Null),
        Err(_) => return json!({ "enabled": false, "values": {} }),
    };
    if raw.is_null() {
        return json!({ "enabled": false, "values": {} });
    }
    let mut values = serde_json::Map::new();
    match raw.get("values") {
        Some(Value::Object(map)) => {
            for (id, value) in map {
                if validate(id, value).is_ok() {
                    values.insert(id.clone(), value.clone());
                }
            }
        }
        Some(Value::Array(arr)) => {
            for (i, value) in arr.iter().enumerate() {
                if validate(&i.to_string(), value).is_ok() {
                    values.insert(i.to_string(), value.clone());
                }
            }
        }
        _ => {}
    }
    json!({ "enabled": raw.get("enabled").and_then(|v| v.as_bool()) == Some(true), "values": Value::Object(values) })
}

/// 对应原版 setDefaultGameOptions(change)
pub fn set(app: &tauri::AppHandle, change: &Value) -> Result<Value, String> {
    let mut state = get(app);
    if change.get("enabled").is_some() && !change["enabled"].is_null() {
        match change["enabled"].as_bool() {
            Some(b) => state["enabled"] = json!(b),
            None => return Err("同步开关无效".into()),
        }
    }
    if let Some(id) = change.get("id").and_then(|v| v.as_str()) {
        if !id.is_empty() {
            let value = &change["value"];
            let values = state["values"].as_object_mut().unwrap();
            if value.is_null() && change.get("value").is_some() {
                values.remove(id);
            } else {
                validate(id, value)?;
                values.insert(id.to_string(), value.clone());
            }
        }
    }
    let path = file(app);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(&state).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    Ok(state)
}

// ---------------- 启动时同步（原版 syncDefaultGameOptions + mergeGameOptions） ----------------

/// 对应 shared/keybindings.ts parseSnapshotId："26w14a" → (26, 14)
fn parse_snapshot_id(version: &str) -> Option<(u32, u32)> {
    let v = version.trim();
    let rest = v.strip_suffix(['a', 'b', 'c', 'd', 'e']).unwrap_or(v);
    let (yy, ww) = rest.split_once('w')?;
    if yy.len() != 2 || ww.is_empty() || ww.len() > 2 {
        return None;
    }
    if !yy.bytes().all(|b| b.is_ascii_digit()) || !ww.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((yy.parse().ok()?, ww.parse().ok()?))
}

/// 对应 shared/keybindings.ts mcVersionFamily：版本字符串 → 可比较数值元组
fn mc_version_family(version: &str) -> Vec<u64> {
    let v = version.trim();
    if let Some((yy, ww)) = parse_snapshot_id(v) {
        let family: Vec<u64> = match (yy, ww) {
            (y, _) if y >= 26 => vec![26, 0],
            (25, w) if w >= 41 => vec![1, 21, 11],
            (25, _) => vec![1, 21, 10],
            (24, _) => vec![1, 21, 4],
            (23, _) => vec![1, 20, 4],
            (22, _) => vec![1, 19, 3],
            (21, _) => vec![1, 18, 2],
            (20, w) if w >= 6 => vec![1, 16, 5],
            (20, _) => vec![1, 15, 2],
            (19, w) if w >= 41 => vec![1, 15, 2],
            (19, _) => vec![1, 14, 4],
            (18, _) => vec![1, 14, 4],
            (17, w) if w >= 43 => vec![1, 13],
            (17, _) => vec![1, 12, 2],
            (16, w) if w >= 20 => vec![1, 10, 2],
            (16, _) => vec![1, 9, 4],
            (15, _) => vec![1, 9],
            _ => vec![1, 8],
        };
        return family;
    }
    // 正式版：剥离开发后缀（1.21.11-pre1 / 26.2-snapshot-1 / "1.14 Pre-Release 2"）
    let base = v.split([' ', '-']).next().unwrap_or("");
    let parts: Option<Vec<u64>> = base.split('.').map(|p| p.parse::<u64>().ok()).collect();
    match parts {
        Some(p) if !p.is_empty() => p,
        _ => vec![999],
    }
}

/// 对应 shared/keybindings.ts mcVersionAtLeast：空版本按最新处理
fn mc_version_at_least(mc_version: &str, target: &str) -> bool {
    if mc_version.trim().is_empty() {
        return true;
    }
    let (a, b) = (mc_version_family(mc_version), mc_version_family(target));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    true
}

/// 供 launch.rs 等模块判定版本门槛（如按键同步需 ≥1.13）
pub fn mc_version_at_least_pub(mc_version: &str, target: &str) -> bool {
    mc_version_at_least(mc_version, target)
}

/// 对应 shared/gameOptions.ts supportedGameOption
fn supported(d: &Value, version: &str) -> bool {
    let since = d["since"].as_str();
    if let Some(s) = since {
        if !mc_version_at_least(version, s) {
            return false;
        }
    }
    if let Some(until) = d["until"].as_str() {
        if mc_version_at_least(version, until) {
            return false;
        }
    }
    true
}

/// 对应 shared/gameOptions.ts encodeGameOption（不含 highContrast 的 resourcePacks 联动，
/// 那部分在 merge_game_options 内处理）
fn encode(d: &Value, value: &Value, version: &str) -> String {
    let encoding = d["encoding"].as_str().unwrap_or("");
    let num = || value.as_f64().unwrap_or(0.0);
    let trim_num = |n: f64| -> String {
        let r = n.round();
        if (n - r).abs() < f64::EPSILON && n.abs() < 9e15 {
            format!("{}", r as i64)
        } else {
            let s = format!("{}", n);
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        }
    };
    match encoding {
        "fov" => trim_num((num() - 70.0) / 40.0),
        "chatOpacity" => trim_num((num() - 10.0) / 90.0),
        "chatWidth" => trim_num((num() - 40.0) / 280.0),
        "chatHeight" => trim_num((num() - 20.0) / 160.0),
        "percent" => trim_num(num() / 100.0),
        "sensitivity" => trim_num(num() / 200.0),
        "json" => serde_json::to_string(value).unwrap_or_else(|_| "null".into()),
        _ => {
            if d["id"].as_str() == Some("lang") && !mc_version_at_least(version, "1.11") {
                // 旧版区域后缀大写：zh_cn → zh_CN
                let raw = value.as_str().unwrap_or("").to_string();
                match raw.split_once('_') {
                    Some((lang, region)) => format!("{lang}_{}", region.to_uppercase()),
                    None => raw,
                }
            } else if d["id"].as_str() == Some("mainHand") && mc_version_at_least(version, "1.19") {
                serde_json::to_string(value).unwrap_or_else(|_| "\"right\"".into())
            } else if value.is_string() {
                value.as_str().unwrap_or("").to_string()
            } else if value.is_boolean() {
                value.as_bool().map(|b| b.to_string()).unwrap_or_default()
            } else {
                trim_num(num())
            }
        }
    }
}

/// 对应原版 mergeGameOptions：把 values 合入 options.txt 文本，返回（新文本, 已应用 id, 不支持 label）
fn merge_game_options(
    text: &str,
    values: &Value,
    version: &str,
) -> Result<(String, Vec<String>, Vec<String>), String> {
    let map = values.as_object().ok_or("默认游戏选项数据损坏")?;
    if !map.is_empty() && (version.trim().is_empty() || mc_version_family(version)[0] == 999) {
        return Err("无法确认 Minecraft 实际版本，未写入默认游戏选项".into());
    }
    let mut writes: Vec<(String, String)> = Vec::new();
    let mut unsupported: Vec<String> = Vec::new();
    for (id, value) in map {
        let Some(d) = find_def(id) else {
            return Err("未知游戏选项".into());
        };
        validate(id, value)?;
        if !supported(d, version) {
            unsupported.push(d["id"].as_str().unwrap_or(id).to_string());
            continue;
        }
        writes.push((id.clone(), encode(d, value, version)));
    }
    // 高对比度开关同时驱动内置 high_contrast 资源包：只写 highContrast:true 下次启动包仍是关的
    if map.contains_key("highContrast") {
        let source = text
            .split('\n')
            .find(|line| line.trim_start_matches('\u{feff}').starts_with("resourcePacks:"))
            .map(|l| l.trim_start_matches('\u{feff}')["resourcePacks:".len()..].to_string());
        let mut packs: Vec<Value> = match &source {
            Some(s) => {
                let parsed: Value = serde_json::from_str(s.trim())
                    .map_err(|_| "现有资源包配置无效，未覆盖 options.txt".to_string())?;
                parsed
                    .as_array()
                    .cloned()
                    .ok_or("现有资源包配置无效，未覆盖 options.txt")?
            }
            None => Vec::new(),
        };
        let want = map.get("highContrast").and_then(|v| v.as_bool()).unwrap_or(false);
        packs.retain(|p| p.as_str() != Some("high_contrast"));
        if want {
            packs.push(json!("high_contrast"));
        }
        writes.push((
            "resourcePacks".into(),
            serde_json::to_string(&packs).unwrap_or_else(|_| "[]".into()),
        ));
    }
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let lines: Vec<&str> = text
        .split("\r\n")
        .flat_map(|l| l.split('\n'))
        .enumerate()
        .map(|(i, l)| (i, l))
        .collect::<Vec<_>>()
        .iter()
        .map(|(_, l)| *l)
        .collect();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out: Vec<String> = Vec::new();
    for line in &lines {
        let id = line.split(':').next().unwrap_or("");
        match writes.iter().find(|(w, _)| w == id) {
            Some((w, v)) => {
                if seen.insert(w.clone()) {
                    out.push(format!("{w}:{v}"));
                }
            }
            None => out.push((*line).to_string()),
        }
    }
    for (w, v) in &writes {
        if !seen.contains(w) {
            seen.insert(w.clone());
            out.push(format!("{w}:{v}"));
        }
    }
    let mut result = out.join(newline);
    if !result.is_empty() {
        result.push_str(newline);
    }
    Ok((result, writes.into_iter().map(|(w, _)| w).collect(), unsupported))
}

/// 对应原版 syncDefaultGameOptions：总开关开启时把玩家修改项写入实例 options.txt。
/// 返回（applied, unsupported）；总开关关闭时返回空。
pub fn sync(app: &tauri::AppHandle, game_dir: &str, version: &str) -> (Vec<String>, Vec<String>) {
    let state = get(app);
    if state["enabled"] != true {
        return (Vec::new(), Vec::new());
    }
    let target = PathBuf::from(game_dir).join("options.txt");
    let before = std::fs::read_to_string(&target).unwrap_or_default();
    let (text, applied, unsupported) =
        match merge_game_options(&before, &state["values"], version) {
            Ok(v) => v,
            Err(_) => return (Vec::new(), Vec::new()),
        };
    if text != before {
        if let Some(dir) = target.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = target.with_extension(format!("txt.wuhuicl-{}.tmp", std::process::id()));
        if std::fs::write(&tmp, &text).is_ok() && std::fs::rename(&tmp, &target).is_ok() {
            return (applied, unsupported);
        }
        let _ = std::fs::remove_file(&tmp);
        return (Vec::new(), unsupported);
    }
    (applied, unsupported)
}
