//! 版本清单缓存（对应原版 core/versionCatalog.ts fetchVersionCatalog / parseVersionCatalog /
//! mergeVersionCatalogs + versions.ts getVersionCatalog / fetchVersionManifest）。
//!
//! 通道：versions:catalog、versions:manifest。
//! 语义：schema 2 缓存（userData/version_manifest.json）+ 5 分钟 TTL +
//!       官方/_bmclapi 双源并发抓取（官方优先合并，滞后镜像缺失的 release 保留）+
//!       全部源失败时回落缓存并标 stale，无缓存报「无法获取版本清单，请检查网络后重试」。

use crate::installer::run_blocking;
use crate::settings;
use serde_json::{json, Value};
use std::path::PathBuf;
use tauri::AppHandle;

/// 缓存 TTL（对应原版 TTL = 5 * 60_000ms）
const TTL_MS: i64 = 5 * 60_000;
/// 对应原版 VERSION_SOURCES：官方 meta 与 bmclapi 清单
const VERSION_SOURCES: &[&str] = &[
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json",
    "https://bmclapi2.bangbang93.com/mc/game/version_manifest.json",
];

fn cache_file(app: &AppHandle) -> PathBuf {
    settings::user_data_dir(app).join("version_manifest.json")
}

/// 对应原版 parseVersionCatalog：校验条目（id/type/releaseTime/url），错误文案原样
pub fn parse_version_catalog(data: &Value) -> Result<Vec<Value>, String> {
    let Some(entries) = data.get("versions").and_then(|v| v.as_array()) else {
        return Err("版本清单格式无效".into());
    };
    let mut versions: Vec<Value> = Vec::new();
    for value in entries {
        let id = value.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let vtype = value.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let release_time = value.get("releaseTime").and_then(|v| v.as_str()).unwrap_or("");
        let url = value.get("url").and_then(|v| v.as_str()).unwrap_or("");
        let type_ok = ["release", "snapshot", "old_beta", "old_alpha"].contains(&vtype);
        let time_ok = chrono::DateTime::parse_from_rfc3339(release_time).is_ok()
            || chrono::NaiveDateTime::parse_from_str(release_time, "%Y-%m-%dT%H:%M:%S%.fZ").is_ok();
        let url_ok = url.starts_with("https://") || url.starts_with("http://");
        if id.is_empty() || !type_ok || !time_ok || !url_ok {
            continue;
        }
        versions.push(json!({
            "id": id,
            "type": vtype,
            "url": url,
            "releaseTime": release_time
        }));
    }
    if versions.is_empty() {
        return Err("版本清单为空或损坏".into());
    }
    Ok(versions)
}

fn release_time_ms(v: &Value) -> i64 {
    v["releaseTime"]
        .as_str()
        .and_then(|t| {
            chrono::DateTime::parse_from_rfc3339(t)
                .ok()
                .or_else(|| chrono::NaiveDateTime::parse_from_str(t, "%Y-%m-%dT%H:%M:%S%.fZ").ok()
                    .map(|n| n.and_utc().fixed_offset()))
                .map(|d| d.timestamp_millis())
        })
        .unwrap_or(0)
}

/// 对应原版 mergeVersionCatalogs：先到者优先（官方在前），按发布时间倒序
pub fn merge_version_catalogs(catalogs: Vec<Vec<Value>>) -> Vec<Value> {
    let mut map: std::collections::HashMap<String, Value> = std::collections::HashMap::new();
    for list in catalogs {
        for version in list {
            let id = version["id"].as_str().unwrap_or("").to_string();
            if !map.contains_key(&id) {
                map.insert(id, version);
            }
        }
    }
    let mut out: Vec<Value> = map.into_values().collect();
    out.sort_by(|a, b| {
        let ta = release_time_ms(a);
        let tb = release_time_ms(b);
        tb.cmp(&ta).then_with(|| {
            let ia = a["id"].as_str().unwrap_or("");
            let ib = b["id"].as_str().unwrap_or("");
            ib.cmp(ia)
        })
    });
    out
}

fn fetch_source(url: &str) -> Result<Vec<Value>, String> {
    let url = url.to_string();
    run_blocking(move || {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(8))
            .build()
            .map_err(|e| e.to_string())?;
        let resp = client
            .get(&url)
            .header("Cache-Control", "no-cache")
            .send()
            .map_err(|e| e.to_string())?;
        let status = resp.status().as_u16();
        let text = resp.text().map_err(|e| e.to_string())?;
        if !(200..300).contains(&status) {
            return Err(format!("清单 HTTP {}", status));
        }
        let data: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        parse_version_catalog(&data)
    })
}

