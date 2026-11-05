//! 平台抽象层：所有 OS 差异集中在这一个模块。
//!
//! 现状：Windows 全功能；macOS/Linux 核心可用，Windows 专属能力
//! （内存整理、OLE 拖出、自更新、游戏窗口观察等）在调用方按
//! `os()` / 前端 platform 分支优雅降级。Android 仅保证可编译的
//! 架构准备（见 docs/ANDROID.md），未做运行时适配。

// ---------------- 平台标识 ----------------

/// 当前 OS：windows / macos / linux / android（std::env::consts::OS）
pub fn os() -> &'static str {
    std::env::consts::OS
}

/// 渲染层用的 Electron 风格平台名（对应 process.platform）：win32 / darwin / linux / android
pub fn electron_platform() -> &'static str {
    match os() {
        "windows" => "win32",
        "macos" => "darwin",
        other => other,
    }
}

/// Mojang 版本规则与 natives 分类器用的 os 名（rules.allow 里是 windows / osx / linux）
pub fn mojang_os() -> &'static str {
    match os() {
        "windows" => "windows",
        "macos" => "osx",
        _ => "linux",
    }
}

/// Mojang Java 运行时清单的平台键（java_runtime manifest 顶层键）。
/// 返回 None 表示当前系统/架构没有官方运行时可下（如 windows-arm64）。
pub fn mojang_runtime_key() -> Option<&'static str> {
    match (os(), std::env::consts::ARCH) {
        ("windows", "x86_64") => Some("windows-x64"),
        ("linux", "x86_64") => Some("linux"),
        ("linux", "aarch64") => Some("linux-arm64"),
        ("macos", "x86_64") => Some("mac-os"),
        ("macos", "aarch64") => Some("mac-os-arm64"),
        _ => None,
    }
}

/// 当前平台是否支持启动桌面版游戏（Android/iOS 不支持）
pub fn desktop_game_supported() -> bool {
    matches!(os(), "windows" | "macos" | "linux")
}

// ---------------- 路径 / JVM 约定 ----------------

/// java 可执行文件名
pub fn java_exe() -> &'static str {
    if cfg!(windows) {
        "java.exe"
    } else {
        "java"
    }
}

/// classpath 分隔符：Windows `;`，其余 `:`
pub fn classpath_separator() -> &'static str {
    if cfg!(windows) {
        ";"
    } else {
        ":"
    }
}

/// PATH 环境变量分隔符：Windows `;`，其余 `:`
pub fn path_env_separator() -> char {
    if cfg!(windows) {
        ';'
    } else {
        ':'
    }
}

/// 把 Maven 风格的相对路径（`com/mojang/…`、zip 内部条目名）拼到 base 下，
/// 产出各平台原生分隔符。原来各处 `replace('/', "\\")` 的跨平台替代。
pub fn join_rel(base: &std::path::Path, rel: &str) -> std::path::PathBuf {
    let mut out = base.to_path_buf();
    for part in rel.split(['/', '\\']).filter(|s| !s.is_empty()) {
        out.push(part);
    }
    out
}

/// 在系统文件管理器中打开文件/目录（explorer / open / xdg-open）
pub fn open_path(path: &str) -> Result<(), String> {
    let (prog, arg) = match os() {
        "windows" => ("explorer.exe", path.to_string()),
        "macos" => ("open", path.to_string()),
        _ => ("xdg-open", path.to_string()),
    };
    let mut cmd = std::process::Command::new(prog);
    cmd.arg(&arg);
    no_window(&mut cmd);
    cmd.spawn().map_err(|e| e.to_string())?;
    Ok(())
}

/// 在文件管理器中定位并选中文件（explorer /select,、open -R）。
/// Linux 无通用协议：降级为打开所在目录。
pub fn show_in_folder(path: &str) -> Result<(), String> {
    let p = std::path::Path::new(path);
    #[cfg(windows)]
    {
        let mut cmd = std::process::Command::new("explorer.exe");
        cmd.arg(format!("/select,{}", p.to_string_lossy()));
        no_window(&mut cmd);
        cmd.spawn().map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        let mut cmd = std::process::Command::new("open");
        cmd.arg("-R").arg(p);
        cmd.spawn().map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let parent = p.parent().unwrap_or(p);
        open_path(&parent.to_string_lossy())
    }
}

// ---------------- 进程 ----------------

/// 进程是否仍在运行（对应原版 process.kill(pid, 0) 探测）
pub fn process_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        const STILL_ACTIVE: u32 = 259;
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return false;
            }
            let mut code: u32 = 0;
            let ok = GetExitCodeProcess(handle, &mut code) != 0;
            CloseHandle(handle);
            ok && code == STILL_ACTIVE
        }
    }
    #[cfg(not(windows))]
    {
        // kill(pid, 0) 不发信号，仅做存在性/权限检查；ESRCH 表示不存在
        unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
    }
}

