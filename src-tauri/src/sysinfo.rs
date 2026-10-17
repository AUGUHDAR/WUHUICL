//! 系统信息 / 光标位置（对应原版 app:systemInfo 与闪屏 pointerTimer 的取点）

use windows_sys::Win32::Foundation::POINT;
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;

/// app:systemInfo → 对应原版 ipc.ts appSystemInfo：credentialStorage + 内存 + platformInfo
pub fn system_info() -> serde_json::Value {
    let mut ms: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    ms.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    let (total, free) = unsafe {
        if GlobalMemoryStatusEx(&mut ms) != 0 {
            (ms.ullTotalPhys, ms.ullAvailPhys)
        } else {
            (0, 0)
        }
    };
    // 对应 credentialStorageStatus(safeStorage)：Windows 上只要 DPAPI 可用即 persistent
    let persistent = dpapi_available();
    serde_json::json!({
        "credentialStorage": {
            "persistent": persistent,
            "sessionOnly": false,
            "message": if persistent { "" } else { "系统安全存储当前不可用，无法安全保存登录令牌" }
        },
        "totalMemMB": total / (1024 * 1024),
        "freeMemMB": free / (1024 * 1024),
        "platform": "windows",
        "architecture": architecture(),
        "installation": if cfg!(debug_assertions) { "development" } else { "portable-exe" },
        "systemMemoryOrganizing": true,
        "nativeGameRuntime": "supported"
    })
}

/// 对应 productArchitecture(process.arch)：x64 / arm64，其余 unsupported
fn architecture() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        _ => "unsupported",
    }
}

fn dpapi_available() -> bool {
    crate::mslogin::dpapi_available()
}

/// 当前光标物理坐标（屏幕坐标系）
pub fn cursor_pos() -> Option<(i32, i32)> {
    let mut pt = POINT { x: 0, y: 0 };
    unsafe {
        if GetCursorPos(&mut pt) != 0 {
            Some((pt.x, pt.y))
        } else {
            None
        }
    }
}
