//! 整合包导出（Modrinth `.mrpack` / CurseForge `.zip`）。
//!
//! 这是「分享闭环」的另一半：能把本机实例打包成别人启动器可导入的格式。
//!
//! 关键设计（对应 Prism `FlamePackExportTask` / `ModrinthPackExportTask`）：
//! - **能被社区识别的 mod 走清单**（files 里记下载地址 + 哈希），目标启动器会自己重下；
//! - **识别不出的 mod 必须落进 overrides**，否则对方导入后少文件、起不来。
//!   这一步最容易漏——Prism 的 `getProjectsInfo` 之后就靠这个区分。
//! - 只导出 `enabled` 的 mod；`.disabled` 的不进包（对方不需要「被我禁用的」这层信息）。
//!
//! 与导入侧的对称性：导出的 mrpack 用我们自己 `parse_mrpack` 能原样读回。

use serde_json::{json, Value};
use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};

/// 一个 mod 文件的导出归宿
#[derive(Debug, Clone)]
pub struct ExportFile {
    /// 相对游戏目录的路径（`mods/xxx.jar`），写进清单与 overrides
    pub rel: String,
    pub abs: PathBuf,
    pub size: u64,
    pub sha1: String,
    pub sha512: String,
    /// 已知的下载地址（识别不出时为空 → 进 overrides）
    pub urls: Vec<String>,
}

/// 导出前的扫描结果
#[derive(Debug)]
pub struct ExportScan {
    pub mc_version: String,
    pub loader: Option<String>,
    pub loader_version: Option<String>,
    /// 进清单的文件（社区可识别）
    pub listed: Vec<ExportFile>,
    /// 进 overrides 的文件（识别不出 / 非 mods 内容）
    pub overridden: Vec<ExportFile>,
}

/// 需要收进 overrides 的配置类目录（不共享的玩家数据）
const OVERRIDE_DIRS: &[&str] = &["config", "defaultconfigs", "kubejs", "scripts"];
/// 需要收进 overrides 的配置类文件
const OVERRIDE_FILES: &[&str] = &["options.txt", "servers.dat"];

fn sha512_hex(path: &Path) -> Result<String, String> {
    use sha2::Digest;
    let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut h = sha2::Sha512::new();
    std::io::copy(&mut f, &mut h).map_err(|e| e.to_string())?;
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// 扫一个实例，算出导出计划。
///
/// `identity_of(sha1) -> Option<urls>` 用来查「这个文件是不是能下回来的社区资源」；
/// 返回 None 就进 overrides。这样把「识别能力」留给调用方（复用 mods/community 的缓存），
/// 本模块只负责格式组装。
pub fn plan_export<F>(
    game_dir: &Path,
    mc_version: &str,
    loader: Option<&str>,
    loader_version: Option<&str>,
    mut identity_of: F,
) -> Result<ExportScan, String>
where
    F: FnMut(&str) -> Option<Vec<String>>,
{
    let mut listed: Vec<ExportFile> = Vec::new();
    let mut overridden: Vec<ExportFile> = Vec::new();
    let mods = game_dir.join("mods");
    if let Ok(rd) = std::fs::read_dir(&mods) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let lower = name.to_lowercase();
            // 只导出启用中的 .jar；.disabled 与半成品后缀跳过
            if !lower.ends_with(".jar") {
                continue;
            }
            let abs = e.path();
            let Ok(meta) = e.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            let size = meta.len();
            let sha1 = crate::installer::sha1_file_hex(&abs)?;
            let sha512 = sha512_hex(&abs)?;
            let rel = format!("mods/{name}");
            let urls = identity_of(&sha1).unwrap_or_default();
            let file = ExportFile {
                rel,
                abs,
                size,
                sha1,
                sha512,
                urls,
            };
            // 有可信下载地址才进清单，否则只能塞进 overrides 保证对方不缺文件
            if file.urls.is_empty() {
                overridden.push(file);
            } else {
                listed.push(file);
            }
        }
    }
    // 配置类目录整体进 overrides（对方需要这些才能跑起来）
    for dir in OVERRIDE_DIRS {
        collect_dir_for_overrides(game_dir, dir, &mut overridden)?;
    }
    for f in OVERRIDE_FILES {
        let abs = game_dir.join(f);
        if abs.is_file() {
            let rel = f.to_string();
            let size = std::fs::metadata(&abs).map(|m| m.len()).unwrap_or(0);
            overridden.push(ExportFile {
                rel,
                abs,
                size,
                sha1: String::new(),
                sha512: String::new(),
                urls: Vec::new(),
            });
        }
    }
    Ok(ExportScan {
        mc_version: mc_version.to_string(),
        loader: loader.map(str::to_string),
        loader_version: loader_version.map(str::to_string),
        listed,
        overridden,
    })
}