/// 对应原版 fetchVersionCatalog：缓存读取（TTL）→ 双源并发抓取 → 合并落盘
/// 返回 {versions, checkedAt, stale}。mirror 设置决定 stale 判定的参照源顺序
/// （bmclapi 模式下官方源仍参与抓取合并，仅官方优先级保留 —— 与原版一致）。
pub fn fetch_version_catalog(app: &AppHandle, refresh: bool) -> Result<Value, String> {
    let file = cache_file(app);
    // 缓存读取（schema 2 + TTL）
    let cached: Option<Value> = std::fs::read_to_string(&file)
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|raw| {
            let versions = parse_version_catalog(&raw).ok()?;
            let checked_at = raw["checkedAt"].as_i64().or_else(|| raw["fetchedAt"].as_i64()).unwrap_or(0);
            Some(json!({
                "versions": versions,
                "checkedAt": checked_at,
                "stale": raw["stale"] == true,
                "__schema": raw["schema"].as_i64().unwrap_or(0)
            }))
        });
    let now = chrono::Utc::now().timestamp_millis();
    if let Some(c) = &cached {
        let schema_ok = c["__schema"].as_i64() == Some(2);
        let fresh = now >= c["checkedAt"].as_i64().unwrap_or(0)
            && now - c["checkedAt"].as_i64().unwrap_or(0) < TTL_MS;
        if !refresh && schema_ok && fresh {
            let mut out = c.clone();
            if let Some(obj) = out.as_object_mut() {
                obj.remove("__schema");
            }
            return Ok(out);
        }
    }
    // 双源并发抓取（官方在前，bmclapi 跟随；对应原版 Promise.allSettled）
    let results: Vec<Result<Vec<Value>, String>> = std::thread::scope(|s| {
        let handles: Vec<_> = VERSION_SOURCES
            .iter()
            .map(|url| s.spawn(move || fetch_source(url)))
            .collect();
        handles.into_iter().map(|h| h.join().unwrap_or(Err("线程崩溃".into()))).collect()
    });
    let valid: Vec<Vec<Value>> = results.iter().filter_map(|r| r.as_ref().ok().cloned()).collect();
    if valid.is_empty() {
        if let Some(c) = cached {
            // 镜像中断期保留已知版本，标记 stale
            let versions = c["versions"].as_array().cloned().unwrap_or_default();
            return Ok(json!({ "versions": versions, "checkedAt": c["checkedAt"], "stale": true }));
        }
        return Err("无法获取版本清单，请检查网络后重试".into());
    }
    // 官方新数据优先去重；滞后的缓存条目保留在镜像缺失时
    let mut all = valid.clone();
    if let Some(c) = &cached {
        all.push(c["versions"].as_array().cloned().unwrap_or_default());
    }
    let versions = merge_version_catalogs(all);
    // stale：官方源（首源）失败（对应原版 results[0].status !== 'fulfilled'）
    let official_first_failed = results.first().map(|r| r.is_err()).unwrap_or(true);
    let stale = official_first_failed;
    let catalog = json!({ "versions": versions, "checkedAt": now, "stale": stale });
    // 原子落盘（tmp + rename）
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut record = catalog.clone();
    if let Some(obj) = record.as_object_mut() {
        obj.insert("schema".into(), json!(2));
    }
    let tmp = file.with_extension(format!("json.{}.tmp", crate::accounts::uuid_v4()));
    if std::fs::write(&tmp, serde_json::to_string(&record).unwrap_or_default()).is_ok() {
        let _ = std::fs::rename(&tmp, &file);
    }
    let _ = std::fs::remove_file(&tmp);
    Ok(catalog)
}

/// 对应原版 versions.ts fetchVersionManifest：清单的 versions 数组
pub fn fetch_version_manifest(app: &AppHandle, refresh: bool) -> Result<Value, String> {
    let catalog = fetch_version_catalog(app, refresh)?;
    Ok(catalog["versions"].clone())
}

/// 统一入口（PORT_CONVENTIONS 铁律 2）
pub fn invoke(app: &tauri::AppHandle, channel: &str, args: &[Value]) -> Option<Result<Value, String>> {
    match channel {
        "versions:catalog" => Some(fetch_version_catalog(app, args.first().and_then(|v| v.as_bool()).unwrap_or(false))),
        "versions:manifest" => Some(fetch_version_manifest(app, args.first().and_then(|v| v.as_bool()).unwrap_or(false))),
        _ => None,
    }
}
