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

/// 睡眠（挂起到内存，S3）。
///
/// `SetSuspendState(false, false, false)`：
/// - 参数1 `bHibernate=false` → 挂起到内存（S3，非休眠 S4）
/// - 参数2 `bForce=false` → 不强制（让系统正常处理）
/// - 参数3 `bWakeupEventsDisabled=false` → **允许唤醒事件**（遥控器/键盘/鼠标可唤醒）
pub fn sleep() -> bool {
    unsafe {
        let r = SetSuspendState(false, false, false);
        if !r {
            super::log::info("system", "睡眠失败（SetSuspendState 返回 false）");
        }
        r
    }
}

/// 锁屏。
pub fn lock() {
    unsafe {
        let _ = LockWorkStation();
    }
}
