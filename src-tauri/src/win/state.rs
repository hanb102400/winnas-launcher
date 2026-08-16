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

/// 单实例锁：命名互斥量。返回 `false` 表示已有另一实例在运行（调用方应直接退出）。
///
/// 句柄有意泄漏：进程存活期间保持互斥量，进程退出时由 OS 回收。
pub fn acquire_single_instance() -> bool {
    unsafe {
        let h = match windows::Win32::System::Threading::CreateMutexW(
            None,
            true,
            windows::core::w!("WinNasLauncher_SingleInstance_Mutex"),
        ) {
            Ok(h) => h,
            // 创建失败则放行（避免误判阻止启动）
            Err(_) => return true,
        };
        let already = windows::Win32::Foundation::GetLastError()
            == windows::Win32::Foundation::ERROR_ALREADY_EXISTS;
        if already {
            let _ = windows::Win32::Foundation::CloseHandle(h);
            return false;
        }
        // HANDLE 为 Copy 且无 Drop，句柄在函数返回后仍保持打开（进程退出时由 OS 回收）
        let _ = h;
        true
    }
}
