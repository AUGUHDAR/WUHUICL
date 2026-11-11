//! 跨启动器实例导入（PCL2 / HMCL / Prism·MultiMC·PolyMC）。
//!
//! 设计取舍（与 scope `migration.cjs` 的差异）：
//! - scope 只做目录白名单直拷、**不解析任何源启动器配置**，迁移后内存/Java/图标全丢。
//!   这里把「读源元数据」当成核心的一部分——它才是导入体验的差距所在。
//! - scope 用随机 UUID 当目标目录名（永不重名也永不提示）。这里复用我们自己的
//!   `unique_instance_id` 语义（`-2`/`-3` 后缀），保持实例名可读。
//! - 先做 PCL/HMCL（标准 mcbbs 布局，我们已全面对齐其目录约定）+ Prism 家族，
//!   Modrinth App / CurseForge / ATLauncher 走「按包重装」而非目录拷贝，不在本模块。

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// 支持的来源启动器
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LauncherKind {
    Pcl,
    Hmcl,
    Prism,
}

impl LauncherKind {
    pub fn label(self) -> &'static str {
        match self {
            LauncherKind::Pcl => "PCL2",
            LauncherKind::Hmcl => "HMCL",
            LauncherKind::Prism => "Prism/MultiMC",
        }
    }
    pub fn id(self) -> &'static str {
        match self {
            LauncherKind::Pcl => "pcl",
            LauncherKind::Hmcl => "hmcl",
            LauncherKind::Prism => "prism",
        }
    }
}

/// 一个可导入的候选实例
#[derive(Debug, Clone)]
pub struct Candidate {
    pub launcher: LauncherKind,
    /// 实例目录的绝对路径（PCL/HMCL 为 versions/<id>；Prism 为 instances/<name>）
    pub path: PathBuf,
    /// 实例名（目录名，可被源配置覆盖）
    pub name: String,
    pub mc_version: String,
    pub loader: Option<String>,
    pub loader_version: Option<String>,
    /// 是否版本隔离（源启动器语义）
    pub isolated: bool,
    /// 源图标绝对路径（可空）
    pub icon: Option<PathBuf>,
    pub note: Option<String>,
}

fn has_versions_dir(root: &Path) -> bool {
    root.join("versions").is_dir()
}

/// 探测本机各启动器的根目录（Windows 优先，同时兼容用户自定义安装位置）。
pub fn detect_roots() -> Vec<(LauncherKind, PathBuf)> {
    let mut out: Vec<(LauncherKind, PathBuf)> = Vec::new();
    let mut push = |kind: LauncherKind, p: PathBuf| {
        if p.is_dir() && !out.iter().any(|(k, e)| *k == kind && e == &p) {
            out.push((kind, p));
        }
    };
    // ---- PCL2 / HMCL：它们都以 .minecraft 为游戏根，靠 versions/ 下的 PCL/ 或 .hmcl 痕迹区分
    if let Some(appdata) = std::env::var_os("APPDATA") {
        let roaming = PathBuf::from(&appdata);
        // 常见位置：官方 .minecraft（多数 PCL/HMCL 用户会直接指向它）
        for base in [
            roaming.join(".minecraft"),
            PathBuf::from(std::env::var_os("USERPROFILE").unwrap_or_default()).join(".minecraft"),
        ] {
            if let Some(kind) = classify_mcbbs_root(&base) {
                push(kind, base);
            }
        }
        // PCL 自己的数据目录
        let pcl_data = roaming.join("Plain Craft Launcher 2");
        for sub in ["Minecraft", ".minecraft"] {
            let p = pcl_data.join(sub);
            if let Some(kind) = classify_mcbbs_root(&p) {
                push(kind, p);
            }
        }
        // HMCL 数据目录
        for sub in ["HMCL/.minecraft", ".hmcl/.minecraft"] {
            let p = roaming.join(sub);
            if let Some(kind) = classify_mcbbs_root(&p) {
                push(kind, p);
            }
        }
    }
    // ---- Prism / MultiMC / PolyMC：固定结构 <root>/instances
    if let Some(appdata) = std::env::var_os("APPDATA") {
        let roaming = PathBuf::from(&appdata);
        for (name, kind) in [
            ("PrismLauncher", LauncherKind::Prism),
            ("MultiMC", LauncherKind::Prism),
            ("PolyMC", LauncherKind::Prism),
        ] {
            let p = roaming.join(name).join("instances");
            push(kind, p);
        }
    }
    out
}

