//! 系统信息 / 光标位置（对应原版 app:systemInfo 与闪屏 pointerTimer 的取点）

use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
use windows_sys::Win32::Foundation::POINT;

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

/// 系统当前可用物理内存（MB，对应 os.freemem()）：内存整理前后对比用
pub fn available_memory_mb() -> u64 {
    let mut ms: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    ms.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    unsafe {
        if GlobalMemoryStatusEx(&mut ms) != 0 {
            ms.ullAvailPhys / (1024 * 1024)
        } else {
            0
        }
    }
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

/// 读剪贴板文本（app:readClipboard）。Win32 直读 CF_UNICODETEXT，
/// 不走 navigator.clipboard——WebView2 对读剪贴板会弹系统权限框，拖拽确认流程里不能有这东西。
pub fn clipboard_text() -> Option<String> {
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    };
    use windows_sys::Win32::System::Memory::{GlobalLock, GlobalUnlock};
    const CF_UNICODETEXT: u32 = 13;
    unsafe {
        if IsClipboardFormatAvailable(CF_UNICODETEXT) == 0 {
            return None;
        }
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return None;
        }
        let result = (|| {
            let handle = GetClipboardData(CF_UNICODETEXT);
            if handle.is_null() {
                return None;
            }
            let ptr = GlobalLock(handle) as *const u16;
            if ptr.is_null() {
                return None;
            }
            let mut len = 0usize;
            while *ptr.add(len) != 0 {
                len += 1;
            }
            let text = String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len));
            GlobalUnlock(handle);
            Some(text)
        })();
        CloseClipboard();
        result
    }
}
