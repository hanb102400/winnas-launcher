//! 系统操作（设计文档 5.4 系统操作组）：关机 / 重启 / 睡眠 / 锁屏。
//!
//! 关机/重启用系统 `shutdown.exe`（无需提权）；睡眠用 `SetSuspendState`；锁屏用 `LockWorkStation`。

use windows::Win32::System::Power::SetSuspendState;
use windows::Win32::System::Shutdown::LockWorkStation;

/// 关机（立即）。
pub fn shutdown() {
    let _ = std::process::Command::new("shutdown")
        .args(["/s", "/t", "0"])
        .spawn();
}

/// 重启（立即）。
pub fn reboot() {
    let _ = std::process::Command::new("shutdown")
        .args(["/r", "/t", "0"])
        .spawn();
}

/// 睡眠（挂起到内存）。
pub fn sleep() {
    unsafe {
        SetSuspendState(false, false, false);
    }
}

/// 锁屏。
pub fn lock() {
    unsafe {
        let _ = LockWorkStation();
    }
}