/// 递归收集目录下所有文件，作为 overrides 内容（不读哈希，省时间）
fn collect_dir_for_overrides(
    game_dir: &Path,
    dir: &str,
    out: &mut Vec<ExportFile>,
) -> Result<(), String> {
    fn walk(root: &Path, cur: &Path, prefix: &str, out: &mut Vec<ExportFile>) -> Result<(), String> {
        let Ok(rd) = std::fs::read_dir(cur) else {
            return Ok(());
        };
        for e in rd.flatten() {
            let p = e.path();
            let Ok(ft) = e.file_type() else { continue };
            let name = e.file_name().to_string_lossy().to_string();
            let rel = format!("{prefix}/{name}");
            if ft.is_dir() {
                walk(root, &p, &rel, out)?;
            } else if ft.is_file() {
                let size = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
                out.push(ExportFile {
                    rel,
                    abs: p.clone(),
                    size,
                    sha1: String::new(),
                    sha512: String::new(),
                    urls: Vec::new(),
                });
            }
            let _ = root;
        }
        Ok(())
    }
    let start = game_dir.join(dir);
    if start.is_dir() {
        walk(game_dir, &start, dir, out)?;
    }
    Ok(())
}

/// 生成 `.mrpack` 到 `dest`。返回 (写入文件数, 清单文件数)
pub fn write_mrpack(
    scan: &ExportScan,
    name: &str,
    version: &str,
    summary: Option<&str>,
    dest: &Path,
) -> Result<Value, String> {
    let file = std::fs::File::create(dest).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    // 1) modrinth.index.json
    let mut deps = json!({ "minecraft": scan.mc_version });
    if let (Some(loader), Some(lv)) = (scan.loader.as_deref(), scan.loader_version.as_deref()) {
        if let Some(key) = mr_loader_key(loader) {
            deps[key] = json!(lv);
        }
    }
    let files: Vec<Value> = scan
        .listed
        .iter()
        .map(|f| {
            json!({
                "path": f.rel,
                "hashes": { "sha1": f.sha1, "sha512": f.sha512 },
                "downloads": f.urls,
                "fileSize": f.size,
                "env": { "client": "required", "server": "optional" }
            })
        })
        .collect();
    let index = json!({
        "formatVersion": 1,
        "game": "minecraft",
        "versionId": version,
        "name": name,
        "summary": summary.unwrap_or(""),
        "files": files,
        "dependencies": deps,
    });
    let text = serde_json::to_string_pretty(&index).map_err(|e| e.to_string())?;
    zip.start_file("modrinth.index.json", opts).map_err(|e| e.to_string())?;
    zip.write_all(text.as_bytes()).map_err(|e| e.to_string())?;

    // 2) overrides/：识别不出的文件必须收进来
    let mut written = 0usize;
    // 目录骨架：先建目录再放文件，zip 里目录条目不是必须的，但加上更贴近标准包
    let mut seen_dirs: HashSet<String> = HashSet::new();
    for f in &scan.overridden {
        let inner = format!("overrides/{}", f.rel.replace('\\', "/"));
        // 目录
        if let Some(pos) = inner.rfind('/') {
            let dir = inner[..pos].to_string();
            if seen_dirs.insert(dir.clone()) {
                let _ = zip.add_directory(dir, opts);
            }
        }
        zip.start_file(&inner, opts).map_err(|e| e.to_string())?;
        let mut src = std::fs::File::open(&f.abs).map_err(|e| e.to_string())?;
        std::io::copy(&mut src, &mut zip).map_err(|e| e.to_string())?;
        written += 1;
    }
    zip.finish().map_err(|e| e.to_string())?;
    Ok(json!({ "listed": scan.listed.len(), "overrides": written, "path": dest.to_string_lossy() }))
}

