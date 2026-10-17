//! 自更新（对应原版 core/selfUpdate + applyUpdate 的通道合同面）。
//! WUHUICL 独立版本线：update:check 查 GitHub Releases（AUGUHDAR/WUHUICL）；
//! 下载/应用在首个正式 Release 发布后启用，通道全部接通并保持原版形状。

use serde_json::{json, Value};
use tauri::{Emitter, Manager};

const REPO_API: &str = "https://api.github.com/repos/AUGUHDAR/WUHUICL/releases";

fn current_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// update:check → UpdateCheckResult {ok, hasUpdate, release?, reason?, fromCache?}
fn check(app: &tauri::AppHandle, force: bool) -> Value {
    let _ = force;
    let resp = crate::net::client()
        .get(format!("{REPO_API}/latest"))
        .header("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(15))
        .send();
    let Ok(resp) = resp else {
        return json!({ "ok": false, "hasUpdate": false, "reason": "网络不可达" });
    };
    if !resp.status().is_success() {
        return json!({ "ok": false, "hasUpdate": false, "reason": format!("HTTP {}", resp.status().as_u16()) });
    }
    let Ok(data) = resp.json::<Value>() else {
        return json!({ "ok": false, "hasUpdate": false, "reason": "响应解析失败" });
    };
    let tag = data["tag_name"]
        .as_str()
        .unwrap_or("")
        .trim_start_matches(['v', 'V'])
        .to_string();
    let has_update = is_newer(&tag, &current_version());
    let assets: Vec<Value> = data["assets"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|a| {
            let name = a["name"].as_str()?.to_string();
            if !name.to_lowercase().ends_with(".exe") {
                return None;
            }
            Some(json!({
                "name": name,
                "url": a["browser_download_url"],
                "size": a["size"].as_u64().unwrap_or(0)
            }))
        })
        .collect();
    json!({
        "ok": true,
        "hasUpdate": has_update,
        "release": if has_update { json!({
            "version": tag,
            "name": data["name"].as_str().unwrap_or(""),
            "body": data["body"].as_str().unwrap_or(""),
            "date": data["published_at"].as_str().unwrap_or(""),
            "assets": assets,
            "rollback": false
        }) } else { Value::Null }
    })
}

/// 语义化版本比较：tag > current
fn is_newer(tag: &str, current: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> {
        s.split('.')
            .map(|p| {
                p.chars()
                    .filter(|c| c.is_ascii_digit())
                    .collect::<String>()
                    .parse()
                    .unwrap_or(0)
            })
            .collect()
    };
    let (a, b) = (parse(tag), parse(current));
    for i in 0..a.len().max(b.len()) {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        if x != y {
            return x > y;
        }
    }
    false
}

/// update:getConfigStatus → {configVersion, current, mismatch}
fn config_status(app: &tauri::AppHandle) -> Value {
    let s = crate::settings::get(app);
    let stored = s["configVersion"].as_i64().unwrap_or(1);
    json!({
        "configVersion": stored,
        "current": 1,
        "mismatch": if stored > 1 { json!("newer") } else { Value::Null }
    })
}

/// update:listReleases → ReleaseInfo[]
fn list_releases() -> Value {
    let Ok(resp) = crate::net::client()
        .get(REPO_API)
        .header("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(15))
        .send()
    else {
        return json!([]);
    };
    if !resp.status().is_success() {
        return json!([]);
    }
    let Ok(data) = resp.json::<Value>() else {
        return json!([]);
    };
    Value::Array(
        data.as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|r| {
                json!({
                    "version": r["tag_name"].as_str().unwrap_or("").trim_start_matches(['v', 'V']),
                    "name": r["name"].as_str().unwrap_or(""),
                    "body": r["body"].as_str().unwrap_or(""),
                    "date": r["published_at"].as_str().unwrap_or(""),
                    "rollback": false
                })
            })
            .collect(),
    )
}

/// 更新提示事件（启动检查发现新版本时推 updatePrompt——对应 index.ts 启动链）
pub fn startup_check(app: &tauri::AppHandle) {
    let handle = app.clone();
    std::thread::spawn(move || {
        let result = check(&handle, false);
        if result["ok"] == true && result["hasUpdate"] == true {
            if let Some(win) = handle.get_webview_window("main") {
                let _ = win.emit_to("main", "event:updatePrompt", result["release"].clone());
            }
        }
    });
}

/// 统一入口
pub fn invoke(
    app: &tauri::AppHandle,
    channel: &str,
    args: &[Value],
) -> Option<Result<Value, String>> {
    match channel {
        "update:check" => Some(Ok(check(
            app,
            args.first().and_then(|v| v.as_bool()).unwrap_or(false),
        ))),
        "update:getState" | "update:getPending" => Some(Ok(Value::Null)),
        "update:getConfigStatus" => Some(Ok(config_status(app))),
        "update:listReleases" => Some(Ok(list_releases())),
        "update:skip" => Some((|| {
            let version = args.first().and_then(|v| v.as_str()).unwrap_or("");
            crate::settings::save(app, json!({ "skipUpdateVersion": version }))?;
            Ok(Value::Null)
        })()),
        "update:resetSettings" => Some((|| {
            let file = crate::settings::settings_file(app);
            if file.exists() {
                let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
                let backup = file.with_file_name(format!("settings.backup-{stamp}.json"));
                let _ = std::fs::copy(&file, backup);
            }
            let mut def = crate::settings::defaults();
            def["configVersion"] = json!(1);
            // 备份后写默认值
            let path = crate::settings::settings_file(app);
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            std::fs::write(
                &path,
                serde_json::to_vec_pretty(&def).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            Ok(Value::Null)
        })()),
        // 更新事务与替换助手未移植：这里如实说明。WUHUICL 交付形态就是便携单文件，
        // 所以不能套用原版那句"当前运行形态不支持自更新（仅便携版）"。
        "update:start" | "update:apply" | "update:applyLocal" | "update:restoreBackup" => {
            Some(Err("更新器未移植".into()))
        }
        // 没有已就绪的更新时，原版 applyPendingIfAny() 恒返回 false
        "update:applyPending" => Some(Ok(Value::Bool(false))),
        "update:pickLocalFile" => Some(Ok(Value::Null)),
        _ => None,
    }
}
