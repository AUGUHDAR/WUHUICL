//! 游戏目录迁移（对应原版 core/gamedir.ts）：
//! 校验（相同/嵌套/可写/磁盘空间）+ 全量数据复制 + 配置切换。
//! 安全语义：复制完成前旧目录与旧配置绝不被修改；失败即回滚（配置最后才写）。
//!
//! 通道：gameDir:migrate → event:progress(stage=migrate) + event:gameDirDone

use crate::installer;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};

const GB: u64 = 1024 * 1024 * 1024;

/// 返回 None 表示本模块不认识该通道（由路由继续分发）；Some(result) 即最终结果。
pub fn invoke(
    app: &tauri::AppHandle,
    channel: &str,
    args: &[Value],
) -> Option<Result<Value, String>> {
    match channel {
        "gameDir:migrate" => Some(game_dir_migrate(app, args)),
        _ => None,
    }
}

/// gameDir:migrate（ipc.ts）：异步任务；event:progress + event:gameDirDone
fn game_dir_migrate(app: &AppHandle, args: &[Value]) -> Result<Value, String> {
    let new_dir = args
        .first()
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let migrate = args.get(1).and_then(|v| v.as_bool()).unwrap_or(false);
    let app2 = app.clone();
    std::thread::spawn(move || {
        let result = migrate_game_dir(&app2, &new_dir, migrate, &|event: &Value| {
            crate::tasks::emit_progress(&app2, event.clone());
        });
        let payload = match result {
            Ok(dir) => json!({ "ok": true, "gameDir": dir }),
            Err(e) => json!({ "ok": false, "error": e }),
        };
        if let Some(win) = app2.get_webview_window("main") {
            let _ = win.emit_to("main", "event:gameDirDone", payload);
        }
    });
    Ok(Value::Null)
}

fn emit(app: &AppHandle, event: Value) {
    crate::tasks::emit_progress(app, event);
}

// ---------------- 统计（gamedir.ts dirStats） ----------------

struct DirStats {
    files: u64,
    bytes: u64,
}

/// 递归统计目录文件数与总字节（跳过下载临时文件）
fn dir_stats(dir: &Path) -> DirStats {
    let mut files: u64 = 0;
    let mut bytes: u64 = 0;
    let mut walk = |d: &Path| {
        if let Ok(entries) = std::fs::read_dir(d) {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if name.ends_with(".part") || name.starts_with(".extract-") {
                    continue;
                }
                let p = e.path();
                if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    let sub = dir_stats(&p);
                    files += sub.files;
                    bytes += sub.bytes;
                } else {
                    files += 1;
                    if let Ok(meta) = e.metadata() {
                        bytes += meta.len();
                    }
                }
            }
        }
    };
    walk(dir);
    DirStats { files, bytes }
}

// ---------------- 校验（gamedir.ts checkTarget） ----------------

// GetDiskFreeSpaceExW（kernel32；windows-sys 的 Storage::FileSystem feature 未启用，手动声明）
#[link(name = "kernel32")]
extern "system" {
    fn GetDiskFreeSpaceExW(
        lpdirectoryname: *const u16,
        lpfreebytesavailabletocaller: *mut u64,
        lptotalnumberofbytes: *mut u64,
        lptotalnumberoffreebytes: *mut u64,
    ) -> i32;
}

fn free_disk_bytes(dir: &Path) -> Option<u64> {
    let wide: Vec<u16> = dir
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut free: u64 = 0;
    unsafe {
        if GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ) != 0
        {
            Some(free)
        } else {
            None
        }
    }
}

/// 解析路径（node path.resolve 等价：相对 → cwd 合并）
fn resolve_path(input: &str) -> PathBuf {
    let p = Path::new(input);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(p)
    }
}

/// 路径前缀包含（分隔符感知，避免 "D:\game" 误匹配 "D:\game2"）
fn starts_with_dir(child: &Path, parent: &Path) -> bool {
    child.starts_with(parent)
}