/// 请求进程优雅退出（关闭主窗口 / SIGTERM）。返回是否已发起。
pub fn graceful_close(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    #[cfg(windows)]
    {
        // PowerShell Process.CloseMainWindow 对 GUI 进程发 WM_CLOSE，等价 Electron 原版
        use std::os::windows::process::CommandExt;
        let script =
            format!("[Diagnostics.Process]::GetProcessById({pid}).CloseMainWindow() | Out-Null");
        let mut cmd = std::process::Command::new("powershell.exe");
        cmd.args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .creation_flags(0x0800_0000);
        cmd.spawn().is_ok()
    }
    #[cfg(not(windows))]
    {
        unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) == 0 }
    }
}

/// 强杀进程树（taskkill /T /F 或 SIGKILL）。
pub fn kill_tree(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut cmd = std::process::Command::new("taskkill");
        cmd.args(["/T", "/F", "/PID"])
            .arg(pid.to_string())
            .creation_flags(0x0800_0000);
        cmd.status().map(|s| s.success()).unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        // unix 下没有廉价的进程树枚举：先杀 pid，子进程由各自所有者管理
        unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) == 0 }
    }
}

// ---------------- 磁盘 / 内存 ----------------

/// 目录所在卷当前用户可用空间（对应 statfs 的 f_bavail * f_bsize）。
/// 失败返回 None：调用方跳过预检，写失败兜底仍会报错。
pub fn free_disk_bytes(dir: &std::path::Path) -> Option<u64> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
        let wide: Vec<u16> = dir
            .as_os_str()
            .to_string_lossy()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let mut avail: u64 = 0;
        let mut total: u64 = 0;
        let mut free: u64 = 0;
        let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut avail, &mut total, &mut free) };
        (ok != 0).then_some(avail)
    }
    #[cfg(not(windows))]
    {
        let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
        let c = std::ffi::CString::new(dir.to_string_lossy().as_bytes()).ok()?;
        let rc = unsafe { libc::statvfs(c.as_ptr(), &mut stat) };
        if rc != 0 {
            return None;
        }
        Some(stat.f_bavail as u64 * stat.f_bsize as u64)
    }
}

/// 物理内存 (总 MB, 当前可用 MB)。
/// macOS 的 sysconf 没有 AVPHYS_PAGES，可用值改走 mach `host_statistics64`
/// （free + inactive + speculative，与活动监视器「可用内存」同口径）；取不到时退化为总量。
pub fn memory_info_mb() -> (u64, u64) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        let mut ms: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
        ms.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        unsafe {
            if GlobalMemoryStatusEx(&mut ms) != 0 {
                (
                    ms.ullTotalPhys / (1024 * 1024),
                    ms.ullAvailPhys / (1024 * 1024),
                )
            } else {
                (0, 0)
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) }.max(1) as u64;
        let total_pages = unsafe { libc::sysconf(libc::_SC_PHYS_PAGES) }.max(1) as u64;
        let total = total_pages * page / (1024 * 1024);

        // 可用页 = free + inactive + speculative（macOS 的 inactive 可被立即回收）
        let mut stats: libc::vm_statistics64 = unsafe { std::mem::zeroed() };
        let mut count = libc::HOST_VM_INFO64_COUNT;
        let rc = unsafe {
            libc::host_statistics64(
                libc::mach_host_self(),
                libc::HOST_VM_INFO64,
                &mut stats as *mut libc::vm_statistics64 as libc::host_info64_t,
                &mut count,
            )
        };
        let avail = if rc == 0 {
            let free_pages = stats.free_count as u64
                + stats.inactive_count as u64
                + stats.speculative_count as u64;
            (free_pages * page / (1024 * 1024)).min(total)
        } else {
            total
        };
        (total, avail)
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        unsafe {
            let page = libc::sysconf(libc::_SC_PAGESIZE).max(1) as u64;
            let total_pages = libc::sysconf(libc::_SC_PHYS_PAGES).max(1) as u64;
            let total = total_pages * page / (1024 * 1024);
            let avail_pages = libc::sysconf(libc::_SC_AVPHYS_PAGES) as i64;
            let avail = if avail_pages > 0 {
                avail_pages as u64 * page / (1024 * 1024)
            } else {
                total
            };
            (total, avail)
        }
    }
}

// ---------------- 桌面辅助（光标 / 剪贴板 / 工作区） ----------------