/// 生成 CurseForge `.zip`（manifest.json + overrides/）。
///
/// 注意：CF 格式的 `files` 只接受 **CurseForge 项目 id + 文件 id**，没有 URL 概念。
/// 所以只有「已知 CF 身份」的文件能进清单；其余一律进 overrides（比 mrpack 更严格）。
/// `cf_identity_of` 接收 `ExportFile`（含 sha1 与本地路径，便于算 murmur2 指纹）。
pub fn write_curseforge(
    scan: &ExportScan,
    name: &str,
    version: &str,
    cf_identity_of: impl Fn(&ExportFile) -> Option<(u64, u64)>,
    dest: &Path,
) -> Result<Value, String> {
    let file = std::fs::File::create(dest).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    let mut cf_files: Vec<Value> = Vec::new();
    let mut force_override: Vec<&ExportFile> = Vec::new();
    for f in &scan.listed {
        match cf_identity_of(f) {
            Some((project_id, file_id)) => cf_files.push(json!({
                "projectID": project_id,
                "fileID": file_id,
                "required": true
            })),
            // mrpack 有 modrinth 地址但 CF 不认识：只能进 overrides
            None => force_override.push(f),
        }
    }
    let mut mod_loaders = Vec::new();
    if let (Some(loader), Some(lv)) = (scan.loader.as_deref(), scan.loader_version.as_deref()) {
        let id = match loader {
            "forge" => format!("forge-{lv}"),
            "neoforge" => format!("neoforege-{lv}"),
            "fabric" => format!("fabric-{lv}"),
            "quilt" => format!("quilt-{lv}"),
            _ => String::new(),
        };
        if !id.is_empty() {
            mod_loaders.push(json!({ "id": id, "primary": true }));
        }
    }
    let manifest = json!({
        "minecraft": {
            "version": scan.mc_version,
            "modLoaders": mod_loaders,
        },
        "manifestType": "minecraftModpack",
        "manifestVersion": 1,
        "name": name,
        "version": version,
        "author": "WUHUICL",
        "files": cf_files,
        "overrides": "overrides",
    });
    zip.start_file("manifest.json", opts).map_err(|e| e.to_string())?;
    zip.write_all(
        serde_json::to_string_pretty(&manifest)
            .map_err(|e| e.to_string())?
            .as_bytes(),
    )
    .map_err(|e| e.to_string())?;

    let mut written = 0usize;
    let mut seen_dirs: HashSet<String> = HashSet::new();
    for f in scan.overridden.iter().chain(force_override.into_iter()) {
        let inner = format!("overrides/{}", f.rel.replace('\\', "/"));
        if let Some(pos) = inner.rfind('/') {
            let dir = inner[..pos].to_string();
            if seen_dirs.insert(dir.clone()) {
                let _ = zip.add_directory(dir, opts);
            }
        }
        zip.start_file(&inner, opts).map_err(|e| e.to_string())?;
        let mut src = std::fs::File::open(&f.abs).map_err(|e| e.to_string())?;
        std::io::copy(&mut src, &mut zip).map_err(|e| e.to_string())?;
        written += 1;
    }
    zip.finish().map_err(|e| e.to_string())?;
    Ok(json!({ "listed": cf_files.len(), "overrides": written, "path": dest.to_string_lossy() }))
}

/// mrpack 的 dependencies 键名（与导入侧 `MR_LOADERS` 对称）
fn mr_loader_key(loader: &str) -> Option<&'static str> {
    match loader {
        "fabric" => Some("fabric-loader"),
        "quilt" => Some("quilt-loader"),
        "forge" => Some("forge"),
        "neoforge" => Some("neoforge"),
        _ => None,
    }
}

// ---------------- IPC ----------------

