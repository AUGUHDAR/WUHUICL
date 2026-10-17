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
        if !choices
            .as_array()
            .map_or(false, |c| c.iter().any(|v| v == value))
        {
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
    std::fs::write(
        &tmp,
        serde_json::to_string_pretty(&state).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    Ok(state)
}
