//! 启动失败诊断 ZIP（对应原版 core/exportLogs.ts + diagnostics.ts + diagnosticArchive.ts）：
//! 所有文本先脱敏，缺失项写入 manifest 占位，不因单个日志缺失而失败。
//!
//! 通道：launch:exportLogs → 弹原生保存对话框 → 生成 ZIP → 返回路径（取消返回 null）

use crate::installer;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;

/// 单个日志上限（diagnosticArchive.MAX_LOG_BYTES 16MB，只取末尾）
const MAX_LOG_BYTES: u64 = 16 * 1024 * 1024;

/// 返回 None 表示本模块不认识该通道（由路由继续分发）；Some(result) 即最终结果。
pub fn invoke(app: &tauri::AppHandle, channel: &str, args: &[Value]) -> Option<Result<Value, String>> {
    match channel {
        "launch:exportLogs" => Some(export_logs(app, args)),
        _ => None,
    }
}

/// launch:exportLogs（ipc.ts）：withGameFolder → exportLaunchLogs
fn export_logs(app: &AppHandle, args: &[Value]) -> Result<Value, String> {
    let version_id = args
        .first()
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let folder = args
        .get(1)
        .and_then(|v| v.as_str())
        .filter(|f| !f.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| installer::folder_of_version(app, &version_id));
    export_launch_logs(app, &version_id, &folder).map(|path| path.map(|p| json!(p)).unwrap_or(Value::Null))
}

// ---------------- 脱敏（diagnostics.ts） ----------------

