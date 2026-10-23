//! 整合包探测与安装（对应原版 core/modpacks.ts 1:1 的核心流程）。
//! modpack:probe / modpack:install / modpack:supplyFiles / modpack:openFile / import:probe。
//! 全量包（fullpack）分支、CurseForge 备用源/包内复用为后续增强；主线
//! mrpack（HTTPS 直链下载）与 CurseForge（API 解析 fileID → 下载 URL）完整实现。

use crate::installer::{self, DownloadDetail};
use crate::tasks::TaskRecord;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};

const PACK_MANIFEST: &str = ".wuhuicl-pack.json";

// ---------------- 通用工具 ----------------

fn sanitize_id(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if "\\/:*?\"<>|".contains(c) || (c as u32) < 0x20 { '-' } else { c })
        .collect();
    let trimmed = cleaned.trim().trim_end_matches('.');
    if trimmed.is_empty() { "modpack".to_string() } else { trimmed.to_string() }
}

fn unique_instance_id(base: &str, target_folder: &str) -> String {
    let versions_dir = Path::new(target_folder).join("versions");
    if !versions_dir.join(base).exists() {
        return base.to_string();
    }
    for n in 2..1000 {
        let candidate = format!("{base}-{n}");
        if !versions_dir.join(&candidate).exists() {
            return candidate;
        }
    }
    format!("{}-{}", base, crate::accounts::uuid_v4())
}

fn pack_file_name(file_path: &str) -> String {
    Path::new(file_path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "modpack".to_string())
}

/// 安全路径拼接（拒绝穿越），返回目标绝对路径
fn safe_join(base: &Path, rel: &str) -> Result<PathBuf, String> {
    let normalized = rel.replace('\\', "/");
    let mut out = base.to_path_buf();
    for part in normalized.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return Err(format!("整合包文件路径不安全：{rel}"));
        }
        out.push(part);
    }
    Ok(out)
}

// ---------------- 手动补充文件（modpackManualFiles.ts） ----------------

/// 下载不动的整合包文件：经 event:progress.manualFiles 请求玩家手动提供，
/// modpack:supplyFiles 弹多选框校验入位后安装线程继续。
#[derive(Clone)]
struct ManualFile {
    project_id: i64,
    file_id: i64,
    file_name: String,
    size: u64,
    sha1: String,
    dest: std::path::PathBuf,
}

struct ManualSession {
    files: Vec<ManualFile>,
    task_id: String,
    created: u64,
}

fn manual_sessions() -> &'static std::sync::Mutex<std::collections::HashMap<String, ManualSession>> {
    static S: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, ManualSession>>> =
        std::sync::OnceLock::new();
    S.get_or_init(|| {
        let mut m = std::collections::HashMap::new();
        let _ = &mut m;
        std::sync::Mutex::new(m)
    })
}

fn manual_files_json(files: &[ManualFile]) -> Value {
    Value::Array(
        files
            .iter()
            .map(|f| {
                json!({
                    "projectID": f.project_id, "fileID": f.file_id,
                    "fileName": f.file_name, "size": f.size, "sha1": f.sha1,
                })
            })
            .collect(),
    )
}

/// 推送 manualFiles 请求（request = None 表示清除补充提示）
fn emit_manual(app: &tauri::AppHandle, task_id: &str, token: Option<(&str, &[ManualFile])>) {
    let manual = token
        .map(|(t, files)| json!({ "token": t, "files": manual_files_json(files) }))
        .unwrap_or(Value::Null);
    crate::tasks::emit_progress(
        app,
        json!({
            "taskId": task_id, "taskTitle": "导入整合包", "stage": "modpack",
            "progress": 0.9, "text": "等待补充整合包文件…", "manualFiles": manual,
        }),
    );
}

