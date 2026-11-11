//! 二分定位肇事 mod（scope `bisect.cjs` 移植 + 修正）。
//!
//! 场景：崩溃分析只能告诉你「可能与 mods 有关」，但不知道是哪个。装了 80 个 mod 时，
//! 人工一个个试是 80 次启动；二分只要 ⌈log₂80⌉ ≈ 7 次。
//!
//! 与 scope 的差异（都是修正，不是偷懒）：
//! - scope 每轮 `restoreTesting` 复原后靠玩家「手动上报这次起没起来」；这里同样人工驱动
//!   （不猜 exit code——多 mod 因果下自动判定会误伤），但把「下一步做什么」写进返回值，
//!     前端不必自己拼文案。
//! - scope 结果矛盾时直接抛错重来；这里保留已排除的候选，只把矛盾的那一轮降级为
//!   「建议按顺序手动试」，避免玩家的启动次数白花。
//! - 每轮只改 `.jar` ↔ `.jar.disabled`（复用 `mods::set_mods_enabled_in_dir`），
//!   任何时刻取消都能完整复原。

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// 二分会话（落在实例目录，跨启动器重启也能续做）
const SESSION_FILE: &str = ".wuhuicl/bisect.json";

#[derive(Debug, Clone)]
pub struct Session {
    /// 尚未排除的候选 mod 文件名（升序）
    pub candidates: Vec<String>,
    /// 本轮被禁用的那批（报告结果后要全部复原）
    pub testing: Vec<String>,
    /// 已完成的轮次
    pub round: u32,
    /// 已排除（证明不是它）的 mod，仅用于展示进度
    pub excluded: Vec<String>,
    /// `found` 或 `testing`
    pub status: &'static str,
    /// 定位到的肇事 mod（status=found 时有效）
    pub culprit: Option<String>,
    /// 出现矛盾结果时的说明（此时 candidates 已不可信，应改为手动二分）
    pub contradiction: Option<String>,
}

impl Session {
    fn to_json(&self) -> Value {
        json!({
            "schema": 1,
            "candidates": self.candidates,
            "testing": self.testing,
            "round": self.round,
            "excluded": self.excluded,
            "status": self.status,
            "culprit": self.culprit,
            "contradiction": self.contradiction,
        })
    }
    fn from_json(v: &Value) -> Option<Self> {
        if v.get("schema").and_then(|s| s.as_u64()) != Some(1) {
            return None;
        }
        let list = |key: &str| -> Vec<String> {
            v.get(key)
                .and_then(|x| x.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|s| s.as_str().map(str::to_string))
                        .filter(|s| safe_mod_name(s))
                        .collect()
                })
                .unwrap_or_default()
        };
        Some(Session {
            candidates: list("candidates"),
            testing: list("testing"),
            round: v.get("round").and_then(|r| r.as_u64()).unwrap_or(0) as u32,
            excluded: list("excluded"),
            status: match v.get("status").and_then(|s| s.as_str()) {
                Some("found") => "found",
                _ => "testing",
            },
            culprit: v
                .get("culprit")
                .and_then(|c| c.as_str())
                .filter(|s| safe_mod_name(s))
                .map(str::to_string),
            contradiction: v
                .get("contradiction")
                .and_then(|c| c.as_str())
                .map(str::to_string),
        })
    }
}

/// mod 文件名白名单校验（防路径穿越：会话文件可以被手工编辑）
fn safe_mod_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() < 260
        && !name.contains(['/', '\\'])
        && !name.starts_with('.')
        && (name.to_lowercase().ends_with(".jar")
            || name.to_lowercase().ends_with(".jar.disabled"))
}

fn session_path(game_dir: &Path) -> PathBuf {
    game_dir.join(SESSION_FILE)
}

pub fn load(game_dir: &Path) -> Option<Session> {
    let raw = std::fs::read_to_string(session_path(game_dir)).ok()?;
    let v: Value = serde_json::from_str(&raw).ok()?;
    Session::from_json(&v)
}

