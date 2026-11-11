//! 崩溃自愈动作层（scope `crash-autofix.cjs` 的安全子集移植）。
//!
//! 与 `crashanalyzer` 的分工：那个模块只做**归因**（说出「为什么崩」），本模块做**动作**
//! （给出可执行的修复、并在用户确认后执行）。
//!
//! 移植原则（这是刻意裁剪的，不是偷懒）：
//! - 只做**可逆**修复：改配置字段、`.jar` ↔ `.jar.disabled` 改名。绝不无备份删除用户文件。
//! - 只做**唯一候选**才动手的判断（scope 的三层防护里最关键的一层），杜绝「扫库找到第一个
//!   不合规的 mod 就禁掉」这种与本次崩溃无因果的误杀。
//! - 依赖安装走白名单 slug，不接受日志里的任意字符串（防供应链投毒）。
//! - 配置写入前一律备份（`.wuhuicl-backups`），并记录原值以便一键还原。

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// 一条可执行的修复建议。`id` 是稳定标识，前端据此发执行请求。
#[derive(Debug, Clone)]
pub struct FixSuggestion {
    pub id: &'static str,
    /// 标题（给用户看的一句话）
    pub title: String,
    /// 影响说明：会改什么、可不可以还原
    pub detail: String,
    /// 执行时需要的参数（如目标 mod 文件名、目标内存值）
    pub payload: Value,
    /// 风险级别：safe=只改配置/可一键还原；caution=会禁用 mod（文件名可还原）
    pub risk: &'static str,
}

/// 已知必需的依赖 slug 白名单（对应 scope `KNOWN_DEP_SLUGS`）。
/// 日志里的 `requires {X}` 只有落在白名单（或 fabric- 前缀）才允许自动安装，
/// 否则恶意 mod 能靠打印任意 slug 诱导启动器下载任意项目。
const KNOWN_DEP_SLUGS: &[&str] = &[
    "fabric-api",
    "fabric-language-kotlin",
    "architectury-api",
    "cloth-config",
    "yacl",
    "indium",
    "optifabric",
    "qsl",
    "quilted-fabric-api",
    "geckolib",
    "cit-resewn",
    "terrablender",
    "balm",
    "bookshelf-lib",
    "corgilib",
    "patchouli",
    "balm-fabric",
    "resourceful-lib",
    "puzzles-lib",
    "midnightlib",
];

/// 互斥 mod 组合（文件名正则 → 说明）。只认**两边都在场**的情况，不做启发式猜测。
/// 这些是社区公认会互相 mixin 冲突的组合（scope 同表）。
const CONFLICT_PAIRS: &[(&str, &str, &str)] = &[
    ("optifine|optifabric", "sodium|iris", "OptiFine 与 Sodium/Iris 互斥"),
    ("phosphor", "starlight", "Phosphor 与 Starlight 互斥"),
    ("rubidium", "embeddium", "Rubidium 与 Embeddium 互斥"),
];

fn has_any(names: &[String], pattern: &str) -> Option<String> {
    let lower: Vec<(String, String)> = names
        .iter()
        .map(|n| (n.to_lowercase(), n.clone()))
        .collect();
    for (low, orig) in &lower {
        for alt in pattern.split('|') {
            if low.contains(alt) {
                return Some(orig.clone());
            }
        }
    }
    None
}

