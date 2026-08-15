// PoC3: 焦点状态机 Idle <-> AppRunning
//
// 验证目标:
//   1. SetWinEventHook(EVENT_SYSTEM_FOREGROUND) 监听前台窗口变化
//   2. AppRunning: 外部应用(notepad)在前台时, Launcher 不夺焦 (让焦给用户正在用的程序)
//   3. Idle: 外部应用退出后, Launcher 主动 SetForegroundWindow 夺焦
//   4. SPI_SETFOREGROUNDLOCKTIMEOUT=0 降低夺焦被系统拒绝的概率
//   5. 记录已知限制: 全屏独占游戏会阻止 SetForegroundWindow (OS 级, 无法绕过)
//
// 使用: 运行后自动演示 (自动拉起 notepad 并退出), 全程 ~8s。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use windows::core::w;
use windows::Win32::Foundation::{CloseHandle, HINSTANCE, LPARAM, LRESULT, WPARAM, HWND};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{
    CreateProcessW, TerminateProcess, WaitForSingleObject, STARTUPINFOW, PROCESS_INFORMATION,
};
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetClassNameW, GetForegroundWindow,
    PeekMessageW, RegisterClassW, SetForegroundWindow, ShowWindow, SystemParametersInfoW,
    TranslateMessage, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, MSG, PM_REMOVE,
    SPI_GETFOREGROUNDLOCKTIMEOUT, SPI_SETFOREGROUNDLOCKTIMEOUT, SPIF_SENDCHANGE, SW_SHOW,
    WNDCLASSW, WINDOW_EX_STYLE, WS_OVERLAPPEDWINDOW, EVENT_SYSTEM_FOREGROUND,
    WINEVENT_OUTOFCONTEXT,
};
use windows::Win32::Graphics::Gdi::UpdateWindow;

const LAUNCHER_CLASS: windows::core::PCWSTR = w!("WinnasPoc3Launcher");

// 记录最近一次 EVENT_SYSTEM_FOREGROUND 事件的前台窗口句柄。
// 进程被强杀后 GetForegroundWindow 可能短暂返回失效句柄, WinEvent 才是权威信号。
static LAST_FG: AtomicUsize = AtomicUsize::new(0);

unsafe extern "system" fn wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

unsafe extern "system" fn on_foreground(
    _hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    _id_obj: i32,
    _id_child: i32,
    _id_thread: u32,
    _time: u32,
) {
    // WINEVENT_OUTOFCONTEXT 回调: 只做轻量记录, 尽快返回
    let mut cls = [0u16; 128];
    let n = GetClassNameW(hwnd, &mut cls);
    let name = if n > 0 {
        String::from_utf16_lossy(&cls[..n as usize])
    } else {
        "<none>".to_string()
    };
    LAST_FG.store(hwnd.0 as usize, Ordering::Relaxed);
    println!("[event] FOREGROUND -> hwnd={hwnd:?} class=\"{name}\"");
}

fn sleep(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

fn pump(ms: u64) {
    // 泵消息并保持窗口响应, 持续 ms 毫秒
    let deadline = std::time::Instant::now() + Duration::from_millis(ms);
    let mut msg = MSG::default();
    loop {
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                _ = TranslateMessage(&msg);
                _ = DispatchMessageW(&msg);
            }
        }
        if std::time::Instant::now() >= deadline {
            break;
        }
        sleep(10);
    }
}