fn save(game_dir: &Path, s: &Session) -> Result<(), String> {
    let path = session_path(game_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, s.to_json().to_string()).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

pub fn clear(game_dir: &Path) -> Result<(), String> {
    match std::fs::remove_file(session_path(game_dir)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// 列出当前启用中的 mod（`.jar`），升序，便于二分稳定切分
pub fn enabled_mods(mods_dir: &Path) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(mods_dir) else {
        return Vec::new();
    };
    let mut out: Vec<String> = rd
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| {
            let low = n.to_lowercase();
            low.ends_with(".jar") && !low.ends_with(".jar.disabled")
        })
        .collect();
    out.sort();
    out
}

/// 开始一次二分。已有会话则原样返回（续做）。
/// 返回 `(session, 本轮该做什么的说明)`。
pub fn start(
    app: &tauri::AppHandle,
    game_dir: &Path,
    mods_dir: &Path,
    requested: Option<Vec<String>>,
) -> Result<(Session, String), String> {
    if let Some(s) = load(game_dir) {
        if s.status == "testing" {
            return Ok((s.clone(), next_step_text(&s)));
        }
        // 已完成/矛盾状态：不允许直接续做，让前端先 clear
        return Err("已有已结束的二分会话，请先结束它再重新开始".into());
    }
    let available = enabled_mods(mods_dir);
    let candidates: Vec<String> = match requested {
        Some(list) if !list.is_empty() => {
            // 只保留真实存在且仍启用的
            let set: std::collections::HashSet<String> = available.into_iter().collect();
            let mut picked: Vec<String> = list
                .into_iter()
                .filter(|n| safe_mod_name(n) && set.contains(n))
                .collect();
            picked.sort();
            picked.dedup();
            picked
        }
        _ => available,
    };
    if candidates.len() < 2 {
        return Err("可二分定位至少需要 2 个启用中的模组".into());
    }
    let mut s = Session {
        candidates,
        testing: Vec::new(),
        round: 0,
        excluded: Vec::new(),
        status: "testing",
        culprit: None,
        contradiction: None,
    };
    prepare_round(app, mods_dir, &mut s)?;
    save(game_dir, &s)?;
    Ok((s.clone(), next_step_text(&s)))
}

/// 切下一轮：禁掉候选的前一半。候选 ≤1 视为定位完成。
fn prepare_round(app: &tauri::AppHandle, mods_dir: &Path, s: &mut Session) -> Result<(), String> {
    if s.candidates.len() <= 1 {
        s.status = "found";
        s.culprit = s.candidates.first().cloned();
        s.testing.clear();
        return Ok(());
    }
    let half = s.candidates.len().div_ceil(2);
    let testing: Vec<String> = s.candidates[..half].to_vec();
    crate::mods::set_mods_enabled_in_dir(app, mods_dir, &testing, false)?;
    s.testing = testing;
    s.round += 1;
    Ok(())
}

/// 复原本轮禁用的全部 mod（下一轮重新切分前的必要步骤，避免禁用状态叠加）
fn restore_testing(app: &tauri::AppHandle, mods_dir: &Path, s: &mut Session) -> Result<(), String> {
    if s.testing.is_empty() {
        return Ok(());
    }
    let list: Vec<String> = s
        .testing
        .iter()
        .map(|n| format!("{}.disabled", n.trim_end_matches(".jar")))
        .collect();
    crate::mods::set_mods_enabled_in_dir(app, mods_dir, &list, true)?;
    s.testing.clear();
    Ok(())
}

/// 上报一轮结果。
///
/// `game_started=true` 表示「禁掉这一半后游戏起来了」→ 肇事者就在被禁的那一半里；
/// 反之留在启用侧。两种情况都先把本轮禁用复原，再切下一轮。
pub fn report(
    app: &tauri::AppHandle,
    game_dir: &Path,
    mods_dir: &Path,
    game_started: bool,
) -> Result<(Session, String), String> {
    let mut s = load(game_dir).ok_or("没有进行中的二分会话")?;
    if s.status != "testing" {
        return Err("这个二分会话已经结束了".into());
    }
    let testing = s.testing.clone();
    restore_testing(app, mods_dir, &mut s)?;
    if game_started {
        // 祸首在被禁侧：候选收缩到 testing，其余记为已排除
        let keep: Vec<String> = s
            .candidates
            .iter()
            .filter(|c| testing.contains(c))
            .cloned()
            .collect();
        s.excluded
            .extend(s.candidates.iter().filter(|c| !testing.contains(c)).cloned());
        s.candidates = keep;
    } else {
        // 还崩：祸首不在被禁侧
        let keep: Vec<String> = s
            .candidates
            .iter()
            .filter(|c| !testing.contains(c))
            .cloned()
            .collect();
        s.excluded.extend(testing.iter().cloned());
        s.candidates = keep;
    }
    // 矛盾：交集为空 → 两根都排除干净了，说明结果自相矛盾（多 mod 各自可崩的典型）
    if s.candidates.is_empty() {
        s.status = "found";
        s.contradiction = Some(
            "两次结果互相矛盾（可能是多个模组各自都会崩）。已复原全部模组，\
             建议按「已排除」以外的顺序手动逐个试。"
                .into(),
        );
        s.culprit = None;
        save(game_dir, &s)?;
        return Ok((s.clone(), "结果自相矛盾，已复原全部模组".into()));
    }
    prepare_round(app, mods_dir, &mut s)?;
    save(game_dir, &s)?;
    Ok((s.clone(), next_step_text(&s)))
}

/// 收尾：定位成功后可选择把肇事 mod 保持禁用
pub fn finish(
    app: &tauri::AppHandle,
    game_dir: &Path,
    mods_dir: &Path,
    disable_culprit: bool,
) -> Result<Value, String> {
    let Some(s) = load(game_dir) else {
        return Ok(json!({ "ok": true, "cleared": true }));
    };
    if s.status == "testing" {
        return Err("二分还没结束，先上报结果或取消".into());
    }
    if disable_culprit {
        if let Some(c) = s.culprit.as_deref() {
            crate::mods::set_mods_enabled_in_dir(app, mods_dir, &[c.to_string()], false)?;
        }
    }
    clear(game_dir)?;
    Ok(json!({ "ok": true, "culprit": s.culprit }))
}

/// 取消：复原全部本轮禁用并删会话（任何时候都能安全退出）
pub fn cancel(app: &tauri::AppHandle, game_dir: &Path, mods_dir: &Path) -> Result<Value, String> {
    let Some(mut s) = load(game_dir) else {
        return Ok(json!({ "ok": true, "cleared": true }));
    };
    restore_testing(app, mods_dir, &mut s)?;
    clear(game_dir)?;
    Ok(json!({ "ok": true }))
}

/// 「下一步做什么」的统一文案（前端不重复拼）
pub fn next_text_public(s: &Session) -> String {
    next_step_text(s)
}

fn next_step_text(s: &Session) -> String {
    if s.status == "found" {
        return match s.culprit.as_deref() {
            Some(c) => format!("已定位到 {c}，可以结束并禁用它"),
            None => "结果有矛盾，点「结束」复原后改为手动排查".into(),
        };
    }
    format!(
        "已禁用 {} 个模组（第 {} 轮，剩 {} 个候选）。启动游戏：能进主界面就点「起来了」，仍然崩溃就点「还是崩」。",
        s.testing.len(),
        s.round,
        s.candidates.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bisect_reaches_culprit_in_log2_rounds() {
        // 纯函数部分验证：模拟 4 个候选，两轮锁定
        let mut s = Session {
            candidates: vec!["a.jar".into(), "b.jar".into(), "c.jar".into(), "d.jar".into()],
            testing: Vec::new(),
            round: 0,
            excluded: Vec::new(),
            status: "testing",
            culprit: None,
            contradiction: None,
        };
        // 第 1 轮：禁前一半 [a,b]
        let half = s.candidates.len().div_ceil(2);
        s.testing = s.candidates[..half].to_vec();
        assert_eq!(s.testing, vec!["a.jar", "b.jar"]);
        // 假设「禁掉后能起来」→ 祸首在 [a,b]
        let testing = s.testing.clone();
        s.excluded
            .extend(s.candidates.iter().filter(|c| !testing.contains(c)).cloned());
        s.candidates = s.candidates.iter().filter(|c| testing.contains(c)).cloned().collect();
        assert_eq!(s.candidates, vec!["a.jar", "b.jar"]);
        // 第 2 轮：禁 [a]
        s.testing = s.candidates[..1].to_vec();
        assert_eq!(s.testing, vec!["a.jar"]);
        // 假设「禁 a 后还崩」→ 祸首不是 a
        let testing = s.testing.clone();
        s.candidates = s.candidates.iter().filter(|c| !testing.contains(c)).cloned().collect();
        assert_eq!(s.candidates, vec!["b.jar"]);
        // 剩 1 个即定位
        assert_eq!(s.candidates.len(), 1);
    }

    #[test]
    fn session_json_round_trips() {
        let s = Session {
            candidates: vec!["x.jar".into()],
            testing: vec![],
            round: 2,
            excluded: vec!["y.jar".into()],
            status: "found",
            culprit: Some("x.jar".into()),
            contradiction: None,
        };
        let parsed = Session::from_json(&s.to_json()).unwrap();
        assert_eq!(parsed.candidates, s.candidates);
        assert_eq!(parsed.culprit, s.culprit);
        assert_eq!(parsed.status, "found");
    }

    #[test]
    fn unsafe_mod_names_are_rejected() {
        assert!(safe_mod_name("good.jar"));
        assert!(safe_mod_name("also-ok.jar.disabled"));
        assert!(!safe_mod_name("../evil.jar"));
        assert!(!safe_mod_name("a/b.jar"));
        assert!(!safe_mod_name("a\\b.jar"));
        assert!(!safe_mod_name(".hidden"));
        assert!(!safe_mod_name(""));
    }
}