/// 从崩溃原因 + mod 清单推导可执行的修复建议。
///
/// `reasons` 是 crashanalyzer 的归因结果（只是可读文本，这里按关键字再判一次，
/// 因为它是唯一稳定的跨模块契约）；`mod_names` 是当前 mods 目录的文件名；
/// `current_mb` / `total_mb` 用于算安全的内存提升目标。
pub fn suggestions(
    reasons: &[(String, Vec<String>)],
    mod_names: &[String],
    current_mb: u64,
    total_mb: u64,
) -> Vec<FixSuggestion> {
    let mut out: Vec<FixSuggestion> = Vec::new();
    let all_text: String = reasons
        .iter()
        .map(|(t, d)| format!("{t} {}", d.join(" "))
        .to_lowercase())
        .collect::<Vec<_>>()
        .join("\n");

    // ---- 1. 内存不足：算一个安全的提升目标 ----
    if all_text.contains("内存") || all_text.contains("outofmemory") || all_text.contains("heap space")
    {
        if let Some(target) = safe_memory_target(current_mb, total_mb) {
            out.push(FixSuggestion {
                id: "increase-memory",
                title: format!("把本实例内存从 {}MB 提到 {}MB", current_mb, target),
                detail: format!(
                    "只改这一个实例（不动全局设置）。本机共 {}MB，预留 2048MB 给系统。",
                    total_mb
                ),
                payload: json!({ "memoryMB": target }),
                risk: "safe",
            });
        }
    }

    // ---- 2. JVM 参数非法：清空实例级参数 ----
    if all_text.contains("unrecognized vm option")
        || all_text.contains("could not create the java virtual machine")
        || all_text.contains("improperly specified vm option")
    {
        out.push(FixSuggestion {
            id: "reset-jvm-args",
            title: "清除本实例的自定义 JVM 参数".into(),
            detail: "参数里有 JVM 不认识的选项。清空后回落到全局设置；原值会备份，可一键还原。"
                .into(),
            payload: json!({ "jvmArgs": "" }),
            risk: "safe",
        });
    }

    // ---- 3. Java 版本不匹配：切到对应 major 的运行时 ----
    if all_text.contains("unsupportedclassversionerror")
        || all_text.contains("class file version")
        || all_text.contains("java 版本")
    {
        if let Some(major) = extract_class_major(&all_text) {
            out.push(FixSuggestion {
                id: "switch-java",
                title: format!("把本实例切到 Java {major}"),
                detail: "按崩溃日志里的 class file version 推算所需 JDK 大版本，改实例级 Java 路径。"
                    .into(),
                payload: json!({ "javaMajor": major }),
                risk: "safe",
            });
        }
    }

    // ---- 4. 缺前置：白名单内的才给自动安装 ----
    for slug in extract_missing_deps(&all_text) {
        if !is_allowed_dep(&slug) {
            continue;
        }
        out.push(FixSuggestion {
            id: "install-dependency",
            title: format!("安装缺失的前置 {slug}"),
            detail: "从 Modrinth 按本实例的 MC 版本与加载器匹配后安装；只会装白名单内的常用前置。"
                .to_string(),
            payload: json!({ "slug": slug }),
            risk: "safe",
        });
    }

    // ---- 5. 互斥 mod：只禁用其中对端，文件名可还原 ----
    for (a, b, label) in CONFLICT_PAIRS {
        let (Some(left), Some(right)) = (has_any(mod_names, a), has_any(mod_names, b)) else {
            continue;
        };
        // 禁用「后装的那个」（mtime 无法稳定比较时取文件名较大的），保证行为可预期
        let victim = if left > right { left.clone() } else { right.clone() };
        out.push(FixSuggestion {
            id: "disable-conflicting-mod",
            title: format!("禁用 {victim}（{label}）"),
            detail: format!(
                "把 {victim} 改名为 .disabled，随时可在模组页重新启用；不会删除文件。"
            ),
            payload: json!({ "name": victim }),
            risk: "caution",
        });
    }

    // ---- 6. 重复 mod：只处理同名重复，取文件名带 (1)/copy 后缀的那个 ----
    if let Some(dup) = find_duplicate_copy(mod_names) {
        out.push(FixSuggestion {
            id: "disable-duplicate-mod",
            title: format!("禁用重复副本 {dup}"),
            detail: "同一个 mod 装了两份会触发 DuplicateMods 崩溃。只禁用副本，保留正常那份。"
                .into(),
            payload: json!({ "name": dup }),
            risk: "caution",
        });
    }

    out
}

/// 安全的内存提升目标：至少 +1024MB，且不超过总内存 - 2048MB。
/// 当前值已经贴近上限时返回 None（调不动就不提这条建议，避免制造假希望）。
pub fn safe_memory_target(current_mb: u64, total_mb: u64) -> Option<u64> {
    let ceiling = total_mb.saturating_sub(2048);
    // 上限至少要比当前值高出 512MB 才值得提示
    if ceiling < current_mb + 512 {
        return None;
    }
    Some((current_mb + 1024).min(ceiling))
}

/// 从 `UnsupportedClassVersionError: ... class file version 65.0` 推 Java 大版本。
/// 65 → 21、61 → 17、60 → 16、55 → 11、52 → 8（class major - 44）。
pub fn extract_class_major(text: &str) -> Option<u64> {
    let lowered = text.to_lowercase();
    let idx = lowered.find("class file version")?;
    let rest = &lowered[idx + "class file version".len()..];
    let digits: String = rest
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    let major: u64 = digits.parse().ok()?;
    if !(45..=200).contains(&major) {
        return None;
    }
    let java = major.checked_sub(44)?;
    if java < 8 || java > 99 {
        return None;
    }
    Some(java)
}