fn main() -> windows::core::Result<()> {
    println!("[poc3] focus state machine Idle <-> AppRunning");
    unsafe {
        // 0. 降低前台锁定超时, 提高夺焦成功率
        //    SPI_SETFOREGROUNDLOCKTIMEOUT: 新值在 uiParam, pvParam 必须为 NULL
        let _ = SystemParametersInfoW(SPI_SETFOREGROUNDLOCKTIMEOUT, 0, None, SPIF_SENDCHANGE);
        let mut lock_timeout: u32 = 0;
        let _ = SystemParametersInfoW(
            SPI_GETFOREGROUNDLOCKTIMEOUT,
            0,
            Some(&mut lock_timeout as *mut u32 as *mut core::ffi::c_void),
            Default::default(),
        );
        println!("[poc3] foreground lock timeout = {lock_timeout} ms (0 = allow immediate)");

        // 1. 注册 Launcher 窗口
        let hinst = HINSTANCE(GetModuleHandleW(None)?.0);
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: hinst,
            lpszClassName: LAUNCHER_CLASS,
            ..Default::default()
        };
        RegisterClassW(&wc);
        let launcher = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            LAUNCHER_CLASS,
            w!("PoC3 Launcher (simulates WinNas)"),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            500,
            320,
            None,
            None,
            Some(hinst),
            None,
        )?;
        let _ = ShowWindow(launcher, SW_SHOW);
        let _ = UpdateWindow(launcher);
        println!("[poc3] launcher window created: {launcher:?}");

        // 2. 监听前台变化
        let hook = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(on_foreground),
            0,
            0,
            WINEVENT_OUTOFCONTEXT,
        );
        println!("[poc3] win event hook installed: {hook:?}");

        // 3. STEP1: Launcher 获得前台 (Idle)
        let _ = SetForegroundWindow(launcher);
        pump(800);
        let fg = GetForegroundWindow();
        println!("[step1] fg == launcher ? {} ({fg:?})", fg == launcher);

        // 4. STEP2: 启动 notepad 模拟外部应用 -> AppRunning
        let mut cmd: Vec<u16> = "notepad.exe".encode_utf16().chain(std::iter::once(0)).collect();
        let mut si = STARTUPINFOW::default();
        si.cb = core::mem::size_of::<STARTUPINFOW>() as u32;
        let mut pi = PROCESS_INFORMATION::default();
        CreateProcessW(
            windows::core::PCWSTR::null(),
            Some(windows::core::PWSTR(cmd.as_mut_ptr())),
            None,
            None,
            false,
            Default::default(),
            None,
            windows::core::PCWSTR::null(),
            &si,
            &mut pi,
        )?;
        println!("[step2] notepad spawned (pid={})", pi.dwProcessId);
        pump(1500);
        let fg2 = GetForegroundWindow();
        println!(
            "[step2] AppRunning: fg should be notepad, fg={fg2:?} (expect != launcher)",
        );

        // 5. STEP3: 再等 2s, 确认 Launcher 没有夺焦 (状态机让焦)
        pump(2000);
        let fg3 = GetForegroundWindow();
        println!(
            "[step3] after 2s fg={fg3:?}, launcher did NOT steal focus ? {}",
            fg3 != launcher
        );

        // 6. STEP4: 关闭 notepad -> 回到 Idle
        let _ = TerminateProcess(pi.hProcess, 0);
        WaitForSingleObject(pi.hProcess, 3000);
        CloseHandle(pi.hThread)?;
        CloseHandle(pi.hProcess)?;
        pump(600);
        let fg4 = GetForegroundWindow();
        println!("[step4] notepad exited, fg={fg4:?}");

        // 7. STEP5: Idle 夺焦
        //    验证信号: WinEvent(LAST_FG) 为权威; GetForegroundWindow 可能返回失效句柄
        let mut ok = false;
        for _ in 0..5 {
            let _ = SetForegroundWindow(launcher);
            pump(300);
            let by_fg = GetForegroundWindow() == launcher;
            let by_event = LAST_FG.load(Ordering::Relaxed) == launcher.0 as usize;
            if by_fg || by_event {
                ok = true;
                break;
            }
        }
        let fg5 = GetForegroundWindow();
        let ev5 = LAST_FG.load(Ordering::Relaxed);
        println!(
            "[step5] Idle reclaim focus: {ok} (GetForegroundWindow={fg5:?}, WinEvent={ev5:#010x})"
        );
        println!("[poc3] note: fullscreen-exclusive games block SetForegroundWindow at OS level; product falls back to 'prompt user to go back'.");

        let _ = UnhookWinEvent(hook);
        println!("[poc3] done");
        Ok(())
    }
}