/// `pack:exportMrpack`：导出 Modrinth 整合包。
///
/// args = [versionId, folder, options]；options: { name?, version?, summary?, destPath? }
pub fn export_mrpack(
    app: &tauri::AppHandle,
    version_id: &str,
    folder: &str,
    opts: &Value,
) -> Result<Value, String> {
    let scan = prepare_scan(app, version_id, folder)?;
    let name = opts
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(version_id)
        .to_string();
    let version = opts
        .get("version")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("1.0.0")
        .to_string();
    let summary = opts.get("summary").and_then(|v| v.as_str());
    let dest = match opts.get("destPath").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
        Some(p) => PathBuf::from(p),
        None => default_export_path(app, &name, "mrpack")?,
    };
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    write_mrpack(&scan, &name, &version, summary, &dest)
}

/// `pack:exportCurseforge`：导出 CurseForge 整合包。
pub fn export_curseforge(
    app: &tauri::AppHandle,
    version_id: &str,
    folder: &str,
    opts: &Value,
) -> Result<Value, String> {
    let scan = prepare_scan(app, version_id, folder)?;
    let name = opts
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(version_id)
        .to_string();
    let version = opts
        .get("version")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("1.0.0")
        .to_string();
    let dest = match opts.get("destPath").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
        Some(p) => PathBuf::from(p),
        None => default_export_path(app, &name, "zip")?,
    };
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    // CF 身份：读文件算 murmur2 指纹 → /fingerprints 反查 project + fileID，
    // 并要求返回项的 sha1 与本地文件一致（指纹会碰撞，单靠指纹不够）。
    let identity_lookup = |f: &ExportFile| -> Option<(u64, u64)> {
        let bytes = std::fs::read(&f.abs).ok()?;
        let fp = crate::mods::curse_fingerprint(&bytes) as u64;
        crate::community::cf_file_ids_by_fingerprint(app, fp, &f.sha1)
    };
    write_curseforge(&scan, &name, &version, identity_lookup, &dest)
}

/// 取实例的运行目录（隔离 → versions/<id>；非隔离 → 文件夹根）
fn game_dir(
    app: &tauri::AppHandle,
    version_id: &str,
    folder: &str,
) -> Result<PathBuf, String> {
    let inst = crate::installer::read_version_json(app, folder, version_id)?;
    let state = crate::installer::instance_directory_state(app, folder, version_id, &inst);
    let path = state["path"].as_str().unwrap_or(folder).to_string();
    Ok(PathBuf::from(path))
}

/// 组装导出计划：扫 mods + 查每个文件的社区下载地址
fn prepare_scan(
    app: &tauri::AppHandle,
    version_id: &str,
    folder: &str,
) -> Result<ExportScan, String> {
    let gdir = game_dir(app, version_id, folder)?;
    let inst = crate::installer::read_version_json(app, folder, version_id)?;
    let meta = crate::installer::resolve_instance_metadata(&inst, &|id| {
        std::fs::read_to_string(crate::installer::version_json_path_in(folder, id))
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
    });
    // 身份 → 下载地址。只在内存里缓存，避免同一个项目反复打接口
    let mods_dir = gdir.join("mods");
    let mut cache: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    let lookup = |sha1: &str| -> Option<Vec<String>> {
        if let Some(hit) = cache.get(sha1) {
            return Some(hit.clone());
        }
        let identity = crate::mods::mod_identity(app, &mods_dir, sha1);
        // 只支持 modrinth：CF 的直链受分发限制，且 mrpack 需要的是 HTTPS 直链
        let project = identity.strip_prefix("modrinth:")?;
        let files = crate::community::community_files(app, "modrinth", project, None).ok()?;
        // 按 sha1 精确匹配，而不是「取第一个版本」——版本号对不上会导出错误的 URL
        let urls: Vec<String> = files
            .iter()
            .filter(|f| {
                f.get("sha1")
                    .and_then(|v| v.as_str())
                    .map(|s| s.eq_ignore_ascii_case(sha1))
                    .unwrap_or(false)
            })
            .filter_map(|f| f.get("url").and_then(|v| v.as_str()).map(str::to_string))
            .filter(|u| u.starts_with("https://"))
            .collect();
        if urls.is_empty() {
            return None;
        }
        cache.insert(sha1.to_string(), urls.clone());
        Some(urls)
    };
    plan_export(
        &gdir,
        &meta.mc_version,
        meta.loader,
        meta.loader_version.as_deref(),
        lookup,
    )
}

