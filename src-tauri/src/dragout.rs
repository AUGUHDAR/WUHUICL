//! 原生文件拖出（Windows OLE DoDragDrop + CF_HDROP）。
//! 对应原版 webContents.startDrag：资源管理器/模组页把文件拖到桌面、QQ 等任意接收窗口。
//! 数据对象只承载 CF_HDROP（拖出文件列表），效果 = 复制。

use std::cell::UnsafeCell;
use std::path::Path;
use std::ptr;

use windows::core::{implement, BOOL, HRESULT};
use windows::Win32::Foundation::{HANDLE, HGLOBAL, GlobalFree, POINT, S_OK};
use windows::Win32::System::Com::{
    DVASPECT_CONTENT, FORMATETC, IDataObject, IDataObject_Impl, IEnumFORMATETC,
    IEnumFORMATETC_Impl, STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::{
    IDropSource, IDropSource_Impl, CF_HDROP,
    DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_MOVE, DROPEFFECT_NONE,
};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};
use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, GetForegroundWindow};

const DRAGDROP_S_DROP: HRESULT = HRESULT(0x0004_0100);
const DRAGDROP_S_CANCEL: HRESULT = HRESULT(0x0004_0101);
const S_FALSE: HRESULT = HRESULT(1);
const E_POINTER: HRESULT = HRESULT(0x8000_4003_u32 as i32);
const E_FAIL: HRESULT = HRESULT(0x8000_4005_u32 as i32);
const E_OUTOFMEMORY: HRESULT = HRESULT(0x8007_000E_u32 as i32);
const DV_E_FORMATETC: HRESULT = HRESULT(0x8004_0002_u32 as i32);
const OLE_E_ADVISENOTSUPPORTED: HRESULT = HRESULT(0x8004_0001_u32 as i32);
const DATADIR_GET: u32 = 1;
const MK_LBUTTON: u32 = 0x0001;

/// 构造 CF_HDROP 的 HGLOBAL（DROPFILES 头 + 宽字符路径表 + 双零终止）
unsafe fn create_hdrop(paths: &[String]) -> Result<HANDLE, String> {
    let mut wide: Vec<u16> = Vec::new();
    for p in paths {
        wide.extend(p.encode_utf16());
        wide.push(0);
    }
    // DROPFILES 头 20 字节 + 路径表 + 第二个 null 终止
    let total = 20 + (wide.len() + 1) * 2;
    let hglobal = GlobalAlloc(GMEM_MOVEABLE, total).map_err(|_| "拖出内存分配失败".to_string())?;
    let locked = GlobalLock(hglobal) as *mut u8;
    if locked.is_null() {
        let _ = GlobalFree(Some(hglobal));
        return Err("拖出内存锁定失败".into());
    }
    // DROPFILES { pFiles: usize(offset=20), pt: POINT{0,0}, fNC: false, fWide: true }
    let header: [u32; 5] = [20, 0, 0, 0, 1];
    ptr::copy_nonoverlapping(header.as_ptr() as *const u8, locked, 20);
    let body = locked.add(20) as *mut u16;
    ptr::copy_nonoverlapping(wide.as_ptr(), body, wide.len());
    *body.add(wide.len()) = 0; // 第二个 null 终止符
    let _ = GlobalUnlock(hglobal);
    Ok(HANDLE(hglobal.0))
}

/// 释放 CF_HDROP HGLOBAL（拖放结束后数据对象里那份由我们自己释放）
unsafe fn free_hdrop(handle: HANDLE) {
    let _ = GlobalFree(Some(HGLOBAL(handle.0)));
}

// ---------------- IDropSource 实现 ----------------

#[implement(IDropSource)]
struct DropSource;

impl IDropSource_Impl for DropSource_Impl {
    fn QueryContinueDrag(&self, escape_pressed: BOOL, key_state: MODIFIERKEYS_FLAGS) -> HRESULT {
        if escape_pressed.as_bool() {
            return DRAGDROP_S_CANCEL;
        }
        if key_state.0 & MK_LBUTTON == 0 {
            return DRAGDROP_S_DROP;
        }
        S_OK
    }
    fn GiveFeedback(&self, _effect: DROPEFFECT) -> HRESULT {
        // 用系统默认光标（原版 startDrag 同语义）
        HRESULT(0x0004_0102) // DRAGDROP_S_USEDEFAULTCURSORS
    }
}

// ---------------- IDataObject 实现（只发 CF_HDROP） ----------------

#[implement(IDataObject)]
struct FileDataObject {
    hdrop: UnsafeCell<HANDLE>,
}

fn supported_format(format: &FORMATETC) -> bool {
    format.cfFormat == CF_HDROP.0
        && (format.tymed as u32 & TYMED_HGLOBAL.0 as u32) != 0
        && format.lindex == -1
        && format.dwAspect == DVASPECT_CONTENT.0 as u32
}