/// 判定一个 .minecraft 根属于 PCL 还是 HMCL（看 versions/ 里的痕迹），识别不出返回 None。
fn classify_mcbbs_root(root: &Path) -> Option<LauncherKind> {
    if !has_versions_dir(root) {
        return None;
    }
    let versions = root.join("versions");
    let mut saw_pcl = false;
    let mut saw_hmcl = false;
    let rd = std::fs::read_dir(&versions).ok()?;
    for e in rd.flatten().take(200) {
        let p = e.path();
        if !p.is_dir() {
            continue;
        }
        if p.join("PCL").is_dir() {
            saw_pcl = true;
        }
        if p.join(".hmcl").is_dir() || root.join(".hmcl").is_dir() {
            saw_hmcl = true;
        }
    }
    // PCL 痕迹优先（PCL 也会生成 .hmcl 兼容目录的情况较少）
    if saw_pcl {
        Some(LauncherKind::Pcl)
    } else if saw_hmcl {
        Some(LauncherKind::Hmcl)
    } else if root.join("PCL").is_dir() {
        Some(LauncherKind::Pcl)
    } else {
        // 有 versions/ 但无明显痕迹：按 HMCL 处理（它是最通用的 mcbbs 布局）
        Some(LauncherKind::Hmcl)
    }
}

/// 扫描某个根目录下的全部可导入实例。
pub fn scan_root(kind: LauncherKind, root: &Path) -> Vec<Candidate> {
    match kind {
        LauncherKind::Pcl | LauncherKind::Hmcl => scan_mcbbs(root, kind),
        LauncherKind::Prism => scan_prism(root),
    }
}