/// 校验目标目录：非当前目录/非嵌套、可创建可写、磁盘剩余空间 ≥ 需求量 × 1.2
pub fn check_target(app: &AppHandle, new_dir: &Path, need_bytes: u64) -> Result<(), String> {
    let cur = resolve_path(&installer::game_dir(app));
    if new_dir == cur {
        return Err("新目录与当前游戏目录相同，无需更改".into());
    }
    if starts_with_dir(new_dir, &cur) {
        return Err("新目录不能位于当前游戏目录内部（会导致无限递归复制）".into());
    }
    if starts_with_dir(&cur, new_dir) {
        return Err("新目录不能是当前游戏目录的父目录（迁移会破坏自身数据）".into());
    }
    std::fs::create_dir_all(new_dir).map_err(|e| format!("目标目录不可写（权限问题）：{}", e))?;
    // 权限测试：创建并删除一个临时文件
    let probe = new_dir.join(format!(
        ".kamucl-probe-{}",
        chrono::Utc::now().timestamp_millis()
    ));
    let write_result = std::fs::write(&probe, "ok");
    let _ = std::fs::remove_file(&probe);
    write_result.map_err(|e| format!("目标目录不可写（权限问题）：{}", e))?;
    if need_bytes > 0 {
        if let Some(free) = free_disk_bytes(new_dir) {
            let need = need_bytes.saturating_mul(12) / 10; // ×1.2
            if free < need {
                return Err(format!(
                    "目标磁盘剩余空间不足：需要约 {:.1} GB，可用 {:.1} GB",
                    need as f64 / GB as f64,
                    free as f64 / GB as f64
                ));
            }
        }
    }
    Ok(())
}

// ---------------- 迁移（gamedir.ts migrateGameDir） ----------------

/// 迁移游戏目录：migrate=true 先全量复制再切换配置；false 仅校验后切换。
pub fn migrate_game_dir(
    app: &AppHandle,
    new_dir: &str,
    migrate: bool,
    progress: &(dyn Fn(&Value) + Send + Sync),
) -> Result<String, String> {
    let src = installer::game_dir(app);
    let resolved = resolve_path(new_dir);
    let stats = if migrate {
        dir_stats(Path::new(&src))
    } else {
        DirStats { files: 0, bytes: 0 }
    };

    progress(&json!({ "stage": "migrate", "progress": 0, "text": "校验目标目录…" }));
    check_target(app, &resolved, stats.bytes)?;

    if migrate && stats.files > 0 {
        let done = Arc::new(Mutex::new(0u64));
        copy_walk(
            Path::new(&src),
            &resolved,
            stats.files,
            &done,
            &|text, fraction| {
                progress(&json!({ "stage": "migrate", "progress": fraction, "text": text }));
            },
        )?;
    }

    // 全部完成才切换配置（之前的任何失败都不会破坏现状）
    crate::gamefolders::set_download_game_folder(app, &resolved.to_string_lossy())?;
    progress(&json!({ "stage": "migrate", "progress": 1, "text": "游戏目录已切换" }));
    Ok(resolved.to_string_lossy().to_string())
}

fn copy_walk(
    from: &Path,
    to: &Path,
    total: u64,
    done: &Arc<Mutex<u64>>,
    progress: &(dyn Fn(&str, f64) + Send + Sync),
) -> Result<(), String> {
    let entries = std::fs::read_dir(from).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.ends_with(".part") || name.starts_with(".extract-") {
            continue;
        }
        let s = entry.path();
        let d = to.join(&name);
        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            copy_walk(&s, &d, total, done, progress)?;
        } else {
            let copy_result = std::fs::copy(&s, &d);
            if copy_result.is_err() {
                // 文件被占用时重试一次（如日志文件正被写入）
                std::thread::sleep(std::time::Duration::from_millis(300));
                std::fs::copy(&s, &d).map_err(|err| format!("复制失败：{}（{}）", name, err))?;
            }
            let mut count = done.lock().unwrap();
            *count += 1;
            if *count % 20 == 0 || *count == total {
                progress(
                    &format!("迁移游戏文件 {}/{}", count, total),
                    (*count as f64 / total as f64) * 0.98,
                );
            }
        }
    }
    Ok(())
}

/// 当前数据量概览 GB（前端确认弹窗展示用；对应当前 currentDataSizeGB）
#[allow(dead_code)]
pub fn current_data_size_gb(app: &AppHandle) -> f64 {
    dir_stats(Path::new(&installer::game_dir(app))).bytes as f64 / GB as f64
}