fn redact_text(input: &str, known_secrets: &[String]) -> String {
    let mut out = input.to_string();
    for secret in known_secrets {
        if secret.is_empty() || secret.len() < 4 {
            continue;
        }
        out = out.replace(secret.as_str(), "<redacted>");
    }
    // Authorization: Bearer ***
    out = regex::Regex::new(r"(?i)(Authorization\s*:\s*Bearer\s+)[^\s,;]+")
        .unwrap()
        .replace_all(&out, "${1}<redacted>")
        .to_string();
    // "token-ish key": "value" / token-ish key: value
    let token_keys = r"(?:access[_-]?token|refresh[_-]?token|client[_-]?token|session[_-]?(?:id|key)|password|authorization)";
    out = regex::Regex::new(&format!(r#"(?i)("?{}"?\s*[:=]\s*")([^"\r\n]+)(")"#, token_keys))
        .unwrap()
        .replace_all(&out, "${1}<redacted>${3}")
        .to_string();
    out = regex::Regex::new(&format!(r"(?i)({}\s*[:=]\s*)([^\s,;]+)", token_keys))
        .unwrap()
        .replace_all(&out, "${1}<redacted>")
        .to_string();
    out = regex::Regex::new(r#"(?i)(--accessToken|--clientId|--xuid|--userProperties)\s+("[^"]*"|\S+)"#)
        .unwrap()
        .replace_all(&out, "$1 <redacted>")
        .to_string();
    out = regex::Regex::new(r#"(?i)(--username|--uuid)\s+("[^"]*"|\S+)"#)
        .unwrap()
        .replace_all(&out, "$1 <redacted-user>")
        .to_string();
    out = regex::Regex::new(r"(?i)([?&](?:access_token|refresh_token|token|key)=)[^&#\s]+")
        .unwrap()
        .replace_all(&out, "${1}<redacted>")
        .to_string();
    // 用户主目录 → %USERPROFILE%
    if let Some(home) = dirs::home_dir() {
        out = out.replace(&home.to_string_lossy().to_string(), "%USERPROFILE%");
    }
    out = regex::Regex::new(r#"[A-Za-z]:\\Users\\[^\\\s"']+"#)
        .unwrap()
        .replace_all(&out, "%USERPROFILE%")
        .to_string();
    out
}

fn redact_path(value: &str, secrets: &[String]) -> String {
    if value.is_empty() {
        return "（未知）".into();
    }
    redact_text(value, secrets)
}

/// 仅用于 ZIP 文件名（diagnostics.ts safeDiagnosticFilePart）
fn safe_file_part(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .map(|c| if "<>:\"/\\|?*".contains(c) || (c as u32) < 0x20 { '_' } else { c })
        .collect();
    let cleaned = cleaned.trim().to_string();
    if cleaned.is_empty() {
        "unknown".into()
    } else {
        cleaned.chars().take(80).collect()
    }
}

// ---------------- 日志收集 ----------------

struct DiagnosticSource {
    archive_path: String,
    source: PathBuf,
    missing_placeholder: bool,
}

struct ManifestEntry {
    archive_path: String,
    source: String,
    status: &'static str,
    original_bytes: Option<u64>,
    included_bytes: Option<usize>,
    modified_at: Option<String>,
    truncated: bool,
    note: String,
}

/// 读取文件末尾 ≤16MB（diagnosticArchive.readTextTail）
fn read_text_tail(file: &Path) -> Result<(String, u64, bool, String), String> {
    let stat = std::fs::metadata(file).map_err(|e| e.to_string())?;
    let included = stat.len().min(MAX_LOG_BYTES);
    let mut handle = std::fs::File::open(file).map_err(|e| e.to_string())?;
    handle
        .seek(std::io::SeekFrom::Start(stat.len() - included))
        .map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; included as usize];
    handle.read_exact(&mut buf).map_err(|e| e.to_string())?;
    let truncated = stat.len() > MAX_LOG_BYTES;
    let text = format!(
        "{}{}",
        if truncated {
            format!("[WUHUICL] 日志过大，仅包含末尾 {} 字节。\n", included)
        } else {
            String::new()
        },
        String::from_utf8_lossy(&buf)
    );
    let modified_at = stat
        .modified()
        .ok()
        .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .unwrap_or_default();
    Ok((text, stat.len(), truncated, modified_at))
}

/// 添加一个脱敏日志（缺失时按 missingPlaceholder 决定是否写占位）
fn add_sanitized_log(
    zip: &mut zip::ZipWriter<std::io::BufWriter<std::fs::File>>,
    files: &mut Vec<ManifestEntry>,
    spec: &DiagnosticSource,
    secrets: &[String],
) {
    let mut entry = ManifestEntry {
        archive_path: spec.archive_path.clone(),
        source: redact_path(&spec.source.to_string_lossy(), secrets),
        status: "missing",
        original_bytes: None,
        included_bytes: None,
        modified_at: None,
        truncated: false,
        note: String::new(),
    };
    match read_text_tail(&spec.source) {
        Ok((text, original_bytes, truncated, modified_at)) => {
            let redacted = redact_text(&text, secrets);
            let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
            if zip.start_file(spec.archive_path.as_str(), options).is_ok() {
                let _ = zip.write_all(redacted.as_bytes());
            }
            entry.status = "included";
            entry.original_bytes = Some(original_bytes);
            entry.included_bytes = Some(redacted.len());
            entry.modified_at = Some(modified_at);
            entry.truncated = truncated;
        }
        Err(e) => {
            let missing = !spec.source.exists();
            entry.status = if missing { "missing" } else { "unreadable" };
            if !missing {
                entry.note = redact_text(&e, secrets);
            }
            if spec.missing_placeholder {
                let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
                if zip.start_file(spec.archive_path.as_str(), options).is_ok() {
                    let _ = zip.write_all(
                        format!("[WUHUICL] 此项{}；请查看 manifest.json。\n", if missing { "不存在" } else { "无法读取" }).as_bytes(),
                    );
                }
            }
        }
    }
    files.push(entry);
}

fn newest_crash_report(dir: &Path) -> Option<PathBuf> {
    let root = dir.join("crash-reports");
    let mut files: Vec<(PathBuf, std::time::SystemTime)> = Vec::new();
    for entry in std::fs::read_dir(root).ok()?.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let lower = name.to_lowercase();
        if !(lower.ends_with(".txt") || lower.ends_with(".log")) {
            continue;
        }
        if let Ok(meta) = entry.metadata() {
            if meta.is_file() {
                files.push((entry.path(), meta.modified().unwrap_or(std::time::UNIX_EPOCH)));
            }
        }
    }
    files.sort_by(|a, b| b.1.cmp(&a.1));
    files.first().map(|(p, _)| p.clone())
}

/// java 概要（exportLogs.ts javaSummary：-version 输出解析）
fn java_summary(java_path: &str, secrets: &[String]) -> Value {
    let unknown = json!({
        "path": redact_path(java_path, secrets),
        "version": "（未知）",
        "architecture": "（未知）"
    });
    if java_path.is_empty() {
        return unknown;
    }
    let mut cmd = std::process::Command::new(java_path);
    cmd.args(["-Xms8m", "-Xmx32m", "-version"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    match cmd.output() {
        Ok(out) => {
            let mut text = String::from_utf8_lossy(&out.stderr).to_string();
            text.push('\n');
            text.push_str(&String::from_utf8_lossy(&out.stdout));
            let version = regex::Regex::new(r#"version\s+"([^"]+)""#)
                .unwrap()
                .captures(&text)
                .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
                .or_else(|| text.lines().next().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()))
                .unwrap_or_else(|| "（未知）".into());
            let architecture = if regex::Regex::new(r"(?i)64-Bit|x86_64|aarch64").unwrap().is_match(&text) {
                "64-bit"
            } else if regex::Regex::new(r"(?i)32-Bit|i[3-6]86|x86").unwrap().is_match(&text) {
                "32-bit"
            } else {
                "（未知）"
            };
            json!({
                "path": redact_path(java_path, secrets),
                "version": version,
                "architecture": architecture
            })
        }
        Err(e) => json!({
            "path": redact_path(java_path, secrets),
            "version": format!("验证失败：{}", redact_text(&e.to_string(), secrets)),
            "architecture": "（未知）"
        }),
    }
}

fn fmt_stamp(now: chrono::DateTime<chrono::Local>) -> String {
    now.format("%Y%m%d-%H%M%S").to_string()
}

/// 诊断摘要（exportLogs.ts summaryText，文案逐行对齐）
fn summary_text(m: &Value) -> String {
    let exit_code = m["process"]["exitCode"]
        .as_i64()
        .map(|c| c.to_string())
        .unwrap_or_else(|| "（未知）".to_string());
    let pid = m["process"]["pid"]
        .as_i64()
        .map(|p| p.to_string())
        .unwrap_or_else(|| "（未知）".to_string());
    let loader_version = m["instance"]["loaderVersion"]
        .as_str()
        .map(|v| format!(" {}", v))
        .unwrap_or_default();
    let lines = vec![
        "================ WUHUICL 启动失败诊断摘要 ================".to_string(),
        format!("导出时间: {}", m["exportedAt"].as_str().unwrap_or("")),
        format!("WUHUICL: {}", m["launcher"]["version"].as_str().unwrap_or("")),
        format!(
            "操作系统: {} {} ({})",
            m["operatingSystem"]["platform"].as_str().unwrap_or(""),
            m["operatingSystem"]["release"].as_str().unwrap_or(""),
            m["operatingSystem"]["architecture"].as_str().unwrap_or("")
        ),
        String::new(),
        format!(
            "实例: {} [{}]",
            m["instance"]["name"].as_str().unwrap_or(""),
            m["instance"]["id"].as_str().unwrap_or("")
        ),
        format!("Minecraft: {}", m["instance"]["minecraftVersion"].as_str().unwrap_or("")),
        format!(
            "Loader: {}{}",
            m["instance"]["loader"].as_str().unwrap_or("vanilla"),
            loader_version
        ),
        format!("实例目录: {}", m["instance"]["directory"].as_str().unwrap_or("")),
        format!(
            "版本隔离: {}",
            if m["instance"]["isolated"].as_bool().unwrap_or(false) { "开启" } else { "关闭" }
        ),
        String::new(),
        format!(
            "Java: {} ({})",
            m["java"]["version"].as_str().unwrap_or(""),
            m["java"]["architecture"].as_str().unwrap_or("")
        ),
        format!("Java 路径: {}", m["java"]["path"].as_str().unwrap_or("")),
        format!("进程 PID: {}", pid),
        format!("启动时间: {}", m["process"]["startedAt"].as_str().unwrap_or("（未知）")),
        format!("退出时间: {}", m["process"]["endedAt"].as_str().unwrap_or("（未知）")),
        format!("退出码: {}", exit_code),
        format!("进程错误: {}", m["process"]["spawnError"].as_str().unwrap_or("（无记录）")),
        format!("启动参数摘要: {}", m["process"]["command"].as_str().unwrap_or("（尚未生成）")),
        String::new(),
        "每个日志的来源、缺失与截断情况见 manifest.json。".to_string(),
    ];
    lines.join("\n")
}

/// exportLogs.ts exportLaunchLogs：收集 crash-report + latest.log 等写入诊断 ZIP
pub fn export_launch_logs(app: &AppHandle, version_id: &str, folder: &str) -> Result<Option<String>, String> {
    // 扫描同时建立“版本 -> 游戏文件夹”映射，避免活动目录切换后收错实例日志
    let installed = crate::versions::scan_folder_public(folder, true);
    let last = crate::launch::last_launch();
    let vid = if version_id.is_empty() {
        last.as_ref().and_then(|l| l["versionId"].as_str()).unwrap_or("unknown").to_string()
    } else {
        version_id.to_string()
    };
    let item = installed
        .iter()
        .find(|v| v["id"].as_str() == Some(vid.as_str()) && installer::same_path(v["folder"].as_str().unwrap_or(""), folder));
    let mut directory_state = match &item {
        Some(i) => json!({
            "path": i["gameDirectory"].as_str().unwrap_or(folder),
            "isolated": i["isolated"] == true
        }),
        None => json!({ "path": folder, "isolated": false }),
    };
    if let Ok(vj) = installer::read_version_json(app, folder, &vid) {
        directory_state = installer::instance_directory_state(app, folder, &vid, &vj);
    }
    let effective_game_dir = if last
        .as_ref()
        .map(|l| l["versionId"].as_str() == Some(vid.as_str()) && l["effectiveGameDir"].is_string())
        .unwrap_or(false)
    {
        last.as_ref().unwrap()["effectiveGameDir"].as_str().unwrap_or("").to_string()
    } else {
        directory_state["path"].as_str().unwrap_or(folder).to_string()
    };
    let isolated = directory_state["isolated"].as_bool().unwrap_or(false);
    let now_local = chrono::Local::now();
    let def_name = format!(
        "WUHUICL-Diagnostic-{}-{}.zip",
        safe_file_part(item.as_ref().and_then(|i| i["mcVersion"].as_str()).unwrap_or(&vid)),
        fmt_stamp(now_local)
    );

    // 保存对话框（exportLogs.ts dialog.showSaveDialog）
    let picked = app
        .dialog()
        .file()
        .set_file_name(&def_name)
        .add_filter("ZIP 压缩包", &["zip"])
        .blocking_save_file();
    let Some(dest) = picked else { return Ok(None) };
    let destination = PathBuf::from(dest.to_string());

    // 脱敏密钥（账号凭据；loginIdentifier/userProperties 不入库）
    let account = crate::mslogin::selected_full(app);
    let secrets: Vec<String> = account
        .as_ref()
        .map(|a| {
            ["accessToken", "refreshToken", "clientToken", "loginIdentifier"]
                .iter()
                .filter_map(|k| a[*k].as_str().map(str::to_string))
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();

    let current_launch = if last
        .as_ref()
        .map(|l| {
            l["versionId"].as_str() == Some(vid.as_str())
                && l["effectiveGameDir"].as_str().map(|d| installer::same_path(d, &effective_game_dir)).unwrap_or(false)
        })
        .unwrap_or(false)
    {
        last.clone()
    } else {
        None
    };

    let os_arch = if cfg!(target_arch = "aarch64") { "aarch64" } else { "x86_64" };
    let java_path_owned: String = current_launch
        .as_ref()
        .and_then(|l| l["javaPath"].as_str())
        .map(str::to_string)
        .or_else(|| item.as_ref().and_then(|i| i["javaPath"].as_str()).map(str::to_string))
        .unwrap_or_else(|| {
            crate::settings::get(app)["javaPath"]
                .as_str()
                .unwrap_or("")
                .to_string()
        });
    let manifest = json!({
        "schemaVersion": 1,
        "exportedAt": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "launcher": { "name": "WUHUICL", "version": app.package_info().version.to_string() },
        "instance": {
            "id": vid,
            "name": item.as_ref().and_then(|i| i["modpackName"].as_str()).unwrap_or(&vid),
            "minecraftVersion": item.as_ref().and_then(|i| i["mcVersion"].as_str()).unwrap_or("（未知）"),
            "loader": item.as_ref().and_then(|i| i["loader"].as_str()).map(|s| json!(s)).unwrap_or(Value::Null),
            "loaderVersion": item.as_ref().and_then(|i| i["loaderVersion"].as_str()).map(|s| json!(s)).unwrap_or(Value::Null),
            "directory": redact_path(&effective_game_dir, &secrets),
            "isolated": isolated
        },
        "process": {
            "pid": current_launch.as_ref().and_then(|l| l["pid"].as_i64()).map(|p| json!(p)).unwrap_or(Value::Null),
            "startedAt": current_launch.as_ref().and_then(|l| l["startedAt"].as_str()).map(|s| json!(s)).unwrap_or(Value::Null),
            "endedAt": current_launch.as_ref().and_then(|l| l["endedAt"].as_str()).map(|s| json!(s)).unwrap_or(Value::Null),
            "exitCode": current_launch.as_ref().and_then(|l| l["exitCode"].as_i64()).map(|c| json!(c)).unwrap_or(Value::Null),
            "spawnError": current_launch.as_ref().and_then(|l| l["spawnError"].as_str()).map(|s| json!(redact_text(s, &secrets))).unwrap_or(Value::Null),
            "command": current_launch.as_ref().and_then(|l| l["commandSummary"].as_str()).map(|s| json!(redact_text(s, &secrets))).unwrap_or(Value::Null)
        },
        "java": java_summary(&java_path_owned, &secrets),
        "operatingSystem": {
            "platform": format!("{} {}", std::env::consts::OS, std::env::consts::FAMILY),
            "release": std::env::consts::OS,
            "architecture": os_arch
        },
        "files": []
    });

    let crash = newest_crash_report(Path::new(&effective_game_dir));
    let launch_log_dir = current_launch
        .as_ref()
        .and_then(|l| l["logDir"].as_str())
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(folder).join("kamucl-logs"));
    let launcher_log = crate::settings::user_data_dir(app).join("logs").join("launcher-current.log");
    let mut sources = vec![
        DiagnosticSource {
            archive_path: match &crash {
                Some(c) => format!("crash-reports/{}", c.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()),
                None => "crash-reports/latest.txt".into(),
            },
            source: crash.clone().unwrap_or_else(|| Path::new(&effective_game_dir).join("crash-reports")),
            missing_placeholder: crash.is_none(),
        },
        DiagnosticSource {
            archive_path: "minecraft/latest.log".into(),
            source: Path::new(&effective_game_dir).join("logs").join("latest.log"),
            missing_placeholder: true,
        },
        DiagnosticSource {
            archive_path: "minecraft/debug.log".into(),
            source: Path::new(&effective_game_dir).join("logs").join("debug.log"),
            missing_placeholder: true,
        },
        DiagnosticSource {
            archive_path: "launcher/launcher-current.log".into(),
            source: launcher_log.clone(),
            missing_placeholder: true,
        },
        DiagnosticSource {
            archive_path: "launcher/launch-combined.log".into(),
            source: launch_log_dir.join("latest.log"),
            missing_placeholder: true,
        },
        DiagnosticSource {
            archive_path: "process/stdout.log".into(),
            source: launch_log_dir.join("stdout.log"),
            missing_placeholder: true,
        },
        DiagnosticSource {
            archive_path: "process/stderr.log".into(),
            source: launch_log_dir.join("stderr.log"),
            missing_placeholder: true,
        },
    ];
    // 历史会话归档一并带上（最多 3 份）
    if let Some(logs_dir) = launcher_log.parent() {
        let mut archives: Vec<String> = std::fs::read_dir(logs_dir)
            .map(|rd| {
                rd.flatten()
                    .filter_map(|e| {
                        let name = e.file_name().to_string_lossy().to_string();
                        regex::Regex::new(r"^launcher-\d{8}-\d{6}.*\.log$")
                            .unwrap()
                            .is_match(&name)
                            .then_some(name)
                    })
                    .collect()
            })
            .unwrap_or_default();
        archives.sort();
        archives.reverse();
        for name in archives.into_iter().take(3) {
            sources.push(DiagnosticSource {
                archive_path: format!("launcher/{}", name),
                source: logs_dir.join(&name),
                missing_placeholder: false,
            });
        }
    }

    write_diagnostic_archive(app, &destination, manifest, sources, &secrets).map(|_| Ok(Some(destination.to_string_lossy().to_string())))?
}

/// diagnosticArchive.ts writeDiagnosticArchive：任一打包错误删除未完成目标
fn write_diagnostic_archive(
    app: &AppHandle,
    destination: &Path,
    mut manifest: Value,
    sources: Vec<DiagnosticSource>,
    secrets: &[String],
) -> Result<(), String> {
    let _ = app;
    let file = std::fs::File::create(destination).map_err(|e| format!("写入诊断包失败：{}", e))?;
    let mut zip = zip::ZipWriter::new(std::io::BufWriter::new(file));
    let mut files: Vec<ManifestEntry> = Vec::new();
    for spec in &sources {
        add_sanitized_log(&mut zip, &mut files, spec, secrets);
    }
    files.sort_by(|a, b| a.archive_path.cmp(&b.archive_path));
    let files_json: Vec<Value> = files
        .iter()
        .map(|e| {
            let mut o = json!({
                "archivePath": e.archive_path,
                "source": e.source,
                "status": e.status
            });
            if let Some(b) = e.original_bytes {
                o["originalBytes"] = json!(b);
            }
            if let Some(b) = e.included_bytes {
                o["includedBytes"] = json!(b);
            }
            if let Some(m) = &e.modified_at {
                if !m.is_empty() {
                    o["modifiedAt"] = json!(m);
                }
            }
            if e.truncated {
                o["truncated"] = json!(true);
            }
            if !e.note.is_empty() {
                o["note"] = json!(e.note);
            }
            o
        })
        .collect();
    if let Some(obj) = manifest.as_object_mut() {
        obj.insert("files".into(), json!(files_json));
    }
    let summary = summary_text(&manifest);
    let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
    let mut write_err: Option<String> = None;
    if zip.start_file("summary.txt", options.clone()).is_err() {
        write_err = Some("zip 写入失败".into());
    } else {
        let _ = zip.write_all(redact_text(&summary, secrets).as_bytes());
    }
    if write_err.is_none() && zip.start_file("manifest.json", options).is_err() {
        write_err = Some("zip 写入失败".into());
    } else if write_err.is_none() {
        let _ = zip.write_all(serde_json::to_vec_pretty(&manifest).unwrap_or_default().as_slice());
    }
    let finish_result = zip.finish().map_err(|e| e.to_string());
    if let Err(e) = finish_result {
        write_err = Some(e);
    }
    if let Some(e) = write_err {
        let _ = std::fs::remove_file(destination);
        return Err(format!("写入诊断包失败：{}", e));
    }
    Ok(())
}

#[allow(dead_code)]
fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}
