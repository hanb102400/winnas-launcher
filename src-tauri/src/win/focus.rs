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
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    keybd_event, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, VK_MENU,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, PeekMessageW, SetForegroundWindow, ShowWindow,
    SystemParametersInfoW, MSG, EVENT_SYSTEM_FOREGROUND, PM_NOREMOVE,
    SPI_GETFOREGROUNDLOCKTIMEOUT, SPI_SETFOREGROUNDLOCKTIMEOUT, SPIF_SENDCHANGE, SW_RESTORE,
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
    // AppRunning 态（已启动外部程序）例外：即使 Launcher 拿到前台也不置顶 ——
    // Playnite 等闪屏关闭瞬间前台会短暂回 Launcher，若此时置顶会把刚出现的主窗口压到下面，
    // 造成「外部程序无法显示在最前」。外部程序全部退出回到 Idle 后才恢复置顶。
    let launcher = state::launcher_hwnd();
    if launcher != 0 {
        let lh = HWND(launcher as *mut _);
        if hwnd == lh {
            if !state::app_running() {
                window::topmost(lh);
            }
        } else {
            window::not_topmost(lh);
        }
    }
}

/// 初始化：保存 Launcher 句柄 + 放宽前台锁定 + 监听前台变化。
pub fn init(hwnd: HWND) {
    state::set_launcher_hwnd(hwnd.0 as isize);
    // 初始前台视为 Launcher（窗口显示后由 on_foreground 纠正）
    FOREGROUND_HWND.store(hwnd.0 as isize, Ordering::SeqCst);
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
    // 启动焦点看门狗：Idle 态兜底夺回被其它程序抢占的前台
    start_watchdog();
}

/// 延时夺焦（外部程序退出后调用；300~800ms 避开大型程序销毁延迟，见 4.8）。
pub fn schedule_reclaim() {
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_millis(500));
        reclaim();
    });
}

/// 取 Launcher 窗口句柄（未初始化返回 None）。
fn launcher_hwnd() -> Option<HWND> {
    let h = state::launcher_hwnd();
    if h == 0 {
        None
    } else {
        Some(HWND(h as *mut _))
    }
}

/// 把 Launcher 带到最前并激活：恢复显示 + 置顶 + 绕过前台锁定。
///
/// Windows 对 `SetForegroundWindow` 有权限限制（本进程需「接收过最近输入 / 挂接前台线程
/// 输入队列 / 前台锁定超时清零」之一），且前台窗口切换/忙碌时单次调用可能失败。
/// 这里用「确保线程消息队列 + 挂接前台线程 + 验证 + 模拟 Alt 输入 + 重试」组合提高成功率：
/// - `PeekMessageW(PM_NOREMOVE)` 强制创建本线程消息队列（spawn 线程队列是惰性创建，
///   无队列时 `AttachThreadInput` 会失败——Home 键夺焦「时好时坏」的根因）；
/// - 模拟一次 Alt 键按下/抬起，让系统认为本进程接收过用户输入，从而放行 `SetForegroundWindow`。
fn bring_to_front(hwnd: HWND) {
    unsafe {
        let _ = ShowWindow(hwnd, SW_RESTORE);
        window::topmost(hwnd);
        // 前置：前台锁定超时清零（启动时已设，但其他程序可能改回，夺焦前重申）
        let _ = SystemParametersInfoW(SPI_SETFOREGROUNDLOCKTIMEOUT, 0, None, SPIF_SENDCHANGE);

        // 确保本线程有消息队列（无队列时 AttachThreadInput 失败）
        let mut msg = MSG::default();
        let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);

        for _ in 0..3 {
            let fg_thread = GetWindowThreadProcessId(GetForegroundWindow(), None);
            let our_thread = GetCurrentThreadId();
            let _ = AttachThreadInput(our_thread, fg_thread, true);
            let _ = SetForegroundWindow(hwnd);
            let _ = AttachThreadInput(our_thread, fg_thread, false);

            if GetForegroundWindow() == hwnd {
                return; // 已拿到前台
            }
            // 兜底：模拟 Alt 键按下/抬起，令系统认为本进程接收过用户输入后重试
            keybd_event(VK_MENU.0 as u8, 0, KEYEVENTF_EXTENDEDKEY, 0);
            keybd_event(VK_MENU.0 as u8, 0, KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP, 0);
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

/// 主动夺回焦点到 Launcher（外部程序退出后调用）。
fn reclaim() {
    // 维护模式：不夺焦（让用户停留在桌面/其他程序）
    if state::maintenance() {
        return;
    }
    let Some(hwnd) = launcher_hwnd() else {
        return;
    };
    super::log::info("focus", "夺焦恢复 Launcher");
    bring_to_front(hwnd);
}

/// Home 键唤起 Launcher（FR-19）：显式用户动作，覆盖 `AppRunning` 态。
pub fn show_launcher() {
    // 维护模式：不打扰用户调试桌面（退出维护模式走「长按返回+菜单」）
    if state::maintenance() {
        return;
    }
    let Some(hwnd) = launcher_hwnd() else {
        return;
    };
    super::log::info("focus", "Home 键唤起 Launcher");
    bring_to_front(hwnd);
}

/// 焦点看门狗（4.8 兜底）：Idle 态下低频检查前台窗口，
/// 若前台被其他程序抢占（开机自启时的其它程序/弹窗等）则静默夺回。
pub fn start_watchdog() {
    std::thread::spawn(|| loop {
        std::thread::sleep(Duration::from_millis(1000));
        // AppRunning 态不干扰外部程序；维护模式不夺
        if state::app_running() || state::maintenance() {
            continue;
        }
        let Some(launcher) = launcher_hwnd() else {
            continue;
        };
        let fg = FOREGROUND_HWND.load(Ordering::SeqCst);
        if fg == launcher.0 as isize {
            continue; // 已在前台
        }
        super::log::info("focus", "看门狗夺回焦点");
        bring_to_front(launcher);
    });
}

/// 还原前台锁定超时到启动前的原始值（由 lib.rs 的 RunEvent::Exit 调用）。
pub fn restore_foreground_lock_timeout() {
    let orig = ORIG_FG_LOCK_TIMEOUT.load(Ordering::SeqCst);
    unsafe {
        let _ = SystemParametersInfoW(SPI_SETFOREGROUNDLOCKTIMEOUT, orig, None, SPIF_SENDCHANGE);
    }
}