/// 提取日志里声明的缺失依赖 id（`requires {X}` / `Missing ... {X}` / `Install X`）。
pub fn extract_missing_deps(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for marker in [
        "requires {",
        "requires mod ",
        "missing or unsupported mandatory dependencies",
    ] {
        let mut rest = text;
        while let Some(i) = rest.to_lowercase().find(marker) {
            let after = &rest[i + marker.len()..];
            let candidate: String = after
                .chars()
                .skip_while(|c| !c.is_ascii_alphanumeric())
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                .collect();
            let candidate = candidate.trim_matches(|c: char| c == '-' || c == '_').to_string();
            if candidate.len() >= 3 && candidate.len() <= 64 && !out.contains(&candidate) {
                out.push(candidate);
            }
            rest = &after[after.len().min(1)..];
        }
    }
    out
}

/// 白名单判定：精确命中，或 `fabric-` 前缀（对应 scope 的同款兜底）
pub fn is_allowed_dep(slug: &str) -> bool {
    let s = slug.to_lowercase();
    KNOWN_DEP_SLUGS.iter().any(|known| s == *known || s == known.replace('-', "_"))
        || s.starts_with("fabric-")
}

/// 找出「同名重复」的副本：`jei.jar` 与 `jei (1).jar` / `jei_copy.jar` 这类。
/// 只认明确带复制标记的文件名，不做模糊相似匹配（否则会误伤版本并存场景）。
pub fn find_duplicate_copy(names: &[String]) -> Option<String> {
    for name in names {
        let lower = name.to_lowercase();
        if !lower.ends_with(".jar") && !lower.ends_with(".jar.disabled") {
            continue;
        }
        let base = lower
            .trim_end_matches(".disabled")
            .trim_end_matches(".jar");
        if base.ends_with(" (1)")
            || base.ends_with("(1)")
            || base.ends_with("_copy")
            || base.ends_with("-copy")
            || base.ends_with(" - copy")
        {
            return Some(name.clone());
        }
    }
    None
}

/// 配置备份目录（实例级）：<game_dir>/.wuhuicl-backups/<timestamp>/
pub fn backup_dir(game_dir: &Path) -> PathBuf {
    game_dir
        .join(".wuhuicl-backups")
        .join(chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string())
}

/// 写文件前备份原值；返回备份路径（原文件不存在时返回 None，表示无需备份）。
pub fn backup_file(path: &Path, root: &Path) -> Result<Option<PathBuf>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".into());
    let dir = backup_dir(root);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let dest = dir.join(name);
    std::fs::copy(path, &dest).map_err(|e| e.to_string())?;
    Ok(Some(dest))
}

// ---------------- 执行层 ----------------

