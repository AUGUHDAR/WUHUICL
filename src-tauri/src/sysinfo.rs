//! 系统信息 / 光标位置（对应原版 app:systemInfo 与闪屏 pointerTimer 的取点）。
//! OS 差异统一走 platform.rs。

/// app:systemInfo → 对应原版 ipc.ts appSystemInfo：credentialStorage + 内存 + platformInfo
pub fn system_info() -> serde_json::Value {
    let (total, free) = crate::platform::memory_info_mb();
    // 对应 credentialStorageStatus(safeStorage)：Windows 上只要 DPAPI 可用即 persistent；
    // macOS/Linux 尚无 Keychain/libsecret 集成，恒 false（前端有对应提示文案）
    let persistent = crate::platform::credential_available();
    // 非 Windows 的降级说明按平台区分：Android 沙箱内 base64 包装可持久保存（仅不加密），
    // 桌面 Linux/macOS 则明确无法持久保存令牌
    let storage_message = if persistent {
        ""
    } else if cfg!(target_os = "android") {
        "本机使用应用沙箱保存登录令牌（重启后仍在）；令牌未加密，请勿在共享设备上登录敏感账号"
    } else {
        "系统安全存储当前不可用，无法安全保存登录令牌"
    };
    serde_json::json!({
        "credentialStorage": {
            "persistent": persistent,
            "sessionOnly": false,
            "message": storage_message
        },
        "totalMemMB": total,
        "freeMemMB": free,
        "platform": crate::platform::electron_platform(),
        "architecture": architecture(),
        "installation": installation(),
        "systemMemoryOrganizing": cfg!(windows),
        "nativeGameRuntime": if crate::platform::desktop_game_supported() { "supported" } else { "unsupported" }
    })
}

/// 安装形态：开发构建 / Windows 便携 exe / 其它平台包管理器安装
fn installation() -> &'static str {
    if cfg!(debug_assertions) {
        "development"
    } else if cfg!(windows) {
        "portable-exe"
    } else {
        "installed"
    }
}

/// 系统当前可用物理内存（MB，对应 os.freemem()）：内存整理前后对比用
pub fn available_memory_mb() -> u64 {
    crate::platform::memory_info_mb().1
}

/// 对应 productArchitecture(process.arch)：x64 / arm64，其余 unsupported
fn architecture() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        _ => "unsupported",
    }
}

/// 当前光标物理坐标（屏幕坐标系）。非 Windows 返回 None，闪屏降级为无指针跟随。
pub fn cursor_pos() -> Option<(i32, i32)> {
    crate::platform::cursor_pos()
}

/// 读剪贴板文本（app:readClipboard），实现见 platform.rs。
pub fn clipboard_text() -> Option<String> {
    crate::platform::clipboard_text()
}