/// install 线程侧：注册会话、推送请求并轮询等待补齐（取消即退出）。返回后必须清会话。
fn wait_manual_files(app: &tauri::AppHandle, rec: &crate::tasks::TaskRecord, files: Vec<ManualFile>) -> Result<String, String> {
    if files.is_empty() {
        return Ok(String::new());
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    {
        let mut s = manual_sessions().lock().unwrap();
        s.retain(|_, v| now.saturating_sub(v.created) < 30 * 60 * 1000);
    }
    let token = crate::accounts::uuid_v4();
    manual_sessions().lock().unwrap().insert(
        token.clone(),
        ManualSession { files, task_id: rec.id.clone(), created: now },
    );
    loop {
        if rec.cancelled.load(std::sync::atomic::Ordering::SeqCst) {
            manual_sessions().lock().unwrap().remove(&token);
            emit_manual(app, &rec.id, None);
            return Err("已取消".into());
        }
        let remaining = manual_sessions()
            .lock()
            .unwrap()
            .get(&token)
            .map(|s| s.files.len())
            .unwrap_or(0);
        if remaining == 0 {
            manual_sessions().lock().unwrap().remove(&token);
            emit_manual(app, &rec.id, None);
            return Ok(token);
        }
        // 每 2 秒重发一次请求，防止前端弹窗被后到进度事件挤掉
        let session = manual_sessions().lock().unwrap().get(&token).map(|s| (s.files.clone(), s.task_id.clone()));
        if let Some((files, task_id)) = session {
            emit_manual(app, &task_id, Some((&token, &files)));
        }
        std::thread::sleep(std::time::Duration::from_millis(2000));
    }
}

/// modpack:supplyFiles 主体：校验所选文件（大小 + SHA1）复制入位，返回 {accepted, remaining, rejected}
fn supply_manual_files(app: &tauri::AppHandle, token: &str, sources: &[std::path::PathBuf]) -> Result<Value, String> {
    let (mut accepted, mut rejected) = (0usize, Vec::<String>::new());
    {
        let mut sessions = manual_sessions().lock().unwrap();
        let Some(session) = sessions.get_mut(token) else {
            return Err("该补充文件任务已结束，请查看下载中心".into());
        };
        for src in sources {
            let base = src.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let Ok(meta) = std::fs::symlink_metadata(src) else {
                rejected.push(base);
                continue;
            };
            if meta.is_symlink() || !meta.is_file() {
                rejected.push(base);
                continue;
            }
            // 大小一致的候选里找 SHA1 匹配（与原版 matches() 同判定）
            let mut matched: Option<usize> = None;
            for (i, f) in session.files.iter().enumerate() {
                if f.size != meta.len() {
                    continue;
                }
                let expected = crate::net::Integrity {
                    sha1: Some(f.sha1.clone()),
                    sha512: None,
                    sha256: None,
                    size: Some(f.size),
                };
                if matches!(crate::net::verify_file(src, &expected, None), Ok(None)) {
                    matched = Some(i);
                    break;
                }
            }
            let Some(i) = matched else {
                rejected.push(base);
                continue;
            };
            let f = &session.files[i];
            if let Some(parent) = f.dest.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            if !f.dest.exists() {
                std::fs::copy(src, &f.dest).map_err(|e| e.to_string())?;
            }
            session.files.remove(i);
            accepted += 1;
        }
        let remaining = session.files.len();
        let task_id = session.task_id.clone();
        let files_now = session.files.clone();
        drop(sessions);
        emit_manual(
            app,
            &task_id,
            if remaining == 0 { None } else { Some((token, &files_now)) },
        );
        Ok(json!({ "accepted": accepted, "remaining": remaining, "rejected": rejected }))
    }
}

// ---------------- 格式检测与解析 ----------------

struct Parsed {
    format: &'static str,
    name: String,
    pack_version: String,
    mc_version: String,
    loader: Option<String>,
    loader_version: Option<String>,
    overrides_prefix: String,
    client_overrides_prefix: Option<String>,
    /// mrpack 直链文件：rel/url/sha1/sha512/size
    files: Vec<Value>,
    /// CurseForge 清单文件：projectID/fileID
    cf_files: Vec<(i64, i64)>,
}

fn read_zip_text(archive: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Result<String, String> {
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        if entry.name().trim_start_matches('/').replace('\\', "/") == name {
            let mut out = String::new();
            entry.read_to_string(&mut out).map_err(|e| e.to_string())?;
            return Ok(out);
        }
    }
    Err(format!("ZIP 中找不到 {name}"))
}

fn entry_names(archive: &mut zip::ZipArchive<std::fs::File>) -> Result<Vec<String>, String> {
    let mut names = Vec::with_capacity(archive.len());
    for i in 0..archive.len() {
        let entry = archive.by_index_raw(i).map_err(|e| e.to_string())?;
        names.push(entry.name().trim_start_matches('/').replace('\\', "/"));
    }
    Ok(names)
}

fn detect_format(archive: &mut zip::ZipArchive<std::fs::File>) -> Result<&'static str, String> {
    let names: HashSet<String> = entry_names(archive)?.into_iter().collect();
    if names.contains("modrinth.index.json") {
        return Ok("mrpack");
    }
    if names.contains("manifest.json") {
        return Ok("curseforge");
    }
    Err("无法识别的整合包格式（支持 Modrinth .mrpack、CurseForge .zip 与含 versions 目录的完整客户端包）".into())
}

const MR_LOADERS: [(&str, &str); 4] = [
    ("fabric-loader", "fabric"),
    ("quilt-loader", "quilt"),
    ("neoforge", "neoforge"),
    ("forge", "forge"),
];

