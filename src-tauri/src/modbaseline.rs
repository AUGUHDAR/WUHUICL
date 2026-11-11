//! mod 基线快照与「最近变更」嫌疑（对应 scope `guard.cjs` 的 mods-baseline + recentModChanges）。
//!
//! 思路：每次启动游戏前为实例的 mods 目录拍一份轻量快照（文件名 + 尺寸 + mtime），
//! 崩溃时与上一份基线对比，得出「这次启动前刚新增/改动/删除」的 mod 清单。
//! 这是崩溃归因里最直观的一条证据——用户最常说的就是「昨天还玩得好好的，今天加了个
//! mod 就崩了」，而纯日志规则匹配答不了这个问题。
//!
//! 只记录元数据不读 jar 内容，所以开销是每次启动一次 readdir，可以忽略。

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// 实例的基线文件（放在游戏目录内的受管子目录，不污染 mods/）
const BASELINE_DIR: &str = ".wuhuicl";
const BASELINE_FILE: &str = "mods-baseline.json";
/// 基线只保留这么多历史，避免无限增长
const BASELINE_KEEP: usize = 20;

/// 一个 mod 文件的轻量身份：改名也算变动，所以用文件名 + 尺寸 + mtime 秒
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModStamp {
    pub name: String,
    pub size: u64,
    pub mtime: u64,
}

/// 一次基线对比的结果
#[derive(Debug, Default)]
pub struct ModChanges {
    /// 本次启动前新增的文件名
    pub added: Vec<String>,
    /// 本次启动前被改动（同名但尺寸/时间变了）的文件名
    pub changed: Vec<String>,
    /// 被删除的文件名
    pub removed: Vec<String>,
}

impl ModChanges {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.changed.is_empty() && self.removed.is_empty()
    }

    /// 给崩溃原因列表用的一行摘要；无变更时返回 None
    pub fn summary(&self) -> Option<String> {
        if self.is_empty() {
            return None;
        }
        let mut parts: Vec<String> = Vec::new();
        if !self.added.is_empty() {
            parts.push(format!("新增 {}", join_names(&self.added)));
        }
        if !self.changed.is_empty() {
            parts.push(format!("改动 {}", join_names(&self.changed)));
        }
        if !self.removed.is_empty() {
            parts.push(format!("移除 {}", join_names(&self.removed)));
        }
        Some(format!("自上次启动以来 mods 有变动：{}", parts.join("；")))
    }
}

/// 超过这个数量只列前几个，避免弹窗被几百个文件名撑爆
const NAME_LIMIT: usize = 6;

fn join_names(names: &[String]) -> String {
    if names.len() <= NAME_LIMIT {
        names.join("、")
    } else {
        format!(
            "{} 等 {} 个",
            names[..NAME_LIMIT].join("、"),
            names.len()
        )
    }
}

fn baseline_path(mods_dir: &Path) -> PathBuf {
    // mods/ 的兄弟目录：<game_dir>/.wuhuicl/mods-baseline.json
    // 用 mods 的父目录，因为 mods 本身就是游戏目录下的子目录
    let base = mods_dir
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(BASELINE_DIR);
    base.join(BASELINE_FILE)
}

