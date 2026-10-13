//! 启动门：复刻 shared/startup.ts 的 StartupGate 与 startupSplash.ts 的编排。
//!
//! 状态流：splash ready → 显示玻璃层并推送 boot:state；
//! 主渲染层逐阶段上报 boot:stage → 全部完成且 renderer-ready 且 painted 且 assembled → reveal。

use super::AppState;
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};

pub const BOOT_STAGES: [&str; 5] = ["settings", "accounts", "instances", "assets", "paint"];

#[derive(Default)]
pub struct BootGate {
    completed: Vec<String>,
    painted: bool,
    renderer_ready: bool,
    assembled: bool,
    revealed: bool,
}

impl BootGate {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn state(&self) -> serde_json::Value {
        json!({
            "completed": self.completed,
            "ready": self.painted && self.renderer_ready
        })
    }

    pub fn can_reveal(&self) -> bool {
        self.painted && self.renderer_ready && self.assembled
    }
}

fn publish(app: &AppHandle) {
    if let Some(splash) = app.get_webview_window("splash") {
        let state = {
            let st = app.state::<AppState>();
            let gate = st.gate.lock().unwrap();
            gate.state()
        };
        let _ = splash.emit_to("splash", "boot:state", state);
    }
}

pub fn set_painted(app: &AppHandle) {
    {
        let st = app.state::<AppState>();
        let mut gate = st.gate.lock().unwrap();
        gate.painted = true;
    }
    publish(app);
    try_reveal(app);
}

/// 主渲染层 boot:stage 上报（原版只认 BOOT_STAGES 内的阶段）
pub fn stage(app: &AppHandle, stage: &str) {
    if !BOOT_STAGES.contains(&stage) {
        return;
    }
    {
        let st = app.state::<AppState>();
        let mut gate = st.gate.lock().unwrap();
        if !gate.completed.iter().any(|s| s == stage) {
            gate.completed.push(stage.to_string());
        }
    }
    publish(app);
}

/// 主渲染层就绪（boot:renderer-ready）
pub fn renderer_ready(app: &AppHandle) {
    {
        let st = app.state::<AppState>();
        let mut gate = st.gate.lock().unwrap();
        gate.renderer_ready = true;
    }
    publish(app);
    try_reveal(app);
}

/// 闪屏动画装配完成（boot:assembled）
pub fn assembled(app: &AppHandle) {
    {
        let st = app.state::<AppState>();
        let mut gate = st.gate.lock().unwrap();
        gate.assembled = true;
    }
    try_reveal(app);
}

fn try_reveal(app: &AppHandle) {
    let can = {
        let st = app.state::<AppState>();
        let gate = st.gate.lock().unwrap();
        gate.can_reveal()
    };
    if !can {
        return;
    }
    let first = {
        let st = app.state::<AppState>();
        let mut gate = st.gate.lock().unwrap();
        if gate.revealed {
            false
        } else {
            gate.revealed = true;
            true
        }
    };
    if !first {
        return;
    }
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.show();
        let _ = main.set_focus();
    }
    if let Some(splash) = app.get_webview_window("splash") {
        let _ = splash.emit_to("splash", "boot:reveal", ());
        // 只兜底清理：闪屏动画正常会自己发 boot:finished
        let handle = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(1500));
            if let Some(s) = handle.get_webview_window("splash") {
                let _ = s.close();
            }
        });
    }
}

/// 闪屏动画完成（boot:finished）：销毁闪屏窗口
pub fn finish(app: &AppHandle) {
    if let Some(splash) = app.get_webview_window("splash") {
        let _ = splash.close();
    }
}

/// 闪屏失败（boot:splash-failed）：直接销毁并揭示主窗（动画失败不阻断启动）
pub fn splash_failed(app: &AppHandle) {
    finish(app);
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.show();
    }
}

/// 启动看门狗：原版渲染层失败会走 animationFailure 兜底；
/// 这里等价——主窗 45s 未就绪则强制揭示（加载失败时用户至少能看到界面状态）。
pub fn arm_watchdog(app: &AppHandle) {
    let handle = app.clone();
    std::thread::spawn(move || {
        for _ in 0..45 {
            std::thread::sleep(std::time::Duration::from_secs(1));
            {
                let st = handle.state::<AppState>();
                if st.gate.lock().unwrap().revealed {
                    return;
                }
            }
        }
        if let Some(main) = handle.get_webview_window("main") {
            let _ = main.show();
            let _ = main.set_focus();
        }
        if let Some(splash) = handle.get_webview_window("splash") {
            let _ = splash.close();
        }
    });
}

/// 闪屏窗口就绪（boot:splash-ready）：显示玻璃层并立即推送一次状态
pub fn splash_ready(app: &AppHandle) {
    if let Some(splash) = app.get_webview_window("splash") {
        let _ = splash.show();
        let _ = splash.set_always_on_top(true);
        publish(app);
    }
}