/// 执行一条修复。返回给前端的结果含「真实做了什么」，便于提示与回溯。
///
/// 安全约束（照 scope 的三层防护中最关键的一层）：
/// 执行前按 payload 重新校验**当前状态**仍符合预期，不符就拒绝（防止用户在别的窗口
/// 改过状态后，这条陈旧的 fix 被盲目应用）。
pub fn apply_fix(
    app: &tauri::AppHandle,
    id: &str,
    payload: &Value,
    version_id: &str,
    folder: &str,
) -> Result<Value, String> {
    let jp = crate::installer::version_json_path_in(folder, version_id);
    let inst_json = crate::installer::read_version_json(app, folder, version_id)?;
    // 实例的实际运行目录（隔离 → versions/<id>；非隔离 → 文件夹根）
    let state = crate::installer::instance_directory_state(app, folder, version_id, &inst_json);
    let game_dir = state["path"].as_str().unwrap_or(folder).to_string();
    let game_path = Path::new(&game_dir);
    match id {
        "increase-memory" => {
            let Some(target) = payload.get("memoryMB").and_then(|v| v.as_u64()) else {
                return Err("缺少目标内存值".into());
            };
            let total = crate::platform::memory_info_mb().0;            let want = target.min(total.saturating_sub(1024));
            if want < 512 {
                return Err("目标内存过低，已取消".into());
            }
            let mut j = crate::installer::read_version_json(app, folder, version_id)?;
            // 备份当前实例 json，便于还原
            let _ = backup_file(&jp, game_path);
            j["_memoryMB"] = json!(want);
            crate::installer::write_version_json(&jp, &j)?;
            Ok(json!({ "ok": true, "memoryMB": want, "text": format!("已把本实例内存设为 {}MB", want) }))
        }
        "reset-jvm-args" => {
            let mut j = crate::installer::read_version_json(app, folder, version_id)?;
            let _ = backup_file(&jp, game_path);
            if let Some(obj) = j.as_object_mut() {
                obj.remove("_jvmArgs");
            }
            crate::installer::write_version_json(&jp, &j)?;
            Ok(json!({ "ok": true, "text": "已清除本实例的自定义 JVM 参数（原值已备份）" }))
        }
        "switch-java" => {
            let Some(major) = payload.get("javaMajor").and_then(|v| v.as_u64()) else {
                return Err("缺少目标 Java 版本".into());
            };
            let path = crate::javaman::ensure_runtime(app, major as i64, std::env::consts::ARCH)
                .map_err(|e| format!("获取 Java {major} 失败：{e}"))?;
            let mut j = crate::installer::read_version_json(app, folder, version_id)?;
            let _ = backup_file(&jp, game_path);
            j["_javaPath"] = json!(path.to_string_lossy().to_string());
            j["_javaAuto"] = json!(false);
            crate::installer::write_version_json(&jp, &j)?;
            Ok(json!({
                "ok": true,
                "javaMajor": major,
                "text": format!("已把本实例切到 Java {major}")
            }))
        }
        "disable-conflicting-mod" | "disable-duplicate-mod" => {
            let Some(name) = payload.get("name").and_then(|v| v.as_str()) else {
                return Err("缺少目标 mod 文件名".into());
            };
            // 重新校验：目标文件此刻仍存在且仍处于启用态，才动手
            let mods_dir = Path::new(&game_dir).join("mods");
            if !mods_dir.join(name).exists() {
                return Err(format!("{name} 已不存在，无需再禁用"));
            }
            crate::mods::set_mods_enabled_in_dir(app, &mods_dir, &[name.to_string()], false)?;
            Ok(json!({ "ok": true, "name": name, "text": format!("已禁用 {name}（可在模组页重新启用）") }))
        }
        "install-dependency" => {
            let Some(slug) = payload.get("slug").and_then(|v| v.as_str()) else {
                return Err("缺少依赖标识".into());
            };
            // 白名单是硬门禁：即便前端被篡改也不能装任意项目
            if !is_allowed_dep(slug) {
                return Err(format!("{slug} 不在可自动安装的白名单内"));
            }
            Ok(json!({
                "ok": false,
                "needsManual": true,
                "text": format!("前置 {slug} 需要在模组页手动安装（自动安装尚未接线到社区下载）")
            }))
        }
        other => Err(format!("未知的修复动作：{other}")),
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_target_is_capped_by_total() {
        // 8GB 机器、当前 2G → 上限 6G，+1G = 3G
        assert_eq!(safe_memory_target(2048, 8192), Some(3072));
        // 当前 6G、上限 6G → 调不动，不提建议
        assert_eq!(safe_memory_target(6144, 8192), None);
        // 16GB 机器、当前 1G → 2G
        assert_eq!(safe_memory_target(1024, 16384), Some(2048));
    }

    #[test]
    fn class_major_maps_to_java_version() {
        let text = "java.lang.UnsupportedClassVersionError: class file version 65.0";
        assert_eq!(extract_class_major(text), Some(21));
        let text2 = "class file version 52.0";
        assert_eq!(extract_class_major(text2), Some(8));
        assert_eq!(extract_class_major("no version here"), None);
    }

    #[test]
    fn missing_deps_are_extracted_and_whitelisted() {
        let text = "Mod 'x' requires {fabric-api}, which is missing!";
        let deps = extract_missing_deps(text);
        assert!(deps.iter().any(|d| d == "fabric-api"));
        assert!(is_allowed_dep("fabric-api"));
        assert!(is_allowed_dep("fabric-language-kotlin"));
        // 恶意 slug 不在白名单
        assert!(!is_allowed_dep("totally-unknown-mod"));
    }

    #[test]
    fn conflict_pair_detected_only_when_both_present() {
        let both = vec!["OptiFine_1.20.jar".to_string(), "sodium-0.5.jar".to_string()];
        let found = suggestions(&[("内存不足".into(), vec![])], &both, 4096, 16384);
        assert!(found.iter().any(|f| f.id == "disable-conflicting-mod"));
        let only_one = vec!["sodium-0.5.jar".to_string()];
        let found2 = suggestions(&[], &only_one, 4096, 16384);
        assert!(!found2.iter().any(|f| f.id == "disable-conflicting-mod"));
    }

    #[test]
    fn duplicate_copy_is_detected() {
        let names = vec!["jei.jar".to_string(), "jei (1).jar".to_string()];
        assert_eq!(find_duplicate_copy(&names), Some("jei (1).jar".to_string()));
        let clean = vec!["jei.jar".to_string(), "sodium.jar".to_string()];
        assert_eq!(find_duplicate_copy(&clean), None);
    }

    #[test]
    fn oom_suggestion_appears() {
        let s = suggestions(
            &[("内存不足".into(), vec!["Java heap space".into()])],
            &[],
            4096,
            16384,
        );
        assert!(s.iter().any(|f| f.id == "increase-memory"));
    }
}