fn parse_mrpack(archive: &mut zip::ZipArchive<std::fs::File>) -> Result<Parsed, String> {
    let text = read_zip_text(archive, "modrinth.index.json")
        .map_err(|_| "modrinth.index.json 已损坏，整合包无法解析".to_string())?;
    let idx: Value = serde_json::from_str(&text).map_err(|_| "modrinth.index.json 已损坏，整合包无法解析".to_string())?;
    if idx["formatVersion"].as_i64() != Some(1) {
        return Err(format!(
            "不支持的 Modrinth 整合包格式版本：{}（当前支持 1）",
            idx["formatVersion"].as_i64().map(|v| v.to_string()).unwrap_or_else(|| "缺失".into())
        ));
    }
    if idx["game"].as_str() != Some("minecraft") {
        return Err(format!(
            "该 Modrinth 包不是 Minecraft 整合包：{}",
            idx["game"].as_str().unwrap_or("game 字段缺失")
        ));
    }
    let mc_version = idx["dependencies"]["minecraft"]
        .as_str()
        .ok_or("整合包清单缺少 Minecraft 版本，文件可能损坏")?
        .to_string();
    let mut loader = None;
    let mut loader_version = None;
    for (key, l) in MR_LOADERS {
        if let Some(v) = idx["dependencies"][key].as_str() {
            loader = Some(l.to_string());
            loader_version = Some(v.to_string());
            break;
        }
    }
    let mut files = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for f in idx["files"].as_array().cloned().unwrap_or_default() {
        let rel_raw = f["path"].as_str().ok_or("modrinth.index.json 包含缺少 path 的文件项")?;
        if f["env"]["client"].as_str() == Some("unsupported") {
            continue;
        }
        let rel = rel_raw.replace('\\', "/").trim_start_matches("./").to_string();
        safe_join(Path::new(".mrpack-path-check"), &rel).map_err(|_| format!("Modrinth 文件路径不安全：{rel_raw}"))?;
        let identity = rel.to_lowercase();
        if !seen.insert(identity) {
            return Err(format!("Modrinth 清单包含重复目标路径：{rel}"));
        }
        let urls: Vec<String> = f["downloads"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|u| u.as_str().map(str::to_string))
            .filter(|u| u.starts_with("https://"))
            .collect();
        if urls.is_empty() {
            return Err(format!("Modrinth 文件没有可信 HTTPS 下载地址：{rel}"));
        }
        let sha1 = f["hashes"]["sha1"].as_str().map(|s| s.to_lowercase());
        let sha512 = f["hashes"]["sha512"].as_str().map(|s| s.to_lowercase());
        if sha1.is_none() && sha512.is_none() {
            return Err(format!("Modrinth 文件缺少 SHA1/SHA512：{rel}"));
        }
        if let Some(s) = &sha1 {
            if s.len() != 40 || !s.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(format!("Modrinth 文件 SHA1 无效：{rel}"));
            }
        }
        if let Some(s) = &sha512 {
            if s.len() != 128 || !s.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(format!("Modrinth 文件 SHA512 无效：{rel}"));
            }
        }
        files.push(json!({
            "rel": rel, "url": urls[0], "urls": urls[1..],
            "sha1": sha1, "sha512": sha512, "size": f["fileSize"].as_u64().unwrap_or(0)
        }));
    }
    Ok(Parsed {
        format: "mrpack",
        name: idx["name"].as_str().map(str::trim).filter(|s| !s.is_empty()).unwrap_or("未命名整合包").to_string(),
        pack_version: idx["versionId"].as_str().unwrap_or("").to_string(),
        mc_version,
        loader,
        loader_version,
        overrides_prefix: "overrides".into(),
        client_overrides_prefix: Some("client-overrides".into()),
        files,
        cf_files: Vec::new(),
    })
}

const CF_LOADER_IDS: [&str; 4] = ["fabric", "forge", "quilt", "neoforge"];

fn parse_curseforge(archive: &mut zip::ZipArchive<std::fs::File>) -> Result<Parsed, String> {
    let text =
        read_zip_text(archive, "manifest.json").map_err(|_| "manifest.json 已损坏，整合包无法解析".to_string())?;
    let mf: Value = serde_json::from_str(&text).map_err(|_| "manifest.json 已损坏，整合包无法解析".to_string())?;
    let mc_version = mf["minecraft"]["version"]
        .as_str()
        .ok_or("整合包清单缺少 Minecraft 版本，文件可能损坏")?
        .to_string();
    let mod_loaders = mf["minecraft"]["modLoaders"].as_array().cloned().unwrap_or_default();
    let primary = mod_loaders.iter().find(|l| l["primary"] == true).or_else(|| mod_loaders.first());
    let mut loader = None;
    let mut loader_version = None;
    if let Some(id) = primary.and_then(|l| l["id"].as_str()) {
        let Some((name, version)) = id.split_once('-') else {
            return Err(format!("暂不支持该整合包使用的加载器：{id}"));
        };
        if !CF_LOADER_IDS.contains(&name.to_lowercase().as_str()) {
            return Err(format!("暂不支持该整合包使用的加载器：{id}"));
        }
        loader = Some(name.to_lowercase());
        loader_version = Some(version.to_string());
    }
    let cf_files = mf["files"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|f| Some((f["projectID"].as_i64()?, f["fileID"].as_i64()?)))
        .collect();
    Ok(Parsed {
        format: "curseforge",
        name: mf["name"].as_str().map(str::trim).filter(|s| !s.is_empty()).unwrap_or("未命名整合包").to_string(),
        pack_version: mf["version"].as_str().unwrap_or("").to_string(),
        mc_version,
        loader,
        loader_version,
        overrides_prefix: mf["overrides"].as_str().unwrap_or("overrides").to_string(),
        client_overrides_prefix: None,
        files: Vec::new(),
        cf_files,
    })
}

