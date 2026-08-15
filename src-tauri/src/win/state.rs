//! 跨模块共享的原子状态（焦点状态机 AppRunning/Idle 的核心标志）。

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicUsize, Ordering};
use std::sync::OnceLock;

use tauri::AppHandle;

/// 是否有外部程序在运行（`AppRunning` = true，`Idle` = false）。
static APP_RUNNING: AtomicBool = AtomicBool::new(false);

/// 全局 AppHandle（供 keyhook 等后台线程 emit 事件到前端）。
static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();

pub fn set_app_handle(h: AppHandle) {
    let _ = APP_HANDLE.set(h);
}

pub fn app_handle() -> Option<&'static AppHandle> {
    APP_HANDLE.get()
}

/// 当前运行中的外部程序数量（用于多开场景的退出判定）。
static RUNNING_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Launcher 窗口 HWND（focus 夺焦目标）。
static LAUNCHER_HWND: AtomicIsize = AtomicIsize::new(0);

/// 维护模式标志（恢复任务栏/桌面，关闭置顶与夺焦，便于调试 Windows 桌面）。
static MAINTENANCE: AtomicBool = AtomicBool::new(false);

#[allow(dead_code)] // M3 焦点防护（on_foreground 主动夺焦）使用
pub fn app_running() -> bool {
    APP_RUNNING.load(Ordering::SeqCst)
}

pub fn set_app_running(v: bool) {
    APP_RUNNING.store(v, Ordering::SeqCst);
}

pub fn inc_running() -> usize {
    RUNNING_COUNT.fetch_add(1, Ordering::SeqCst) + 1
}

/// 递减运行计数，返回剩余数量。
pub fn dec_running() -> usize {
    RUNNING_COUNT.fetch_sub(1, Ordering::SeqCst) - 1
}

pub fn launcher_hwnd() -> isize {
    LAUNCHER_HWND.load(Ordering::SeqCst)
}

pub fn set_launcher_hwnd(h: isize) {
    LAUNCHER_HWND.store(h, Ordering::SeqCst);
}

pub fn maintenance() -> bool {
    MAINTENANCE.load(Ordering::SeqCst)
}

pub fn set_maintenance(v: bool) {
    MAINTENANCE.store(v, Ordering::SeqCst);
}
