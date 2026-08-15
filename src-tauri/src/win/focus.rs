//! 抢焦点防护 / 焦点状态机（设计文档 4.8）
//!
//! 状态：Idle（无外部程序）↔ AppRunning（有外部程序运行）。
//! 已由 M0 PoC3 验证：`SPI_SETFOREGROUNDLOCKTIMEOUT=0`、AppRunning 让焦、Idle 夺焦。
//!
//! M2 实现核心闭环：程序退出（process.rs 的 Job 检测）→ `schedule_reclaim` 延时夺焦。
//! 防系统弹窗/通知抢焦的主动夺回（`AttachThreadInput` + 白名单）在 M3 完善。

use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};
use std::time::Duration;

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::{
    SetForegroundWindow, SystemParametersInfoW, EVENT_SYSTEM_FOREGROUND,
    SPI_GETFOREGROUNDLOCKTIMEOUT, SPI_SETFOREGROUNDLOCKTIMEOUT, SPIF_SENDCHANGE,
    WINEVENT_OUTOFCONTEXT,
};

use super::{state, window};

/// 最近一次前台窗口句柄（`EVENT_SYSTEM_FOREGROUND` 回调记录；权威信号，
/// 进程强杀后 `GetForegroundWindow` 有短暂失效期，见 PoC3 结论）。
static FOREGROUND_HWND: AtomicIsize = AtomicIsize::new(0);

/// 启动前的原始前台锁定超时（毫秒），退出时还原该全局用户参数。
static ORIG_FG_LOCK_TIMEOUT: AtomicU32 = AtomicU32::new(200_000);

unsafe extern "system" fn on_foreground(
    _hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    _id_obj: i32,
    _id_child: i32,
    _id_thread: u32,
    _time: u32,
) {
    // 轻量记录（WINEVENT_OUTOFCONTEXT 回调，尽快返回）
    FOREGROUND_HWND.store(hwnd.0 as isize, Ordering::SeqCst);

    // 维护模式：不管理置顶（恢复桌面，让用户自由调试）
    if state::maintenance() {
        return;
    }

    // 置顶跟随焦点：Launcher 聚焦 → 置顶；失焦 → 取消置顶（让其他程序显示在最前）
    let launcher = state::launcher_hwnd();
    if launcher != 0 {
        let lh = HWND(launcher as *mut _);
        if hwnd == lh {
            window::topmost(lh);
        } else {
            window::not_topmost(lh);
        }
    }
}

/// 初始化：保存 Launcher 句柄 + 放宽前台锁定 + 监听前台变化。
pub fn init(hwnd: HWND) {
    state::set_launcher_hwnd(hwnd.0 as isize);
    unsafe {
        // 记录原始前台锁定超时（全局用户参数），退出时在 restore_foreground_lock_timeout 还原
        let mut orig: u32 = 0;
        if SystemParametersInfoW(
            SPI_GETFOREGROUNDLOCKTIMEOUT,
            0,
            Some(&mut orig as *mut u32 as *mut core::ffi::c_void),
            Default::default(),
        )
        .is_ok()
        {
            ORIG_FG_LOCK_TIMEOUT.store(orig, Ordering::SeqCst);
        }
        // SPI_SETFOREGROUNDLOCKTIMEOUT：新值在 uiParam，pvParam 必须为 NULL（PoC3 已修正）
        let _ = SystemParametersInfoW(SPI_SETFOREGROUNDLOCKTIMEOUT, 0, None, SPIF_SENDCHANGE);
        // 钩子泄漏到进程结束（系统自动清理），不显式卸载
        let _hook = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(on_foreground),
            0,
            0,
            WINEVENT_OUTOFCONTEXT,
        );
    }
}

/// 延时夺焦（外部程序退出后调用；300~800ms 避开大型程序销毁延迟，见 4.8）。
pub fn schedule_reclaim() {
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_millis(500));
        reclaim();
    });
}

/// 主动夺回焦点到 Launcher（恢复显示 + 置顶 + 激活）。
fn reclaim() {
    // 维护模式：不夺焦（让用户停留在桌面/其他程序）
    if state::maintenance() {
        return;
    }
    let h = state::launcher_hwnd();
    if h == 0 {
        return;
    }
    let hwnd = HWND(h as *mut _);
    super::log::info("focus", "夺焦恢复 Launcher");
    unsafe {
        // 置顶 + 激活（Launcher 未隐藏，直接恢复最前）
        window::topmost(hwnd);
        let _ = SetForegroundWindow(hwnd);
    }
}

/// 还原前台锁定超时到启动前的原始值（由 lib.rs 的 RunEvent::Exit 调用）。
pub fn restore_foreground_lock_timeout() {
    let orig = ORIG_FG_LOCK_TIMEOUT.load(Ordering::SeqCst);
    unsafe {
        let _ = SystemParametersInfoW(SPI_SETFOREGROUNDLOCKTIMEOUT, orig, None, SPIF_SENDCHANGE);
    }
}
