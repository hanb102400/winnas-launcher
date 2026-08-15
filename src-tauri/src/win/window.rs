//! 窗口管理：置顶（设计文档 4.2 修订）
//!
//! 全屏用 tauri `fullscreen: true`（内部是正确原生 `Fullscreen::Borderless` 全屏，见 4.2）。
//! 置顶是独立操作（全屏不改变 Z 顺序），用原生 `SetWindowPos(HWND_TOPMOST)`。
//!
//! **置顶跟随焦点**（4.8 状态机配套）：Launcher 聚焦时置顶；失焦（用户 Alt+Tab 切到其他程序、
//! 或启动了外部程序）时取消置顶，让其他程序能显示在最前。由 `focus.rs` 的前台监听驱动。

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    SetWindowPos, HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_SHOWWINDOW,
};

/// 置顶（不改尺寸/位置，`SWP_NOACTIVATE` 不抢焦点）。
pub fn topmost(hwnd: HWND) {
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW | SWP_NOACTIVATE,
        );
    }
}

/// 取消置顶（失焦时调用，让其他程序能显示在最前）。
pub fn not_topmost(hwnd: HWND) {
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_NOTOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}