/// PCL / HMCL：versions/<id>/ 每个子目录是一个实例
fn scan_mcbbs(root: &Path, kind: LauncherKind) -> Vec<Candidate> {
    let versions = root.join("versions");
    let Ok(rd) = std::fs::read_dir(&versions) else {
        return Vec::new();
    };
    let mut out: Vec<Candidate> = Vec::new();
    for e in rd.flatten() {
        let dir = e.path();
        if !dir.is_dir() {
            continue;
        }
        let dir_name = e.file_name().to_string_lossy().to_string();
        let json_path = dir.join(format!("{dir_name}.json"));
        if !json_path.is_file() {
            continue;
        }
        let Ok(raw) = std::fs::read_to_string(&json_path) else {
            continue;
        };
        let Ok(j) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        // 复用统一元数据解析（现在也支持 patches 老格式）
        let meta = crate::installer::resolve_instance_metadata(&j, &|id| {
            std::fs::read_to_string(versions.join(id).join(format!("{id}.json")))
                .ok()
                .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        });
        // 读源启动器的实例级配置
        let (name, isolated, note, icon) = match kind {
            LauncherKind::Pcl => read_pcl_settings(&dir, &dir_name),
            _ => read_hmcl_settings(root, &dir_name),
        };
        out.push(Candidate {
            launcher: kind,
            path: dir,
            name,
            mc_version: meta.mc_version,
            loader: meta.loader.map(str::to_string),
            loader_version: meta.loader_version,
            isolated,
            icon,
            note,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// PCL 实例设置：`versions/<id>/PCL/Setup.ini`（PCL 的每实例设置都落这里）
fn read_pcl_settings(dir: &Path, fallback_name: &str) -> (String, bool, Option<String>, Option<PathBuf>) {
    let ini = dir.join("PCL").join("Setup.ini");
    let mut name = fallback_name.to_string();
    let mut isolated = true; // PCL 默认开启版本隔离
    let mut note: Option<String> = None;
    if let Ok(text) = std::fs::read_to_string(&ini) {
        for line in text.lines() {
            let line = line.trim();
            let Some((key, val)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let val = val.trim();
            match key {
                // PCL 的隔离开关（V2 口径）：true=隔离
                "VersionArgumentIndieV2" => isolated = val.eq_ignore_ascii_case("true"),
                "CustomInfo" => {
                    if !val.is_empty() {
                        note = Some(val.to_string())
                    }
                }
                _ => {}
            }
        }
    }
    // 图标：PCL 常用 versions/<id>/PCL/Logo.png
    let mut icon: Option<PathBuf> = None;
    for candidate in [
        dir.join("PCL").join("Logo.png"),
        dir.join("icon.png"),
    ] {
        if candidate.is_file() {
            icon = Some(candidate);
            break;
        }
    }
    if name.is_empty() {
        name = fallback_name.to_string();
    }
    (name, isolated, note, icon)
}

/// HMCL 实例设置：`.hmcl/config/instance-game-settings.json`（全局一份，按实例 id 索引）
fn read_hmcl_settings(root: &Path, id: &str) -> (String, bool, Option<String>, Option<PathBuf>) {
    let mut isolated = true; // HMCL 默认隔离
    let cfg = root.join(".hmcl").join("config").join("instance-game-settings.json");
    if let Ok(raw) = std::fs::read_to_string(&cfg) {
        if let Ok(j) = serde_json::from_str::<Value>(&raw) {
            if let Some(entry) = j.get(id) {
                // runningDirectory 存在且不是 ".minecraft" 视为隔离
                if let Some(rd) = entry.get("runningDirectory").and_then(|v| v.as_str()) {
                    isolated = !rd.trim().is_empty() && rd.trim() != ".minecraft";
                }
            }
        }
    }
    // 图标：HMCL 支持多后缀（png/jpg/webp/apng...）
    let dir = root.join("versions").join(id);
    let mut icon: Option<PathBuf> = None;
    for ext in ["png", "jpg", "jpeg", "webp", "bmp", "gif", "apng"] {
        let p = dir.join(format!("icon.{ext}"));
        if p.is_file() {
            icon = Some(p);
            break;
        }
    }
    (id.to_string(), isolated, None, icon)
}

/// Prism / MultiMC：instances/<name>/ 内含 instance.cfg，游戏内容在 minecraft/（或 .minecraft/）
fn scan_prism(instances: &Path) -> Vec<Candidate> {
    let Ok(rd) = std::fs::read_dir(instances) else {
        return Vec::new();
    };
    let mut out: Vec<Candidate> = Vec::new();
    for e in rd.flatten() {
        let dir = e.path();
        if !dir.is_dir() {
            continue;
        }
        let cfg = dir.join("instance.cfg");
        if !cfg.is_file() {
            continue;
        }
        let mut name = e.file_name().to_string_lossy().to_string();
        let mut note: Option<String> = None;
        let mut icon: Option<PathBuf> = None;
        if let Ok(text) = std::fs::read_to_string(&cfg) {
            for line in text.lines() {
                let Some((k, v)) = line.split_once('=') else { continue };
                match k.trim() {
                    "name" if !v.trim().is_empty() => name = v.trim().to_string(),
                    "notes" if !v.trim().is_empty() => note = Some(v.trim().to_string()),
                    "iconKey" if !v.trim().is_empty() => {
                        let p = dir.join(v.trim());
                        if p.is_file() {
                            icon = Some(p);
                        }
                    }
                    _ => {}
                }
            }
        }
        // 组件版本在 mmc-pack.json
        let (mc_version, loader, loader_version) = read_mmc_pack(&dir.join("mmc-pack.json"));
        // 游戏根：minecraft/ 或 .minecraft/
        let game_root = if dir.join("minecraft").is_dir() {
            dir.join("minecraft")
        } else {
            dir.join(".minecraft")
        };
        out.push(Candidate {
            launcher: LauncherKind::Prism,
            path: game_root,
            name,
            mc_version,
            loader,
            loader_version,
            // Prism 的实例目录天然独立，视为隔离
            isolated: true,
            icon,
            note,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// 读 Prism 的 `mmc-pack.json` 组件表 → (MC 版本, 加载器, 加载器版本)
fn read_mmc_pack(path: &Path) -> (String, Option<String>, Option<String>) {
    let mut mc = "未知".to_string();
    let mut loader: Option<String> = None;
    let mut loader_version: Option<String> = None;
    let Ok(raw) = std::fs::read_to_string(path) else {
        return (mc, loader, loader_version);
    };
    let Ok(j) = serde_json::from_str::<Value>(&raw) else {
        return (mc, loader, loader_version);
    };
    let Some(components) = j.get("components").and_then(|c| c.as_array()) else {
        return (mc, loader, loader_version);
    };
    for c in components {
        let uid = c.get("uid").and_then(|v| v.as_str()).unwrap_or("");
        let ver = c.get("cachedVersion").and_then(|v| v.as_str()).unwrap_or("");
        match uid {
            "net.minecraft" => {
                if !ver.is_empty() {
                    mc = fuzzy_mc_version(ver);
                }
            }
            "net.fabricmc.fabric-loader" => {
                loader = Some("fabric".into());
                loader_version = Some(ver.to_string());
            }
            "org.quiltmc.quilt-loader" => {
                loader = Some("quilt".into());
                loader_version = Some(ver.to_string());
            }
            "net.minecraftforge" => {
                loader = Some("forge".into());
                // mmc 的 forge cachedVersion 形如 "1.20.1-47.2.0"，取后半段
                loader_version = Some(
                    ver.split_once('-').map(|(_, b)| b).unwrap_or(ver).to_string(),
                );
            }
            "net.neoforged" => {
                loader = Some("neoforge".into());
                loader_version = Some(
                    ver.split_once('-').map(|(_, b)| b).unwrap_or(ver).to_string(),
                );
            }
            _ => {}
        }
    }
    (mc, loader, loader_version)
}

/// Prism 的 net.minecraft cachedVersion 可能是 "1.20.1" 或带前缀的形式，取版本片段
fn fuzzy_mc_version(v: &str) -> String {
    let trimmed = v.trim();
    if crate::installer::is_minecraft_version_id_pub(trimmed) {
        return trimmed.to_string();
    }
    // 形如 "1.20.1_forge" / "1.20.1-47.2.0"：截到第一个分隔符。
    // 注意 `_` 与 `-` 都是分隔符（Prism 的 mmc-pack 会用下划线拼加载器名），
    // 不能算进版本串——否则 "1.12.2_forge" 会被原样当成版本。
    let cut: String = trimmed
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '.')
        .collect();
    let candidate = cut.trim_end_matches('.');
    if candidate.is_empty() {
        "未知".into()
    } else {
        candidate.to_string()
    }
}

// ---------------- IPC：扫描 / 导入 ----------------

/// 扫描本机所有可导入实例（`import:scan`）
pub fn scan_all() -> Value {
    let roots = detect_roots();
    let mut instances: Vec<Value> = Vec::new();
    let mut scanned_roots: Vec<Value> = Vec::new();
    for (kind, root) in roots {
        let cands = scan_root(kind, &root);
        scanned_roots.push(json!({
            "launcher": kind.label(),
            "id": kind.id(),
            "root": root.to_string_lossy(),
            "count": cands.len(),
        }));
        for c in cands {
            instances.push(json!({
                "launcher": c.launcher.label(),
                "launcherId": c.launcher.id(),
                "path": c.path.to_string_lossy(),
                "name": c.name,
                "mcVersion": c.mc_version,
                "loader": c.loader,
                "loaderVersion": c.loader_version,
                "isolated": c.isolated,
                "icon": c.icon.map(|p| p.to_string_lossy().to_string()),
                "note": c.note,
            }));
        }
    }
    json!({ "roots": scanned_roots, "instances": instances })
}

/// 确认导入：把一个候选实例复制到目标游戏文件夹（`import:apply`）。
///
/// 复制内容：源实例目录里的隔离内容（ISOLATION_DIRS）+ 版本 json/jar，
/// 不复制 PCL/HMCL/Prism 自己的配置文件（我们有自己的 schema）。
pub fn apply_import(
    app: &tauri::AppHandle,
    source_path: &str,
    target_folder: &str,
    requested_name: Option<&str>,
) -> Result<Value, String> {
    let src = Path::new(source_path);
    if !src.is_dir() {
        return Err("源实例目录不存在".into());
    }
    let versions_dir = Path::new(target_folder).join("versions");
    std::fs::create_dir_all(&versions_dir).map_err(|e| e.to_string())?;
    // 目标实例名：用户可改，缺省用源目录名
    let base = requested_name
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            src.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "imported".into())
        });
    // 冲突：与整合包导入同口径，`-2`/`-3` 递增
    let mut name = base.clone();
    let mut n = 2;
    while versions_dir.join(&name).exists() && n < 1000 {
        name = format!("{base}-{n}");
        n += 1;
    }
    let dest = versions_dir.join(&name);
    let rec = crate::tasks::register_task(&format!("导入实例 {name}"), "version");
    let result = (|| -> Result<Value, String> {
        crate::tasks::emit_progress(
            app,
            json!({ "taskId": rec.id, "overall": 0.05, "text": format!("正在导入 {name}…") }),
        );
        std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
        // 1) 版本 json：源目录里与目录同名的 json，或任意唯一 json
        let mut copied_files = 0usize;
        let mut version_json: Option<Value> = None;
        if let Ok(rd) = std::fs::read_dir(src) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_file() {
                    let ext = p.extension().map(|x| x.to_string_lossy().to_lowercase());
                    if matches!(ext.as_deref(), Some("jar") | Some("json")) {
                        let file_name = p.file_name().unwrap().to_string_lossy().to_string();
                        // json 重命名为 <实例名>.json；jar 同理
                        let target_name = if ext.as_deref() == Some("json") {
                            format!("{name}.json")
                        } else {
                            format!("{name}.jar")
                        };
                        let to = dest.join(&target_name);
                        if !to.exists() {
                            std::fs::copy(&p, &to).map_err(|e| e.to_string())?;
                            copied_files += 1;
                            if ext.as_deref() == Some("json") {
                                version_json = std::fs::read_to_string(&to)
                                    .ok()
                                    .and_then(|t| serde_json::from_str::<Value>(&t).ok());
                            }
                        }
                    }
                }
            }
        }
        // 2) 隔离内容目录（我们自己的 ISOLATION_DIRS 口径）
        const COPY_DIRS: &[&str] = &[
            "mods",
            "saves",
            "config",
            "resourcepacks",
            "shaderpacks",
            "screenshots",
        ];
        for d in COPY_DIRS {
            let from = src.join(d);
            if from.is_dir() {
                copy_dir_best_effort(&from, &dest.join(d), &mut copied_files)?;
            }
        }
        // 3) 文件类隔离内容
        for f in ["options.txt", "servers.dat"] {
            let from = src.join(f);
            if from.is_file() {
                let to = dest.join(f);
                if !to.exists() {
                    std::fs::copy(&from, &to).map_err(|e| e.to_string())?;
                    copied_files += 1;
                }
            }
        }
        // 4) 改写实例 json：id 对齐新名、写 _mcVersion/_loader、标明隔离
        let json_path = dest.join(format!("{name}.json"));
        if let Some(mut j) = version_json {
            if let Some(obj) = j.as_object_mut() {
                obj.insert("id".into(), json!(name));
                obj.insert("_gameDir".into(), json!(true));
            }
            std::fs::write(
                &json_path,
                serde_json::to_string_pretty(&j).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
        }
        crate::installer::register_version_folder(&name, target_folder);
        let _ = crate::installer::set_new_instance_isolation(target_folder, &name, true);
        let _ = crate::installer::flatten_instance_public(&name, target_folder);
        crate::tasks::emit_progress(
            app,
            json!({ "taskId": rec.id, "overall": 1.0, "text": format!("{name} 导入完成") }),
        );
        Ok(json!({ "id": name, "files": copied_files }))
    })();
    crate::tasks::task_done(app, &rec.id, result.is_ok(), None, false, None);
    result
}

/// 递归复制目录；已存在的文件跳过（保留目标侧内容，避免覆盖用户已有存档）
fn copy_dir_best_effort(
    from: &Path,
    to: &Path,
    copied: &mut usize,
) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
    let rd = std::fs::read_dir(from).map_err(|e| e.to_string())?;
    for e in rd.flatten() {
        let p = e.path();
        let ft = e.file_type().map_err(|x| x.to_string())?;
        let target = to.join(e.file_name());
        if ft.is_dir() {
            copy_dir_best_effort(&p, &target, copied)?;
        } else if ft.is_file() && !target.exists() {
            // 单文件失败不中断整体导入（个别文件被占用/无权限时其余照常搬）
            if std::fs::copy(&p, &target).is_ok() {
                *copied += 1;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mc_version_fuzzy_handles_forge_suffix() {
        assert_eq!(fuzzy_mc_version("1.20.1"), "1.20.1");
        assert_eq!(fuzzy_mc_version("1.20.1-47.2.0"), "1.20.1");
        assert_eq!(fuzzy_mc_version("1.12.2_forge"), "1.12.2");
    }

    #[test]
    fn mmc_pack_components_map_loaders() {
        let tmp = std::env::temp_dir().join(format!("wuhuicl-mmc-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let p = tmp.join("mmc-pack.json");
        std::fs::write(
            &p,
            r#"{"components":[
                {"uid":"net.minecraft","cachedVersion":"1.20.1"},
                {"uid":"net.fabricmc.fabric-loader","cachedVersion":"0.15.11"}
            ]}"#,
        )
        .unwrap();
        let (mc, loader, lv) = read_mmc_pack(&p);
        assert_eq!(mc, "1.20.1");
        assert_eq!(loader.as_deref(), Some("fabric"));
        assert_eq!(lv.as_deref(), Some("0.15.11"));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