fn open_zip(path: &str) -> Result<zip::ZipArchive<std::fs::File>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("整合包无法打开：{e}"))?;
    zip::ZipArchive::new(file).map_err(|e| format!("整合包无法打开：{e}"))
}

fn prefix_exists(archive: &mut zip::ZipArchive<std::fs::File>, prefix: &str) -> bool {
    let prefix = format!("{prefix}/");
    entry_names(archive)
        .map(|names| names.iter().any(|n| n.starts_with(&prefix)))
        .unwrap_or(false)
}

/// modpack:probe → ModpackInfo
pub fn probe_modpack(file_path: &str) -> Result<Value, String> {
    let mut archive = open_zip(file_path)?;
    let format = detect_format(&mut archive)?;
    let file_name = pack_file_name(file_path);
    let parsed = match format {
        "mrpack" => parse_mrpack(&mut archive)?,
        "curseforge" => parse_curseforge(&mut archive)?,
        _ => return Err("无法识别的整合包格式".into()),
    };
    let download_bytes = if parsed.format == "mrpack" {
        parsed.files.iter().filter_map(|f| f["size"].as_u64()).sum()
    } else {
        0
    };
    Ok(json!({
        "format": parsed.format,
        "innerName": parsed.name,
        "fileName": file_name,
        "version": parsed.pack_version,
        "mcVersion": parsed.mc_version,
        "loader": parsed.loader,
        "loaderVersion": parsed.loader_version,
        "fileCount": if parsed.format == "mrpack" { parsed.files.len() } else { parsed.cf_files.len() },
        "downloadBytes": download_bytes,
        "hasOverrides": prefix_exists(&mut archive, &parsed.overrides_prefix),
        "hasClientOverrides": parsed.client_overrides_prefix.as_ref().map(|p| prefix_exists(&mut archive, p)).unwrap_or(false),
        "existingInstances": []
    }))
}

/// import:probe → ImportProbeResult（modpack / world / unknown 三分支；world 走 worlds.rs）
pub fn probe_import(input_path: &str) -> Result<Value, String> {
    if !input_path.is_empty() && Path::new(input_path).extension().map(|e| e.to_string_lossy().to_lowercase()) == Some("zip".into()) {
        if let Ok(info) = crate::worlds::probe_world(input_path) {
            if !info.is_null() {
                return Ok(json!({ "kind": "world", "info": info }));
            }
        }
    }
    let ext = Path::new(input_path)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if ext == "mrpack" || ext == "zip" {
        if let Ok(info) = probe_modpack(input_path) {
            return Ok(json!({ "kind": "modpack", "info": info }));
        }
    }
    Ok(json!({ "kind": "unknown", "path": input_path }))
}

// ---------------- CurseForge 文件解析（下载 URL） ----------------

/// CurseForge fileID → 下载信息（复用 community.rs 的 CF 通道；失败报错文案一致）
fn resolve_cf_file(app: &tauri::AppHandle, project_id: i64, file_id: i64) -> Result<Value, String> {
    let result = crate::community::cf_fetch_public(app, &format!("/mods/{project_id}/files/{file_id}"))?;
    let data = result.get("data").cloned().ok_or("CurseForge 文件信息获取失败")?;
    let url = data["downloadUrl"]
        .as_str()
        .map(str::to_string)
        .or_else(|| {
            // CDN 兜底 URL（CurseForge 官方无 downloadUrl 时）
            let slug = data["modId"].as_i64().map(|m| m.to_string()).unwrap_or_default();
            let fname = data["fileName"].as_str().map(str::to_string).unwrap_or_default();
            (!fname.is_empty()).then(|| {
                let sid = data["modId"].as_i64().unwrap_or(0);
                format!("https://mediafilez.forgecdn.net/files/{}/{}/{}", sid / 1000, sid % 1000, fname)
            })
        })
        .ok_or("该 CurseForge 文件没有可用下载地址（作者禁止分发）")?;
    Ok(json!({ "url": url, "fileName": data["fileName"].as_str().unwrap_or(""), "sha1": data["hashes"].as_array().and_then(|hs| hs.iter().find(|h| h["algo"].as_i64() == Some(1)).and_then(|h| h["value"].as_str())).unwrap_or(""), "size": data["fileLength"].as_u64().unwrap_or(0) }))
}