/// 扫描 mods 目录当前状态。只收 `.jar`（含 `.jar.disabled`，因为禁用也是变动）
pub fn scan_mods(mods_dir: &Path) -> Vec<ModStamp> {
    let mut out: Vec<ModStamp> = Vec::new();
    let Ok(rd) = std::fs::read_dir(mods_dir) else {
        return out;
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if !name.to_lowercase().ends_with(".jar") && !name.to_lowercase().ends_with(".jar.disabled")
        {
            continue;
        }
        let Ok(meta) = e.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        out.push(ModStamp {
            name,
            size: meta.len(),
            mtime,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn stamps_to_json(stamps: &[ModStamp]) -> Value {
    json!(stamps
        .iter()
        .map(|s| json!({ "name": s.name, "size": s.size, "mtime": s.mtime }))
        .collect::<Vec<_>>())
}

fn stamps_from_json(v: &Value) -> Vec<ModStamp> {
    v.as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|s| {
                    Some(ModStamp {
                        name: s["name"].as_str()?.to_string(),
                        size: s["size"].as_u64().unwrap_or(0),
                        mtime: s["mtime"].as_u64().unwrap_or(0),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 纯函数：两份快照对比出变更。便于单测，也便于在崩溃之外复用。
pub fn diff_stamps(before: &[ModStamp], after: &[ModStamp]) -> ModChanges {
    let mut out = ModChanges::default();
    let before_map: std::collections::HashMap<&str, &ModStamp> =
        before.iter().map(|s| (s.name.as_str(), s)).collect();
    let after_map: std::collections::HashMap<&str, &ModStamp> =
        after.iter().map(|s| (s.name.as_str(), s)).collect();
    for s in after {
        match before_map.get(s.name.as_str()) {
            // 同名但尺寸或时间变了 → 改动（换版本、覆盖同名文件）
            Some(old) if old.size != s.size || old.mtime != s.mtime => {
                out.changed.push(s.name.clone());
            }
            Some(_) => {}
            // 之前没有 → 新增
            None => out.added.push(s.name.clone()),
        }
    }
    for s in before {
        if !after_map.contains_key(s.name.as_str()) {
            out.removed.push(s.name.clone());
        }
    }
    out.added.sort();
    out.changed.sort();
    out.removed.sort();
    out
}

/// 读取最近一份基线（无则 None）
pub fn load_baseline(mods_dir: &Path) -> Option<Vec<ModStamp>> {
    let path = baseline_path(mods_dir);
    let raw = std::fs::read_to_string(&path).ok()?;
    let doc: Value = serde_json::from_str(&raw).ok()?;
    // 结构：{ "entries": [ { "at": "...", "mods": [...] }, ... ] }，最后一条为最新
    let entries = doc.get("entries")?.as_array()?;
    entries.last().map(|e| stamps_from_json(&e["mods"]))
}

/// 游戏启动前调用：对比上一份基线并写入本次快照。
/// 返回本次启动前的变更（用于崩溃归因）；第一次运行无基线时返回空。
pub fn snapshot_before_launch(mods_dir: &Path) -> ModChanges {
    let current = scan_mods(mods_dir);
    let changes = load_baseline(mods_dir)
        .map(|prev| diff_stamps(&prev, &current))
        .unwrap_or_default();
    let _ = write_baseline(mods_dir, &current);
    // 变更同步落一份到磁盘：崩溃分析在另一个线程/时刻，直接从文件读回更可靠
    let _ = write_pending_changes(mods_dir, &changes);
    changes
}

fn write_baseline(mods_dir: &Path, stamps: &[ModStamp]) -> Result<(), String> {
    let path = baseline_path(mods_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut entries: Vec<Value> = std::fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|doc| doc.get("entries").and_then(|e| e.as_array()).cloned())
        .unwrap_or_default();
    entries.push(json!({
        "at": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        "mods": stamps_to_json(stamps),
    }));
    if entries.len() > BASELINE_KEEP {
        let drop = entries.len() - BASELINE_KEEP;
        entries.drain(0..drop);
    }
    let doc = json!({ "schemaVersion": 1, "entries": entries });
    let text = serde_json::to_string(&doc).map_err(|e| e.to_string())?;
    // 原子写：先写临时文件再改名，避免启动瞬间被读到半截 JSON
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

fn pending_path(mods_dir: &Path) -> PathBuf {
    let base = mods_dir
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(BASELINE_DIR);
    base.join("mods-changes.json")
}

fn write_pending_changes(mods_dir: &Path, changes: &ModChanges) -> Result<(), String> {
    let path = pending_path(mods_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let doc = json!({
        "added": changes.added,
        "changed": changes.changed,
        "removed": changes.removed,
    });
    std::fs::write(&path, serde_json::to_string(&doc).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}

/// 崩溃分析时调用：读回本次启动前记录的变更。读不到（第一次跑或文件坏了）返回空。
pub fn read_pending_changes(mods_dir: &Path) -> ModChanges {
    let path = pending_path(mods_dir);
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return ModChanges::default();
    };
    let Ok(doc) = serde_json::from_str::<Value>(&raw) else {
        return ModChanges::default();
    };
    let list = |key: &str| -> Vec<String> {
        doc.get(key)
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect())
            .unwrap_or_default()
    };
    ModChanges {
        added: list("added"),
        changed: list("changed"),
        removed: list("removed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stamp(name: &str, size: u64, mtime: u64) -> ModStamp {
        ModStamp {
            name: name.into(),
            size,
            mtime,
        }
    }

    #[test]
    fn added_and_removed_are_detected() {
        let before = vec![stamp("a.jar", 10, 100), stamp("gone.jar", 5, 50)];
        let after = vec![stamp("a.jar", 10, 100), stamp("new.jar", 7, 200)];
        let d = diff_stamps(&before, &after);
        assert_eq!(d.added, vec!["new.jar"]);
        assert!(d.changed.is_empty());
        assert_eq!(d.removed, vec!["gone.jar"]);
    }

    #[test]
    fn same_name_new_size_is_a_change() {
        let before = vec![stamp("a.jar", 10, 100)];
        let after = vec![stamp("a.jar", 12, 100)];
        let d = diff_stamps(&before, &after);
        assert!(d.added.is_empty());
        assert_eq!(d.changed, vec!["a.jar"]);
        assert!(d.removed.is_empty());
    }

    #[test]
    fn identical_snapshots_report_nothing() {
        let s = vec![stamp("a.jar", 10, 100), stamp("b.jar", 20, 200)];
        assert!(diff_stamps(&s, &s).is_empty());
    }

    #[test]
    fn summary_mentions_each_kind() {
        let d = ModChanges {
            added: vec!["new.jar".into()],
            changed: vec!["b.jar".into()],
            removed: vec!["gone.jar".into()],
        };
        let text = d.summary().unwrap();
        assert!(text.contains("新增 new.jar"));
        assert!(text.contains("改动 b.jar"));
        assert!(text.contains("移除 gone.jar"));
    }

    #[test]
    fn long_lists_are_truncated() {
        let d = ModChanges {
            added: (0..10).map(|i| format!("m{i}.jar")).collect(),
            ..Default::default()
        };
        let text = d.summary().unwrap();
        assert!(text.contains("等 10 个"));
    }
}
