//! 提取应用图标（设计文档 4.11）：exe / lnk → PNG data URL（base64）
//!
//! 流程：`SHGetFileInfoW` 取 HICON → `GetIconInfo` 取位图 → `GetDIBits` 取 32 位 BGRA
//! → 转 RGBA → `image` crate 编码 PNG → base64 data URL。
//! 前端可直接用 `<img src=dataURL>` 展示，无需文件系统路径。

use base64::Engine;
use windows::core::PCWSTR;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, SelectObject, BITMAP,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
};
use windows::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, ICONINFO};

/// 提取图标为 PNG data URL（base64），失败返回 None（前端用默认图标）。
pub fn extract_icon_data_url(path: &str) -> Option<String> {
    let (w, h, rgba) = extract_icon_rgba(path)?;
    let img = image::RgbaImage::from_raw(w, h, rgba)?;
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png).ok()?;
    let png = buf.into_inner();
    let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
    Some(format!("data:image/png;base64,{b64}"))
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
        GetIconInfo(hicon, &mut ii).ok()?;
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
            return None;
        }
        let w = bm.bmWidth as u32;
        let h = bm.bmHeight as u32;
        if w == 0 || h == 0 {
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
            return None;
        }

        // 清理
        let _ = DestroyIcon(hicon);
        if !ii.hbmColor.0.is_null() {
            let _ = DeleteObject(HGDIOBJ(ii.hbmColor.0));
        }
        if !ii.hbmMask.0.is_null() {
            let _ = DeleteObject(HGDIOBJ(ii.hbmMask.0));
        }

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