// ---------------- 安装 ----------------

/// TaskRecord → Arc 克隆（download API 签名要求）
fn clone_rec(rec: &TaskRecord) -> TaskRecord {
    TaskRecord {
        id: rec.id.clone(),
        title: rec.title.clone(),
        kind: rec.kind,
        cancelled: rec.cancelled.clone(),
        paused: rec.paused.clone(),
        status: std::sync::Mutex::new(*rec.status.lock().unwrap()),
    }
}

fn extract_overrides(
    archive: &mut zip::ZipArchive<std::fs::File>,
    prefix: &str,
    inst_dir: &Path,
    rec: Option<&TaskRecord>,
    on_entry: &mut impl FnMut(usize, usize, &str),
) -> Result<Vec<String>, String> {
    let prefix_norm = format!("{}/", prefix.trim_matches('/'));
    let mut names: Vec<(usize, String)> = Vec::new();
    for i in 0..archive.len() {
        let entry = archive.by_index_raw(i).map_err(|e| e.to_string())?;
        let name = entry.name().trim_start_matches('/').replace('\\', "/");
        if name.starts_with(&prefix_norm) && !name.ends_with('/') {
            names.push((i, name));
        }
    }
    let mut written = Vec::new();
    for (count, (i, name)) in names.iter().enumerate() {
        if let Some(r) = rec {
            crate::tasks::wait_while_paused(r);
            if r.cancelled.load(std::sync::atomic::Ordering::SeqCst) {
                return Err("已取消".into());
            }
        }
        let rel = name[prefix_norm.len()..].to_string();
        let target = safe_join(inst_dir, &rel)?;
        if target.is_dir() {
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut entry = archive.by_index(*i).map_err(|e| e.to_string())?;
        let mut out = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut out).map_err(|e| e.to_string())?;
        std::fs::write(&target, out).map_err(|e| e.to_string())?;
        on_entry(count + 1, names.len(), &rel);
        written.push(rel);
    }
    Ok(written)
}

/// 安装整合包，返回实例版本 id（对应 installModpack / installModpackInFolder 主线）
pub fn install_modpack(
    app: &tauri::AppHandle,
    file_path: &str,
    opts: &Value,
    rec: &TaskRecord,
) -> Result<String, String> {
    install_modpack_with_sink(app, file_path, opts, rec, None)
}