fn hdrop_format() -> FORMATETC {
    FORMATETC {
        cfFormat: CF_HDROP.0,
        ptd: ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    }
}

/// HDROP 内容总字节数：DROPFILES 头 + 路径表 + 双零
unsafe fn hdrop_total_bytes(handle: HANDLE) -> usize {
    let hdrop = HDROP(handle.0);
    let count = DragQueryFileW(hdrop, u32::MAX, None);
    let mut total = 20usize;
    for i in 0..count {
        let len = DragQueryFileW(hdrop, i, None) as usize; // 不含终止符
        total += (len + 1) * 2;
    }
    total += 2; // 第二个 null
    total
}

impl IDataObject_Impl for FileDataObject_Impl {
    fn GetData(&self, pformatetcin: *const FORMATETC) -> windows::core::Result<STGMEDIUM> {
        unsafe {
            if pformatetcin.is_null() || !supported_format(&*pformatetcin) {
                return Err(DV_E_FORMATETC.into());
            }
            let handle = *self.hdrop.get();
            if handle.is_invalid() {
                return Err(E_FAIL.into());
            }
            // 接收方 ReleaseStgMedium 会 free：复制一份交给它
            let size = hdrop_total_bytes(handle);
            let copy = GlobalAlloc(GMEM_MOVEABLE, size).map_err(|_| E_OUTOFMEMORY)?;
            let src = GlobalLock(HGLOBAL(handle.0)) as *const u8;
            let dst = GlobalLock(copy) as *mut u8;
            if src.is_null() || dst.is_null() {
                let _ = GlobalFree(Some(copy));
                return Err(E_FAIL.into());
            }
            ptr::copy_nonoverlapping(src, dst, size);
            let _ = GlobalUnlock(HGLOBAL(handle.0));
            let _ = GlobalUnlock(copy);
            Ok(STGMEDIUM {
                tymed: TYMED_HGLOBAL.0 as u32,
                u: STGMEDIUM_0 { hGlobal: copy },
                pUnkForRelease: std::mem::ManuallyDrop::new(None),
            })
        }
    }
    fn GetDataHere(&self, _pformatetc: *const FORMATETC, _pmedium: *mut STGMEDIUM) -> windows::core::Result<()> {
        Err(DV_E_FORMATETC.into())
    }
    fn QueryGetData(&self, pformatetc: *const FORMATETC) -> HRESULT {
        unsafe {
            if pformatetc.is_null() {
                return E_POINTER;
            }
            if supported_format(&*pformatetc) {
                S_OK
            } else {
                DV_E_FORMATETC
            }
        }
    }
    fn GetCanonicalFormatEtc(&self, pformatectin: *const FORMATETC, pformatetcout: *mut FORMATETC) -> HRESULT {
        unsafe {
            if pformatetcout.is_null() {
                return E_POINTER;
            }
            *pformatetcout = *pformatectin;
            (*pformatetcout).ptd = ptr::null_mut();
            S_FALSE // DATA_S_SAMEFORMATETC
        }
    }
    fn SetData(&self, _pformatetc: *const FORMATETC, _pmedium: *const STGMEDIUM, _frelease: BOOL) -> windows::core::Result<()> {
        Err(DV_E_FORMATETC.into())
    }
    fn EnumFormatEtc(&self, dwdirection: u32) -> windows::core::Result<IEnumFORMATETC> {
        if dwdirection != DATADIR_GET {
            return Err(DV_E_FORMATETC.into());
        }
        Ok(FmtEnum { index: UnsafeCell::new(0) }.into())
    }
    fn DAdvise(&self, _pformatetc: *const FORMATETC, _advf: u32, _padvsink: windows::core::Ref<windows::Win32::System::Com::IAdviseSink>) -> windows::core::Result<u32> {
        Err(OLE_E_ADVISENOTSUPPORTED.into())
    }
    fn DUnadvise(&self, _dwconnection: u32) -> windows::core::Result<()> {
        Err(OLE_E_ADVISENOTSUPPORTED.into())
    }
    fn EnumDAdvise(&self) -> windows::core::Result<windows::Win32::System::Com::IEnumSTATDATA> {
        Err(OLE_E_ADVISENOTSUPPORTED.into())
    }
}

// ---------------- IEnumFORMATETC 实现（只枚举 CF_HDROP 一项） ----------------

#[implement(IEnumFORMATETC)]
struct FmtEnum {
    index: UnsafeCell<u32>,
}

