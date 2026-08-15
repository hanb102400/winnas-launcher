//! 提取应用图标（设计文档 4.11）：exe / lnk → PNG data URL（base64）
//!
//! 流程：`SHGetFileInfoW` 取 HICON → `GetIconInfo` 取位图 → `GetDIBits` 取 32 位 BGRA
//! → 转 RGBA → `image` crate 编码 PNG → base64 data URL。
//! 前端可直接用 `<img src=dataURL>` 展示，无需文件系统路径。

use base64::Engine;
use windows::core::{Interface, PCWSTR};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, SelectObject, BITMAP,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED, STGM_READ, IPersistFile,
};
use windows::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, IShellLinkW, ShellLink};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};

/// 解析图标源：`.lnk` 快捷方式用 IShellLink 解析目标文件路径（避免快捷方式箭头叠加）。
fn resolve_icon_source(path: &str) -> String {
    if !path.to_lowercase().ends_with(".lnk") {
        return path.to_string();
    }
    resolve_lnk_target(path).unwrap_or_else(|| path.to_string())
}

/// 用 IShellLink 解析 `.lnk` 指向的目标路径。
fn resolve_lnk_target(lnk_path: &str) -> Option<String> {
    unsafe {
        // STA 初始化（S_OK=本次初始化 / S_FALSE=线程已初始化）；RPC_E_CHANGED_MODE 等错误则放弃
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        if hr.is_err() {
            return None;
        }
        let shell_link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_ALL).ok()?;
        let persist: IPersistFile = shell_link.cast().ok()?;
        let wide: Vec<u16> = lnk_path.encode_utf16().chain(std::iter::once(0)).collect();
        persist.Load(PCWSTR::from_raw(wide.as_ptr()), STGM_READ).ok()?;
        let mut buf = [0u16; 1024];
        shell_link.GetPath(&mut buf, std::ptr::null_mut(), 0).ok()?;
        let len = buf.iter().position(|&c| c == 0).unwrap_or(0);
        if len > 0 {
            Some(String::from_utf16_lossy(&buf[..len]))
        } else {
            None
        }
    }
}

/// 提取图标为 PNG data URL（base64），失败返回 None（前端用默认图标）。
pub fn extract_icon_data_url(path: &str) -> Option<String> {
    // .lnk 先解析目标图标源（去掉快捷方式箭头）
    let src = resolve_icon_source(path);
    let (w, h, rgba) = extract_icon_rgba(&src)?;
    let img = image::RgbaImage::from_raw(w, h, rgba)?;
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png).ok()?;
    let png = buf.into_inner();
    let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
    Some(format!("data:image/png;base64,{b64}"))
}

/// 清理 SHGetFileInfoW / GetIconInfo 取得的图标资源（GDI 句柄）。
unsafe fn destroy_icon_resources(hicon: HICON, ii: &ICONINFO) {
    let _ = DestroyIcon(hicon);
    if !ii.hbmColor.0.is_null() {
        let _ = DeleteObject(HGDIOBJ(ii.hbmColor.0));
    }
    if !ii.hbmMask.0.is_null() {
        let _ = DeleteObject(HGDIOBJ(ii.hbmMask.0));
    }
}

fn extract_icon_rgba(path: &str) -> Option<(u32, u32, Vec<u8>)> {
    unsafe {
        // 1. SHGetFileInfoW 提取 HICON（.exe/.lnk 均可）
        let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        let mut sfi = SHFILEINFOW::default();
        let r = SHGetFileInfoW(
            PCWSTR::from_raw(wide.as_ptr()),
            Default::default(),
            Some(&mut sfi),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON,
        );
        if r == 0 || sfi.hIcon.0.is_null() {
            return None;
        }
        let hicon = sfi.hIcon;

        // 2. GetIconInfo 取位图（优先彩色，否则 mask）
        let mut ii = ICONINFO::default();
        if GetIconInfo(hicon, &mut ii).is_err() {
            destroy_icon_resources(hicon, &ii);
            return None;
        }
        let hbm = if !ii.hbmColor.0.is_null() {
            ii.hbmColor
        } else {
            ii.hbmMask
        };

        // 3. 取位图尺寸
        let mut bm = BITMAP::default();
        let got = GetObjectW(
            HGDIOBJ(hbm.0),
            std::mem::size_of::<BITMAP>() as i32,
            Some(&mut bm as *mut _ as *mut core::ffi::c_void),
        );
        if got == 0 {
            destroy_icon_resources(hicon, &ii);
            return None;
        }
        let w = bm.bmWidth as u32;
        let h = bm.bmHeight as u32;
        if w == 0 || h == 0 {
            destroy_icon_resources(hicon, &ii);
            return None;
        }

        // 4. GetDIBits 取 32 位 BGRA（负高度 = top-down）
        let mut bmi = BITMAPINFO::default();
        bmi.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: bm.bmWidth,
            biHeight: -bm.bmHeight,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        };
        let mut pixels = vec![0u8; (w * h * 4) as usize];
        let hdc = CreateCompatibleDC(None);
        let old = SelectObject(hdc, HGDIOBJ(hbm.0));
        let got = GetDIBits(
            hdc,
            hbm,
            0,
            h,
            Some(pixels.as_mut_ptr() as *mut core::ffi::c_void),
            &mut bmi,
            DIB_RGB_COLORS,
        );
        SelectObject(hdc, old);
        let _ = DeleteDC(hdc);
        if got == 0 {
            destroy_icon_resources(hicon, &ii);
            return None;
        }

        // 清理图标资源（hbmColor/hbmMask 由 GetIconInfo 分配，需单独 DeleteObject）
        destroy_icon_resources(hicon, &ii);

        // 5. BGRA → RGBA
        let mut rgba = vec![0u8; pixels.len()];
        for i in (0..pixels.len()).step_by(4) {
            rgba[i] = pixels[i + 2]; // R
            rgba[i + 1] = pixels[i + 1]; // G
            rgba[i + 2] = pixels[i]; // B
            rgba[i + 3] = pixels[i + 3]; // A
        }
        Some((w, h, rgba))
    }
}