/// `sink`：社区下载链路传入（stage/progress/text 转发进 ModpackInstallJob.emit_install，
/// 由社区侧做 overall 0.1+0.9x 映射）；本地导入传 None，行为与原先完全一致。
pub fn install_modpack_with_sink(
    app: &tauri::AppHandle,
    file_path: &str,
    opts: &Value,
    rec: &TaskRecord,
    sink: Option<&(dyn Fn(&str, f64, &str) + Send + Sync)>,
) -> Result<String, String> {
    let settings = crate::settings::get(app);
    let target_folder = opts["targetFolder"]
        .as_str()
        .map(str::to_string)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| settings["activeFolder"].as_str().unwrap_or_default().to_string());
    if target_folder.is_empty() {
        return Err("未配置游戏目录".into());
    }
    let started = std::time::Instant::now();
    let emit = |app: &tauri::AppHandle, task_id: &str, stage: &str, progress: f64, text: &str| {
        crate::tasks::emit_progress(app, json!({ "taskId": task_id, "taskTitle": "导入整合包", "stage": stage, "progress": progress, "text": text }));
        if let Some(sink) = sink {
            sink(stage, progress, text);
        }
    };

    let mut archive = open_zip(file_path)?;
    let name_source = if opts["nameSource"] == "inner" { "inner" } else { "file" };
    let file_name = pack_file_name(file_path);
    emit(app, &rec.id, "modpack", 0.0, "解析整合包信息…");
    let format = detect_format(&mut archive)?;
    let parsed = match format {
        "mrpack" => parse_mrpack(&mut archive)?,
        "curseforge" => parse_curseforge(&mut archive)?,
        _ => return Err("无法识别的整合包格式".into()),
    };
    crate::tasks::wait_while_paused(rec);
    if rec.cancelled.load(std::sync::atomic::Ordering::SeqCst) {
        return Err("已取消".into());
    }
    let loader_text = parsed
        .loader
        .as_ref()
        .map(|l| format!(" + {} {}", l, parsed.loader_version.clone().unwrap_or_default()))
        .unwrap_or_default();
    emit(app, &rec.id, "modpack", 0.02, &format!("{}（MC {}{}）", parsed.name, parsed.mc_version, loader_text));

    // 实例命名与冲突
    let requested = sanitize_id(
        opts["instanceName"].as_str().map(str::trim).filter(|s| !s.is_empty())
            .unwrap_or(if name_source == "inner" { &parsed.name } else { &file_name }),
    );
    let action = opts["conflictAction"].as_str().unwrap_or("new").to_string();
    let id: String;
    if action == "update" || action == "overwrite" {
        if opts["confirmReplace"] != true {
            return Err(format!("{}现有实例前必须在确认页明确确认", if action == "update" { "更新" } else { "覆盖" }));
        }
        let existing = opts["existingId"].as_str().unwrap_or("").trim().to_string();
        id = sanitize_id(&existing);
        if existing.is_empty() || id != existing {
            return Err("需要选择有效的现有实例".into());
        }
    } else if action == "rename" {
        id = requested.clone();
        if Path::new(&target_folder).join("versions").join(&id).exists() {
            return Err(format!("实例名称已存在：{id}"));
        }
    } else {
        id = unique_instance_id(&requested, &target_folder);
    }
    let inst_dir = Path::new(&target_folder).join("versions").join(&id);

    let result = (|| -> Result<String, String> {
        // 1) 解析下载清单
        let mut pending: Vec<(String, String, Vec<String>, String, Option<String>, u64)> = Vec::new(); // rel/url/urls/sha1/sha512/size
        if parsed.format == "mrpack" {
            for f in &parsed.files {
                let rel = f["rel"].as_str().unwrap_or_default().to_string();
                let dest = safe_join(&inst_dir, &rel).map_err(|_| format!("整合包文件路径不安全：{rel}"))?;
                let _ = dest;
                pending.push((
                    rel,
                    f["url"].as_str().unwrap_or_default().to_string(),
                    f["urls"].as_array().cloned().unwrap_or_default().iter().filter_map(|u| u.as_str().map(str::to_string)).collect(),
                    f["sha1"].as_str().unwrap_or_default().to_string(),
                    f["sha512"].as_str().map(str::to_string),
                    f["size"].as_u64().unwrap_or(0),
                ));
            }
        } else {
            for (project_id, file_id) in &parsed.cf_files {
                let info = resolve_cf_file(app, *project_id, *file_id)?;
                let rel = format!("mods/{}", info["fileName"].as_str().unwrap_or_default());
                safe_join(&inst_dir, &rel).map_err(|_| format!("整合包文件路径不安全：{rel}"))?;
                pending.push((
                    rel,
                    info["url"].as_str().unwrap_or_default().to_string(),
                    Vec::new(),
                    info["sha1"].as_str().unwrap_or_default().to_string(),
                    None,
                    info["size"].as_u64().unwrap_or(0),
                ));
            }
        }

        // 2) 基础运行时安装（安装器管线：版本 json/jar/libraries）
        emit(app, &rec.id, "modpack", 0.05, "准备游戏环境…");
        let installed_id = crate::installer::install_vanilla_public(
            app,
            Some(rec),
            &parsed.mc_version,
            &target_folder,
            Some(&id),
            parsed.loader.as_deref(),
            parsed.loader_version.as_deref(),
            |event: &Value| {
                let mut e = event.clone();
                e["overall"] = json!(e["overall"].as_f64().unwrap_or_else(|| e["progress"].as_f64().unwrap_or(0.0)) * 0.42);
                crate::tasks::emit_progress(app, e);
            },
        )?;
        if installed_id != id {
            return Err("整合包运行配置未安装到目标实例".into());
        }

        // 3) 下载整合包文件（下不动的转手动补充，不再整包失败）
        let total = pending.len();
        let mut done = 0usize;
        let total_bytes: u64 = pending.iter().map(|p| p.5).sum();
        let mut done_bytes: u64 = 0;
        let mut manual: Vec<ManualFile> = Vec::new();
        for (idx, (rel, url, urls, sha1, sha512, size)) in pending.iter().enumerate() {
            crate::tasks::wait_while_paused(rec);
            if rec.cancelled.load(std::sync::atomic::Ordering::SeqCst) {
                return Err("已取消".into());
            }
            let dest = safe_join(&inst_dir, rel).map_err(|e| e)?;
            let rel_clone = rel.clone();
            let _ = &rel_clone;
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            if dest.exists() {
                done += 1;
                continue;
            }
            let mut candidates: Vec<String> = vec![url.clone()];
            candidates.extend(urls.iter().cloned());
            let mut last_err = String::new();
            let mut ok = false;
            for (attempt, cand) in candidates.iter().enumerate() {
                if attempt > 0 {
                    last_err = String::new();
                }
                let mut opts = crate::download::DownloadFileOpts::default();
                if !sha1.is_empty() {
                    opts.sha1 = Some(sha1.as_str());
                }
                if let Some(s) = sha512 {
                    opts.sha512 = Some(s.as_str());
                }
                opts.size = if *size > 0 { Some(*size) } else { None };
                match crate::download::download_file(app, &std::sync::Arc::new(clone_rec(rec)), cand, &dest, &opts) {
                    Ok(()) => {
                        ok = true;
                        break;
                    }
                    Err(e) => last_err = e,
                }
            }
            if !ok {
                // 下载不动的文件（受限/已下架）转手动补充：projectID/fileID 按 pending
                // 构造顺序从解析结果回查（mrpack=files[idx]，CurseForge=cf_files[idx]）
                let (pid, fid) = if parsed.format == "curseforge" {
                    parsed.cf_files.get(idx).cloned().unwrap_or((0, 0))
                } else {
                    let f = parsed.files.get(idx).cloned().unwrap_or(Value::Null);
                    (
                        f["projectID"].as_i64().unwrap_or(0),
                        f["fileID"].as_i64().unwrap_or(0),
                    )
                };
                let name = std::path::Path::new(rel)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                emit(
                    app,
                    &rec.id,
                    "modpack",
                    0.45 + 0.5 * (done as f64 / total.max(1) as f64),
                    &format!("「{name}」自动下载失败，稍后可手动补充"),
                );
                manual.push(ManualFile {
                    project_id: pid,
                    file_id: fid,
                    file_name: name,
                    size: *size,
                    sha1: sha1.to_lowercase(),
                    dest: dest.clone(),
                });
                continue;
            }
            done += 1;
            done_bytes += size;
            emit(
                app,
                &rec.id,
                "modpack",
                0.45 + 0.5 * (done as f64 / total.max(1) as f64),
                &format!("下载整合包文件 {done}/{total}（{}MB）", done_bytes / 1_048_576),
            );
            let _ = DownloadDetail::default();
        }

        // 3.5) 手动补充：下不动的文件挂起等待玩家经 modpack:supplyFiles 补齐（取消即失败）
        if !manual.is_empty() {
            wait_manual_files(app, rec, manual.clone())?;
            // 事后复核（防会话竞态）：补齐的文件必须真实落位且校验通过
            for f in &manual {
                let expected = crate::net::Integrity {
                    sha1: Some(f.sha1.clone()),
                    sha512: None,
                    sha256: None,
                    size: Some(f.size),
                };
                if let Some(reason) = crate::net::verify_file(&f.dest, &expected, Some(rec.cancelled.as_ref()))? {
                    return Err(format!("手动补充的文件校验未通过（{}：{}）", f.file_name, reason));
                }
            }
        }

        // 4) 实例 json：pack 元数据 + 自包含化
        let json_path = inst_dir.join(format!("{id}.json"));
        let mut instance_json = crate::installer::read_version_json_from(&json_path)?;
        if let Some(obj) = instance_json.as_object_mut() {
            obj["id"] = json!(id);
            obj["modpackName"] = json!(parsed.name);
            obj["modpackVersion"] = json!(parsed.pack_version);
            obj["_gameDir"] = json!(true);
        }
        std::fs::write(&json_path, serde_json::to_string_pretty(&instance_json).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        let _ = crate::installer::flatten_instance_public(&id, &target_folder);

        // 5) overrides 覆盖
        emit(app, &rec.id, "modpack", 0.96, "解压覆盖文件…");
        let mut override_files = extract_overrides(&mut archive, &parsed.overrides_prefix, &inst_dir, Some(rec), &mut |done, total, name| {
            emit(app, &rec.id, "modpack", 0.96 + 0.02 * done as f64 / total.max(1) as f64, &format!("正在写入整合包配置 {done}/{total} · {name}"));
        })?;
        if let Some(client_prefix) = &parsed.client_overrides_prefix {
            let client_files = extract_overrides(&mut archive, client_prefix, &inst_dir, Some(rec), &mut |_, _, _| {})?;
            override_files.extend(client_files);
        }

        // 6) manifest
        let mut managed: HashSet<String> = pending.iter().map(|p| p.0.clone()).collect();
        managed.extend(override_files);
        let manifest = json!({
            "schemaVersion": 1,
            "name": parsed.name,
            "version": parsed.pack_version,
            "managedFiles": managed.into_iter().collect::<Vec<_>>(),
            "installedAt": chrono::Utc::now().to_rfc3339()
        });
        std::fs::write(inst_dir.join(PACK_MANIFEST), serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;

        emit(app, &rec.id, "done", 1.0, &format!("{} 安装完成", parsed.name));
        Ok(id)
    })();

    match result {
        Ok(id) => {
            println!("[modpacks] {} → {}（{:.1}s）", file_name, id, started.elapsed().as_secs_f32());
            Ok(id)
        }
        Err(e) => {
            // 新装失败整目录清理（update/overwrite 的备份恢复接入备份模块后补全）
            if action == "new" || action == "rename" {
                let _ = std::fs::remove_dir_all(&inst_dir);
            }
            Err(e)
        }
    }
}

/// 统一入口
/// 社区整合包下载完成后的后台安装（community.rs 的 ModpackInstallJob → install_modpack）。
/// 对应原版 community.ts `installModpack(tmpPath, installProgress, { signal, nameSource: 'inner', targetFolder })`：
/// 实例名取包内名、重名自动避让（conflictAction 默认 new）；结束（无论成败）交回 finish，
/// 由社区侧清理临时目录并推 event:taskDone / event:installDone。
pub fn install_modpack_job(job: crate::community::ModpackInstallJob) {
    let crate::community::ModpackInstallJob {
        app,
        task_id,
        pack_path,
        target_folder,
        emit_install,
        finish,
        ..
    } = job;
    let outcome = (|| -> Result<String, String> {
        let Some(rec) = crate::tasks::get_task(&task_id) else {
            return Err("任务已结束".into());
        };
        let sink = move |stage: &str, progress: f64, text: &str| {
            emit_install(&json!({ "stage": stage, "progress": progress, "text": text }));
        };
        let opts = json!({
            "nameSource": "inner",
            "targetFolder": target_folder.clone().unwrap_or_default(),
        });
        install_modpack_with_sink(&app, &pack_path.to_string_lossy(), &opts, &rec, Some(&sink))
    })();
    (finish)(outcome);
}

pub fn invoke(app: &tauri::AppHandle, channel: &str, args: &[Value]) -> Option<Result<Value, String>> {
    match channel {
        "modpack:probe" => Some(
            args.first()
                .and_then(|v| v.as_str())
                .ok_or_else(|| "缺少参数".to_string())
                .and_then(probe_modpack),
        ),
        "import:probe" => Some(
            args.first()
                .and_then(|v| v.as_str())
                .ok_or_else(|| "缺少参数".to_string())
                .and_then(probe_import),
        ),
        "modpack:install" => Some((|| {
            let file_path = args.first().and_then(|v| v.as_str()).ok_or("缺少参数")?.to_string();
            let opts = args.get(1).cloned().unwrap_or(json!({}));
            let rec = crate::tasks::register_task("导入整合包", "modpack");
            let app2 = app.clone();
            let file2 = file_path.clone();
            let rec2 = rec.clone();
            std::thread::spawn(move || {
                let result = install_modpack(&app2, &file2, &opts, &rec2);
                let version_id = result.as_ref().ok().cloned();
                let cancelled = result.as_ref().err().map(|e| e == "已取消").unwrap_or(false);
                crate::tasks::task_done(
                    &app2,
                    &rec2.id,
                    result.is_ok(),
                    result.as_ref().err().cloned(),
                    cancelled,
                    None,
                );
                // event:installDone（versionId = 新实例 id）
                crate::installer::emit_event(
                    &app2,
                    "event:installDone",
                    &json!({
                        "versionId": opts["targetVersionId"].as_str().unwrap_or(""),
                        "installedId": version_id,
                        "ok": result.is_ok(),
                        "error": result.as_ref().err(),
                        "taskId": rec2.id,
                        "cancelled": cancelled
                    }),
                );
            });
            Ok(Value::Null)
        })()),
        "modpack:supplyFiles" => Some((|| {
            let token = args.first().and_then(|v| v.as_str()).ok_or("缺少参数")?;
            let remaining = manual_sessions()
                .lock()
                .unwrap()
                .get(token)
                .map(|s| s.files.len())
                .ok_or_else(|| "该补充文件任务已结束，请查看下载中心".to_string())?;
            // 主进程侧多选框（对应原版 dialog.showOpenDialog multiSelections）
            use tauri_plugin_dialog::DialogExt;
            let picked = app
                .dialog()
                .file()
                .set_title(format!("补充整合包文件（剩余 {remaining} 个）"))
                .add_filter("模组文件", &["jar", "disabled"])
                .blocking_pick_files();
            let Some(paths) = picked else {
                return Ok(json!({ "accepted": 0, "remaining": remaining, "rejected": [] }));
            };
            let paths: Vec<std::path::PathBuf> = paths.into_iter().map(|p| std::path::PathBuf::from(p.to_string())).collect();
            if paths.is_empty() {
                return Ok(json!({ "accepted": 0, "remaining": remaining, "rejected": [] }));
            }
            supply_manual_files(app, token, &paths)
        })()),
        "modpack:openFile" => Some((|| {
            let _token = args.first().and_then(|v| v.as_str()).unwrap_or("");
            let file_id = args.get(1).and_then(|v| v.as_i64()).unwrap_or(0);
            use tauri_plugin_opener::OpenerExt;
            app.opener()
                .open_url(format!("https://www.curseforge.com/minecraft/mc-mods/files/{file_id}"), None::<&str>)
                .map_err(|e| e.to_string())?;
            Ok(Value::Null)
        })()),
        _ => None,
    }
}