/// 默认导出位置：<用户数据>/exports/<name>-<时间>.<ext>
fn default_export_path(
    app: &tauri::AppHandle,
    name: &str,
    ext: &str,
) -> Result<PathBuf, String> {
    let dir = crate::settings::user_data_dir(app).join("exports");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let safe: String = name
        .chars()
        .map(|c| if "\\/:*?\"<>|".contains(c) || (c as u32) < 0x20 { '-' } else { c })
        .collect();
    Ok(dir.join(format!(
        "{}-{}.{ext}",
        safe.trim(),
        chrono::Utc::now().format("%Y%m%d-%H%M%S")
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mr_loader_keys_match_import_side() {
        assert_eq!(mr_loader_key("fabric"), Some("fabric-loader"));
        assert_eq!(mr_loader_key("quilt"), Some("quilt-loader"));
        assert_eq!(mr_loader_key("forge"), Some("forge"));
        assert_eq!(mr_loader_key("neoforge"), Some("neoforge"));
        assert_eq!(mr_loader_key("unknown"), None);
    }

    /// 端到端：导出 mrpack → 用导入侧的 parse_mrpack 读回，字段必须对得上。
    /// 这条测试是「分享闭环」的真正保证，比单测格式字段更有意义。
    #[test]
    fn exported_mrpack_round_trips_through_importer() {
        let tmp = std::env::temp_dir().join(format!("wuhuicl-exp-{}", std::process::id()));
        let game = tmp.join("game");
        let mods = game.join("mods");
        let cfg = game.join("config");
        std::fs::create_dir_all(&mods).unwrap();
        std::fs::create_dir_all(&cfg).unwrap();
        // 一个「社区可识别」的 mod（查表给定 URL）与一个「识别不出」的 mod
        std::fs::write(mods.join("known.jar"), b"known-mod-bytes").unwrap();
        std::fs::write(mods.join("local-only.jar"), b"local-mod-bytes").unwrap();
        std::fs::write(mods.join("disabled.jar.disabled"), b"x").unwrap();
        std::fs::write(cfg.join("some.toml"), b"cfg").unwrap();

        let scan = plan_export(
            &game,
            "1.20.1",
            Some("fabric"),
            Some("0.15.11"),
            |sha1: &str| {
                // 只给 known.jar 一个 URL：用它的真实 sha1 判断
                let known = crate::installer::sha1_file_hex(&mods.join("known.jar")).unwrap();
                if sha1 == known {
                    Some(vec!["https://example.invalid/known.jar".to_string()])
                } else {
                    None
                }
            },
        )
        .unwrap();

        // known → 清单；local-only + disabled 不进清单
        assert_eq!(scan.listed.len(), 1);
        assert_eq!(scan.listed[0].rel, "mods/known.jar");
        // local-only.jar 必须进 overrides，否则对方导入后缺文件
        assert!(scan.overridden.iter().any(|f| f.rel == "mods/local-only.jar"));
        assert!(scan.overridden.iter().any(|f| f.rel == "config/some.toml"));
        // .disabled 不导出
        assert!(!scan.overridden.iter().any(|f| f.rel.contains("disabled")));

        let dest = tmp.join("out.mrpack");
        write_mrpack(&scan, "测试包", "1.0.0", Some("summary"), &dest).unwrap();

        // 用导入侧解析器读回
        let file = std::fs::File::open(&dest).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let parsed = crate::modpacks::parse_mrpack_for_test(&mut archive).unwrap();
        assert_eq!(parsed["mcVersion"].as_str(), Some("1.20.1"));
        assert_eq!(parsed["loader"].as_str(), Some("fabric"));
        assert_eq!(parsed["loaderVersion"].as_str(), Some("0.15.11"));
        assert_eq!(parsed["files"].as_array().map(|a| a.len()), Some(1));
        assert_eq!(parsed["files"][0]["rel"].as_str(), Some("mods/known.jar"));

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
