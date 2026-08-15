//! 隐藏任务栏（设计文档 4.3）——非持久化
//!
//! 关键约束（评审修订）：**禁止用 `ABS_AUTOHIDE`**（全局持久设置，崩溃/强杀会遗留"任务栏永久自动隐藏"）。
//! 这里用 `ShowWindow(SW_HIDE)` 临时隐藏，仅窗口态、不写任何持久配置；配合 4.13 状态快照兜底。

use windows::core::w;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, ShowWindow, SW_HIDE, SW_SHOW};

fn taskbar_hwnd() -> Option<HWND> {
    let hwnd = unsafe { FindWindowW(w!("Shell_TrayWnd"), None).unwrap_or_default() };
    if hwnd.0.is_null() {
        None
    } else {
        Some(hwnd)
    }
}

/// 隐藏任务栏（临时，非持久化）。
pub fn hide() {
    if let Some(hwnd) = taskbar_hwnd() {
        unsafe {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
    }
}

/// 恢复任务栏显示。
pub fn restore() {
    if let Some(hwnd) = taskbar_hwnd() {
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOW);
        }
    }
}