/// 当前光标物理坐标（闪屏指针跟随用）。非 Windows 无轻量实现，返回 None 降级。
pub fn cursor_pos() -> Option<(i32, i32)> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::POINT;
        use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
        let mut pt = POINT { x: 0, y: 0 };
        unsafe {
            if GetCursorPos(&mut pt) != 0 {
                Some((pt.x, pt.y))
            } else {
                None
            }
        }
    }
    #[cfg(not(windows))]
    None
}

/// 读剪贴板文本（app:readClipboard）。
/// Windows 直读 CF_UNICODETEXT（WebView2 对 navigator.clipboard 会弹权限框）；
/// macOS 走 pbpaste；Linux 走 wl-paste / xclip，环境里没有则 None（前端回退 navigator.clipboard）。
pub fn clipboard_text() -> Option<String> {
    #[cfg(windows)]
    {
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
    #[cfg(not(windows))]
    {
        let run = |prog: &str, args: &[&str]| -> Option<String> {
            let out = std::process::Command::new(prog).args(args).output().ok()?;
            if !out.status.success() {
                return None;
            }
            let text = String::from_utf8_lossy(&out.stdout).to_string();
            let trimmed = text.trim_end_matches(['\r', '\n']).to_string();
            (!trimmed.is_empty()).then_some(trimmed)
        };
        if cfg!(target_os = "macos") {
            run("pbpaste", &[])
        } else {
            run("wl-paste", &["--no-newline"])
                .or_else(|| run("xclip", &["-selection", "clipboard", "-o"]))
        }
    }
}

/// 主显示器工作区尺寸（任务栏除外），游戏窗口 maximized 参数用。
/// 非 Windows 无 SPI_GETWORKAREA 等价物，返回 None（调用方回落到分辨率设置值）。
pub fn work_area_size() -> Option<(i64, i64)> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::RECT;
        use windows_sys::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETWORKAREA};
        let mut rect: RECT = unsafe { std::mem::zeroed() };
        unsafe {
            if SystemParametersInfoW(
                SPI_GETWORKAREA,
                0,
                &mut rect as *mut RECT as *mut core::ffi::c_void,
                0,
            ) != 0
            {
                Some((
                    (rect.right - rect.left) as i64,
                    (rect.bottom - rect.top) as i64,
                ))
            } else {
                None
            }
        }
    }
    #[cfg(not(windows))]
    None
}

// ---------------- 凭据存储（对应 Electron safeStorage） ----------------

/// 非 Windows 降级包装的魔术前缀（DPAPI 密文是任意二进制，不会撞上 ASCII 前缀）
#[cfg(not(windows))]
const PLAIN_FALLBACK_PREFIX: &[u8] = b"WUHUICL-PLAIN1:";

/// 安全存储是否可用（对应 safeStorage.isAvailability）：
/// Windows = DPAPI 探针；其余平台暂无 Keychain/libsecret 集成，恒 false（UI 已有提示文案）。
pub fn credential_available() -> bool {
    #[cfg(windows)]
    {
        crate::mslogin::dpapi_available()
    }
    #[cfg(not(windows))]
    false
}

/// 加密账号令牌。Windows 用 DPAPI；其余平台用带前缀的 base64 包装（非加密，仅占位）。
pub fn credential_protect(plain: &[u8]) -> Result<Vec<u8>, String> {
    #[cfg(windows)]
    {
        crate::mslogin::dpapi_protect(plain)
    }
    #[cfg(not(windows))]
    {
        use base64::Engine;
        let mut out = PLAIN_FALLBACK_PREFIX.to_vec();
        out.extend_from_slice(
            base64::engine::general_purpose::STANDARD
                .encode(plain)
                .as_bytes(),
        );
        Ok(out)
    }
}

/// 解密账号令牌；失败返回 None（调用方视为需要重新登录）。
pub fn credential_unprotect(blob: &[u8]) -> Option<Vec<u8>> {
    #[cfg(windows)]
    {
        crate::mslogin::dpapi_unprotect(blob)
    }
    #[cfg(not(windows))]
    {
        use base64::Engine;
        let text = std::str::from_utf8(blob).ok()?;
        let b64 = text.strip_prefix(std::str::from_utf8(PLAIN_FALLBACK_PREFIX).ok()?)?;
        base64::engine::general_purpose::STANDARD.decode(b64).ok()
    }
}

// ---------------- Windows 子进程辅助 ----------------

/// 给 Command 加「不弹控制台窗口」标志（仅 Windows 生效）。
/// 散落各处的 `#[cfg(windows)] creation_flags(...)` 统一收敛到这。
pub fn no_window(cmd: &mut std::process::Command) -> &mut std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000) // CREATE_NO_WINDOW
    }
    #[cfg(not(windows))]
    cmd
}