impl IEnumFORMATETC_Impl for FmtEnum_Impl {
    fn Next(&self, celt: u32, rgelt: *mut FORMATETC, pceltfetched: *mut u32) -> HRESULT {
        unsafe {
            if rgelt.is_null() {
                return E_POINTER;
            }
            let idx = *self.index.get();
            if idx == 0 && celt >= 1 {
                *rgelt = hdrop_format();
                *self.index.get() = 1;
                if !pceltfetched.is_null() {
                    *pceltfetched = 1;
                }
                return if celt == 1 { S_OK } else { S_FALSE };
            }
            if !pceltfetched.is_null() {
                *pceltfetched = 0;
            }
            S_FALSE
        }
    }
    fn Skip(&self, celt: u32) -> windows::core::Result<()> {
        unsafe {
            *self.index.get() += celt;
            Ok(())
        }
    }
    fn Reset(&self) -> windows::core::Result<()> {
        unsafe {
            *self.index.get() = 0;
            Ok(())
        }
    }
    fn Clone(&self) -> windows::core::Result<IEnumFORMATETC> {
        unsafe {
            Ok(FmtEnum {
                index: UnsafeCell::new(*self.index.get()),
            }
            .into())
        }
    }
}

// ---------------- 对外入口 ----------------

/// 启动一次模态的原生拖出（在调用线程进入 DoDragDrop 循环，直到玩家松开/取消）。
/// 返回最终生效的效果（"copy" / "move" / "none"）。
pub fn start_drag_out(paths: &[String]) -> Result<&'static str, String> {
    if paths.is_empty() {
        return Err("没有可拖出的文件".into());
    }
    unsafe {
        // DoDragDrop 要求线程已 OLE 初始化（STA）。IPC 处理线程可能尚未初始化：
        // RPC_E_CHANGED_MODE = 已是 MTA/其他模式，此时直接沿用现有模式（wry 主线程已 STA）。
        let co_hr = windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
        );
        let co_initialized = co_hr.is_ok();
        let result = start_drag_out_inner(paths);
        if co_initialized {
            windows::Win32::System::Com::CoUninitialize();
        }
        result
    }
}

unsafe fn start_drag_out_inner(paths: &[String]) -> Result<&'static str, String> {
    {
        let hdrop = create_hdrop(paths)?;
        let data_obj: IDataObject = FileDataObject { hdrop: UnsafeCell::new(hdrop) }.into();
        let source: IDropSource = DropSource.into();
        let mut effect = DROPEFFECT_NONE.0;
        let hr = do_drag_drop_raw(
            &data_obj,
            &source,
            DROPEFFECT_COPY.0,
            &mut effect,
        );
        // 数据对象自持的那份 hdrop 释放；接收方拿到的副本由其 ReleaseStgMedium 负责
        free_hdrop(hdrop);
        // 拖放完成后显式释放 COM 引用（Windows 会在事件循环里拿引用，这里只保证本地引用计数）
        drop(data_obj);
        drop(source);
        if hr.is_err() && hr != DRAGDROP_S_DROP && hr != DRAGDROP_S_CANCEL {
            return Err(format!("拖出被系统中断（0x{:08X}）", hr.0));
        }
        Ok(if effect & DROPEFFECT_COPY.0 != 0 {
            "copy"
        } else if effect & DROPEFFECT_MOVE.0 != 0 {
            "move"
        } else {
            "none"
        })
    }
}

// windows crate 未导出 DoDragDrop 的安全包装：直接链接 ole32
#[link(name = "ole32")]
unsafe extern "system" {
    fn DoDragDrop(
        pdataobj: *const core::ffi::c_void,
        pdropsource: *const core::ffi::c_void,
        dwokeffects: u32,
        pdweffect: *mut u32,
    ) -> HRESULT;
}

/// 包装：把接口转裸指针调用（DoDragDrop 保持引用，返回后我们再释放）
unsafe fn do_drag_drop_raw(
    data_obj: &IDataObject,
    source: &IDropSource,
    effects: u32,
    effect: &mut u32,
) -> HRESULT {
    DoDragDrop(
        windows::core::Interface::as_raw(data_obj),
        windows::core::Interface::as_raw(source),
        effects,
        effect,
    )
}

#[allow(dead_code)]
fn cursor_point() -> POINT {
    let mut p = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    p
}

#[allow(dead_code)]
pub fn foreground_hwnd() -> isize {
    unsafe { GetForegroundWindow().0 as isize }
}

/// 校验一批拖出路径：必须存在、数量 1–1000、去重（目录也允许）
pub fn validate_paths(paths: &[String]) -> Result<(), String> {
    if paths.is_empty() || paths.len() > 1000 {
        return Err("每次可拖出 1–1000 个文件，请分批选择".into());
    }
    let mut seen: Vec<String> = Vec::new();
    for p in paths {
        let path = Path::new(p);
        if !path.exists() {
            return Err(format!(
                "文件已不存在：{}",
                path.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| p.clone())
            ));
        }
        let canonical = path.to_string_lossy().to_lowercase();
        if seen.contains(&canonical) {
            continue;
        }
        seen.push(canonical);
    }
    Ok(())
}
