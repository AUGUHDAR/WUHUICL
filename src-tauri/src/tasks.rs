//! 后台任务注册表（对应原版 core/tasks.ts）：
//! 安装/导入/下载类任务的取消/暂停控制；取消令牌贯穿下载循环。

use serde_json::json;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};

#[derive(Clone, Copy, PartialEq)]
pub enum TaskStatus {
    Running,
    Paused,
    Cancelling,
}

pub struct TaskRecord {
    pub id: String,
    pub title: String,
    pub kind: &'static str,
    pub cancelled: Arc<AtomicBool>,
    pub paused: Arc<AtomicBool>,
    pub status: Mutex<TaskStatus>,
}

static TASKS: Mutex<Option<HashMap<String, Arc<TaskRecord>>>> = Mutex::new(None);
static SEQ: AtomicU64 = AtomicU64::new(0);

fn with_tasks<T>(f: impl FnOnce(&mut HashMap<String, Arc<TaskRecord>>) -> T) -> T {
    let mut guard = TASKS.lock().unwrap();
    let map = guard.get_or_insert_with(HashMap::new);
    f(map)
}

/// registerTask：id 形如 `{kind}-{millis}-{seq}`（与原版一致）
pub fn register_task(title: &str, kind: &'static str) -> Arc<TaskRecord> {
    let id = format!("{}-{}-{}", kind, chrono::Utc::now().timestamp_millis(), SEQ.fetch_add(1, Ordering::SeqCst) + 1);
    let rec = Arc::new(TaskRecord {
        id: id.clone(),
        title: title.to_string(),
        kind,
        cancelled: Arc::new(AtomicBool::new(false)),
        paused: Arc::new(AtomicBool::new(false)),
        status: Mutex::new(TaskStatus::Running),
    });
    with_tasks(|m| m.insert(id, rec.clone()));
    rec
}

/// tasks:cancel → boolean（等待由调用方的轮询取消令牌实现）
pub fn cancel_task(id: &str) -> bool {
    let rec = with_tasks(|m| m.get(id).cloned());
    match rec {
        Some(rec) => {
            {
                let mut st = rec.status.lock().unwrap();
                if *st != TaskStatus::Cancelling {
                    *st = TaskStatus::Cancelling;
                    rec.cancelled.store(true, Ordering::SeqCst);
                    rec.paused.store(false, Ordering::SeqCst);
                }
            }
            true
        }
        None => false,
    }
}

pub fn pause_task(id: &str) -> bool {
    let rec = with_tasks(|m| m.get(id).cloned());
    match rec {
        Some(rec) => {
            if *rec.status.lock().unwrap() == TaskStatus::Running {
                *rec.status.lock().unwrap() = TaskStatus::Paused;
                rec.paused.store(true, Ordering::SeqCst);
                true
            } else {
                false
            }
        }
        None => false,
    }
}

pub fn resume_task(id: &str) -> bool {
    let rec = with_tasks(|m| m.get(id).cloned());
    match rec {
        Some(rec) => {
            if *rec.status.lock().unwrap() == TaskStatus::Paused {
                *rec.status.lock().unwrap() = TaskStatus::Running;
                rec.paused.store(false, Ordering::SeqCst);
                true
            } else {
                false
            }
        }
        None => false,
    }
}

/// finishTask：任务结束移除记录
pub fn finish_task(id: &str) {
    with_tasks(|m| {
        m.remove(id);
    });
}

pub fn get_task(id: &str) -> Option<Arc<TaskRecord>> {
    with_tasks(|m| m.get(id).cloned())
}

pub fn task_cancelled(id: &str) -> bool {
    get_task(id).map(|t| t.cancelled.load(Ordering::SeqCst)).unwrap_or(false)
}

/// 暂停门控：在下载循环里调用，暂停时阻塞直至恢复/取消（对应原版 pauseWaiter）
pub fn wait_while_paused(rec: &TaskRecord) {
    while rec.paused.load(Ordering::SeqCst) && !rec.cancelled.load(Ordering::SeqCst) {
        std::thread::sleep(std::time::Duration::from_millis(120));
    }
}

// ---------------- 事件推送（event:progress / event:taskDone） ----------------

/// 推送进度事件（ProgressEvent 形状由调用方给足；taskDone 由 task_done 收尾）
pub fn emit_progress(app: &tauri::AppHandle, event: serde_json::Value) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.emit_to("main", "event:progress", event);
    }
}

/// 任务收尾：event:taskDone {taskId, ok, error?, cancelled?, stage?} 并移除登记
pub fn task_done(
    app: &tauri::AppHandle,
    task_id: &str,
    ok: bool,
    error: Option<String>,
    cancelled: bool,
    stage: Option<String>,
) {
    let payload = json!({
        "taskId": task_id,
        "ok": ok,
        "error": error,
        "cancelled": cancelled,
        "stage": stage
    });
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.emit_to("main", "event:taskDone", payload);
    }
    finish_task(task_id);
}
