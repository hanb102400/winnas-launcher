//! 启动外部程序 + 进程树退出检测（设计文档 4.9）
//!
//! 每程序一个 Job Object（`KILL_ON_JOB_CLOSE`），进程树全部退出（active=0）时 Job 句柄 signaled，
//! 天然覆盖"父进程退出、子进程存活"场景（Steam / 模拟器 / 启动器）。已由 M0 PoC4 验证。
//!
//! 退出后递减运行计数；归零则 `AppRunning=false` 并触发 focus 延时夺焦（4.8）。

use std::time::Duration;

use windows::core::{w, BOOL, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, RECT, WAIT_OBJECT_0};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_BASIC_LIMIT_INFORMATION,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, Thread32First, Thread32Next,
    PROCESSENTRY32W, TH32CS_SNAPPROCESS, TH32CS_SNAPTHREAD, THREADENTRY32,
};
use windows::Win32::System::Threading::{
    AttachThreadInput, CreateProcessW, GetCurrentThreadId, GetProcessId, OpenProcess,
    QueryFullProcessImageNameW, WaitForSingleObject, PROCESS_INFORMATION, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, STARTUPINFOW,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    keybd_event, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, VK_MENU,
};
use windows::Win32::UI::Shell::{ShellExecuteExW, SHELLEXECUTEINFOW, SEE_MASK_NOCLOSEPROCESS};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, EnumThreadWindows, EnumWindows, GetForegroundWindow, GetParent, GetWindowRect,
    GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
    SetForegroundWindow, SetWindowPos, ShowWindow, HWND_MESSAGE, HWND_NOTOPMOST, HWND_TOPMOST,
    SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SW_RESTORE, SW_SHOWNORMAL,
};

use super::{focus, state, window};

/// 枚举窗口回调的线程局部上下文（通过 lParam 传入，避免全局静态的并发竞态）。
/// 收集进程所有可见顶层窗口，按「主窗口评分」取最大（见 window_score），
/// 从而跳过闪屏选中真正的主窗口（Playnite / Steam 等先弹闪屏再开主窗口）。
///
/// 匹配范围 = 启动的 pid **∪** 同 exe 文件名的进程（启动器「父进程退出、UI 子进程接管」
/// 时，窗口不在启动的 pid 下，需按文件名把子进程的窗口也算进来；`exe_name` 空串则仅按 pid）。
struct FindWindowState {
    pid: u32,
    /// 小写 exe 文件名；空串表示仅按 pid 匹配（.lnk 非 exe 目标走 Shell，无确定文件名）。
    exe_name: String,
    best: HWND,
    best_score: u64,
}

/// 「主窗口」评分：面积越大越像主窗口；标题非空强烈加分（闪屏通常小且无标题）。
unsafe fn window_score(hwnd: HWND) -> u64 {
    let mut rect = RECT::default();
    if GetWindowRect(hwnd, &mut rect).is_err() {
        return 0;
    }
    let w = (rect.right - rect.left).max(0) as u64;
    let h = (rect.bottom - rect.top).max(0) as u64;
    if w == 0 || h == 0 {
        return 0;
    }
    let area = w * h;
    if GetWindowTextLengthW(hwnd) > 0 {
        area + 10_000_000
    } else {
        area
    }
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let state = &mut *(lparam.0 as *mut FindWindowState);
    if !IsWindowVisible(hwnd).as_bool() {
        return BOOL(1); // 只考虑可见窗口
    }
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    let own_pid = pid == state.pid;
    // 非启动 pid：按 exe 文件名匹配（覆盖「父退出、子进程开窗」场景）。路径查询较贵，仅在不匹配时才做。
    let own_exe = if own_pid || state.exe_name.is_empty() {
        false
    } else {
        process_full_path(pid)
            .map(|p| {
                std::path::Path::new(&p)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_lowercase() == state.exe_name)
                    .unwrap_or(false)
            })
            .unwrap_or(false)
    };
    if own_pid || own_exe {
        let score = window_score(hwnd);
        if score > state.best_score {
            state.best_score = score;
            state.best = hwnd;
        }
    }
    BOOL(1) // 继续枚举全部，取评分最高的
}

/// 找进程的「主窗口」：匹配进程（启动 pid ∪ 同 exe 名）的全部可见顶层窗口中评分最高的。
fn find_main_window(pid: u32, exe_name: &str) -> Option<HWND> {
    unsafe {
        let mut state = FindWindowState {
            pid,
            exe_name: exe_name.to_lowercase(),
            best: HWND(std::ptr::null_mut()),
            best_score: 0,
        };
        let _ = EnumWindows(
            Some(enum_proc),
            LPARAM(&mut state as *mut FindWindowState as isize),
        )
        .ok();
        if state.best.0.is_null() {
            None
        } else {
            Some(state.best)
        }
    }
}

/// 取路径的文件名并转小写（窗口按 exe 名匹配用；空路径返回空串）。
fn exe_file_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// 指定 pid 的进程是否存活（受保护进程访问被拒也视为存活，避免误判）。
fn process_exists(pid: u32) -> bool {
    unsafe {
        match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(h) => {
                let _ = CloseHandle(h);
                true
            }
            Err(_) => false,
        }
    }
}

/// 深挖诊断的枚举上下文：收集相关 pid 集合的全部窗口（顶层含不可见 + 线程窗口含托盘消息窗口）。
/// `cur_pid` 为外层枚举时的当前进程，仅用于行内标注。
struct DiagState {
    pids: Vec<u32>,
    cur_pid: u32,
    rows: Vec<String>,
}

unsafe extern "system" fn diag_enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let s = &mut *(lparam.0 as *mut DiagState);
    let mut wp = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut wp));
    if s.pids.contains(&wp) {
        let title = window_title(hwnd);
        let vis = IsWindowVisible(hwnd).as_bool();
        s.rows.push(format!("pid={wp} 可见={vis} 标题=\"{title}\""));
    }
    BOOL(1)
}

/// 线程窗口回调：`EnumThreadWindows` 能枚举到「消息专用窗口」（父窗口 = `HWND_MESSAGE`），
/// 最小化到托盘的 app 常用这种窗口承载托盘图标回调——顶层枚举（`EnumWindows`）看不到它们。
unsafe extern "system" fn diag_thread_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let s = &mut *(lparam.0 as *mut DiagState);
    let msg_only = GetParent(hwnd).ok() == Some(HWND_MESSAGE);
    let title = window_title(hwnd);
    let vis = IsWindowVisible(hwnd).as_bool();
    s.rows.push(format!(
        "pid={} 消息专用={msg_only} 可见={vis} 标题=\"{title}\"",
        s.cur_pid
    ));
    BOOL(1)
}

/// 全桌面枚举上下文：收集所有可见且有标题的顶层窗口（pid + exe 文件名），
/// 用于定位「UI 到底在哪个进程」（启动的 pid 名下找不到时，UI 可能被别的进程承载）。
struct DesktopDiagState {
    rows: Vec<String>,
}

unsafe extern "system" fn diag_desktop_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let s = &mut *(lparam.0 as *mut DesktopDiagState);
    if !IsWindowVisible(hwnd).as_bool() {
        return BOOL(1);
    }
    let title = window_title(hwnd);
    if title.is_empty() {
        return BOOL(1); // 只记有标题的，过滤掉标题栏为空的工具窗口
    }
    let mut wp = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut wp));
    let exe = process_full_path(wp)
        .map(|p| {
            std::path::Path::new(&p)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default()
        })
        .unwrap_or_default();
    s.rows.push(format!("pid={wp} exe={exe} 标题=\"{title}\""));
    BOOL(1)
}

/// 诊断（轮询一直找不到可见主窗口时调用）：区分三种情况——
/// 1. 顶层窗口有但 `可见=false` → 应用启动最小化/隐藏（未销毁窗口）；
/// 2. 无顶层窗口但**有线程消息专用窗口**（托盘图标回调窗口）→ **最小化到托盘**，激活无济于事，需改应用设置；
/// 3. 顶层窗口与线程窗口都**没有** → 「父进程退出、UI 子进程接管」或应用启动卡死（连托盘都没进）。
///
/// 相关进程 = 启动的 pid ∪ 同 exe 文件名的所有进程（Toolhelp 进程快照，子进程同名也能捞到）。
fn log_launch_diagnostic(pid: u32, exe_name: &str) {
    let mut pids: Vec<u32> = vec![pid];
    if !exe_name.is_empty() {
        unsafe {
            if let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
                let mut entry = PROCESSENTRY32W::default();
                entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
                if Process32FirstW(snapshot, &mut entry).is_ok() {
                    loop {
                        let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(0);
                        let name = String::from_utf16_lossy(&entry.szExeFile[..len]).to_lowercase();
                        if name == exe_name && !pids.contains(&entry.th32ProcessID) {
                            pids.push(entry.th32ProcessID);
                        }
                        if Process32NextW(snapshot, &mut entry).is_err() {
                            break;
                        }
                    }
                }
                let _ = CloseHandle(snapshot);
            }
        }
    }
    let alive = process_exists(pid);
    super::log::info(
        "launch",
        &format!("诊断 目标pid={pid} 存活={alive} 同exe进程集={pids:?} exe={exe_name}"),
    );
    unsafe {
        // ① 顶层窗口（含不可见）
        let mut s = DiagState {
            pids: pids.clone(),
            cur_pid: 0,
            rows: Vec::new(),
        };
        let _ = EnumWindows(Some(diag_enum_proc), LPARAM(&mut s as *mut DiagState as isize)).ok();
        if s.rows.is_empty() {
            super::log::info("launch", "诊断 无任何顶层窗口");
        } else {
            for r in &s.rows {
                super::log::info("launch", &format!("诊断 顶层窗口: {r}"));
            }
        }
        // ② 线程窗口（含消息专用窗口，识别最小化到托盘）
        let mut t = DiagState {
            pids: pids.clone(),
            cur_pid: 0,
            rows: Vec::new(),
        };
        if let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) {
            let mut te = THREADENTRY32::default();
            te.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
            if Thread32First(snapshot, &mut te).is_ok() {
                loop {
                    if pids.contains(&te.th32OwnerProcessID) {
                        t.cur_pid = te.th32OwnerProcessID;
                        let _ = EnumThreadWindows(
                            te.th32ThreadID,
                            Some(diag_thread_proc),
                            LPARAM(&mut t as *mut DiagState as isize),
                        )
                        .ok();
                    }
                    if Thread32Next(snapshot, &mut te).is_err() {
                        break;
                    }
                }
            }
            let _ = CloseHandle(snapshot);
        }
        if t.rows.is_empty() {
            super::log::info("launch", "诊断 亦无任何线程窗口（非托盘，疑启动卡死/子进程）");
        } else {
            for r in &t.rows {
                super::log::info("launch", &format!("诊断 线程窗口: {r}"));
            }
        }
        // ③ 全桌面可见窗口（定位 UI 实际承载进程，可能不在目标 exe 名下）
        let mut d = DesktopDiagState { rows: Vec::new() };
        let _ = EnumWindows(
            Some(diag_desktop_proc),
            LPARAM(&mut d as *mut DesktopDiagState as isize),
        )
        .ok();
        super::log::info(
            "launch",
            &format!("诊断 桌面可见窗口 {} 个:", d.rows.len()),
        );
        for r in d.rows.iter().take(40) {
            super::log::info("launch", &format!("诊断   {r}"));
        }
        // ④ playnite 家族进程（名字含 exe_name 的词干，如 "playnite" 覆盖 DesktopApp/FullscreenApp/Updater）
        let stem = exe_name.split('.').next().unwrap_or("").to_string();
        if !stem.is_empty() {
            let mut fam: Vec<String> = Vec::new();
            if let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
                let mut entry = PROCESSENTRY32W::default();
                entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
                if Process32FirstW(snapshot, &mut entry).is_ok() {
                    loop {
                        let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(0);
                        let name = String::from_utf16_lossy(&entry.szExeFile[..len]).to_lowercase();
                        if name.contains(&stem) {
                            fam.push(format!("pid={} exe={name}", entry.th32ProcessID));
                        }
                        if Process32NextW(snapshot, &mut entry).is_err() {
                            break;
                        }
                    }
                }
                let _ = CloseHandle(snapshot);
            }
            if fam.is_empty() {
                super::log::info("launch", &format!("诊断 无 {stem}* 家族进程"));
            } else {
                super::log::info(
                    "launch",
                    &format!("诊断 {stem}* 家族进程: {:?}", fam.join(" | ")),
                );
            }
        }
    }
}

/// 取指定进程的完整镜像路径（受保护进程拿不到，返回 None）。
fn process_full_path(pid: u32) -> Option<String> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut size = buf.len() as u32;
        let r = QueryFullProcessImageNameW(
            h,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut size,
        );
        let _ = CloseHandle(h);
        r.ok()?;
        let n = (size as usize).min(buf.len());
        let len = buf[..n].iter().position(|&c| c == 0).unwrap_or(n);
        if len > 0 {
            Some(String::from_utf16_lossy(&buf[..len]))
        } else {
            None
        }
    }
}

/// 归一化路径用于比较：去 `\\?\` 前缀 + 小写。
fn normalize_path(p: &str) -> String {
    p.trim_start_matches("\\\\?\\").to_lowercase()
}

/// 检测是否有与 `exe_path` 同路径的进程已在运行，返回其 PID。
/// 先按文件名粗筛，再比对完整镜像路径（归一化后），避免不同目录同名 exe 误判。
fn find_running_process(exe_path: &str) -> Option<u32> {
    let exe_name = std::path::Path::new(exe_path)
        .file_name()?
        .to_string_lossy()
        .to_lowercase();
    let target_norm = normalize_path(
        &std::fs::canonicalize(exe_path)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| exe_path.to_string()),
    );
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
        let mut entry = PROCESSENTRY32W::default();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut found = None;
        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(0);
                let name = String::from_utf16_lossy(&entry.szExeFile[..len]).to_lowercase();
                if name == exe_name {
                    // 同名进程：进一步校验完整路径（拿不到路径时退化为仅按文件名，保留原行为）
                    let same_path = process_full_path(entry.th32ProcessID)
                        .map(|full| normalize_path(&full) == target_norm)
                        .unwrap_or(true);
                    if same_path {
                        found = Some(entry.th32ProcessID);
                        break;
                    }
                }
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snapshot);
        found
    }
}

/// 启动后异步把前台给程序主窗口（`CreateProcessW` 启动的程序不会自动抢前台，
/// 而 Launcher 全屏置顶会挡住它，需主动激活）。
///
/// 可靠激活序列：取消 Launcher 置顶 → 恢复目标窗口 → 临时置顶目标窗口（topmost 必然在最前）→ 激活。
///
/// 持续轮询（最多 30s，.NET 应用冷启动可能远超 10s，日志曾见 Playnite 主窗口 10s 内未出现）：
/// - Playnite/Steam 等先弹闪屏再开主窗口，`find_main_window` 按面积/标题挑「主窗口」，
///   窗口变化（闪屏→主窗口）或前台被抢时重新激活；
/// - 不提前收尾：期间目标窗口持续保持置顶+前台，确保主窗口无论何时出现（30s 内）都被激活；
///   AppRunning 期间 Launcher 不置顶（4.8），不会与目标窗口互抢；
/// - 轮询结束统一取消置顶收尾。一直找不到可见窗口时，每 5s 深挖一次诊断
///   （目标 pid 是否存活 + 同 exe 进程的顶层窗口含不可见），区分「最小化到托盘/隐藏」与
///   「父进程退出、UI 在子进程」两种情况。
fn bring_to_foreground(pid: u32, exe_name: &str) {
    let exe_name = exe_name.to_lowercase();
    std::thread::spawn(move || {
        let mut last = HWND(std::ptr::null_mut());
        let mut last_act = 0u32;
        for i in 0u32..300 {
            // 最多轮询 30 秒
            match find_main_window(pid, &exe_name) {
                Some(hwnd) => {
                    if hwnd != last || unsafe { GetForegroundWindow() } != hwnd {
                        // 窗口变化（闪屏→主窗口）或前台被抢：重新激活（至少间隔 300ms，避免高频刷）
                        if i.saturating_sub(last_act) >= 3 {
                            activate_window(hwnd);
                            last_act = i;
                        }
                        last = hwnd;
                    }
                    // 诊断：每 ~2s 记录一次主窗口状态，便于排查激活失败
                    if i % 20 == 0 {
                        super::log::info(
                            "launch",
                            &format!("轮询中 窗口={hwnd:?} 标题=\"{}\"", window_title(hwnd)),
                        );
                    }
                }
                None => {
                    if i % 50 == 0 {
                        // 每 5s 深挖：进程存活 + 相关进程的顶层窗口（含不可见）
                        log_launch_diagnostic(pid, &exe_name);
                    } else if i % 20 == 0 {
                        super::log::info(
                            "launch",
                            "轮询中 未找到可见主窗口（可能启动最小化到托盘）",
                        );
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        // 收尾：取消残留置顶 + 提到普通层顶部（避免顶住 Launcher），并留日志
        if !last.0.is_null() {
            finish_activation(last);
            super::log::info(
                "launch",
                &format!("激活轮询结束(30s) 最后窗口标题=\"{}\"", window_title(last)),
            );
        }
    });
}

/// 取窗口标题（日志用）。
fn window_title(hwnd: HWND) -> String {
    unsafe {
        let mut buf = [0u16; 256];
        let n = GetWindowTextW(hwnd, &mut buf);
        String::from_utf16_lossy(&buf[..n as usize])
    }
}

/// 激活指定窗口：取消 Launcher 置顶 → 恢复 → 临时置顶 → 绕过前台锁定激活（失败则模拟 Alt 重试）。
fn activate_window(hwnd: HWND) {
    unsafe {
        super::log::info("launch", &format!("激活窗口 标题=\"{}\"", window_title(hwnd)));
        // 1. 取消 Launcher 置顶（让目标窗口能显示最前）
        let launcher = state::launcher_hwnd();
        if launcher != 0 {
            window::not_topmost(HWND(launcher as *mut core::ffi::c_void));
        }
        // 2. 恢复目标窗口（若最小化）
        let _ = ShowWindow(hwnd, SW_RESTORE);
        // 3. 临时置顶（topmost 必然显示在最前；先置顶再激活，即使激活失败窗口也可见）
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
        );
        // 4. 挂接「当前前台线程」绕过前台锁定限制（与 focus.rs bring_to_front 一致；
        //    加上 SPI_SETFOREGROUNDLOCKTIMEOUT=0 双保险），SetForegroundWindow 失败则模拟 Alt 重试
        let fg_thread = GetWindowThreadProcessId(GetForegroundWindow(), None);
        let our_thread = GetCurrentThreadId();
        let _ = AttachThreadInput(our_thread, fg_thread, true);
        let mut ok = false;
        for _ in 0..3 {
            let _ = SetForegroundWindow(hwnd);
            if GetForegroundWindow() == hwnd {
                ok = true;
                break;
            }
            // 兜底：模拟 Alt 键按下/抬起，令系统认为本进程接收过用户输入后重试
            keybd_event(VK_MENU.0 as u8, 0, KEYEVENTF_EXTENDEDKEY, 0);
            keybd_event(VK_MENU.0 as u8, 0, KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP, 0);
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = AttachThreadInput(our_thread, fg_thread, false);
        super::log::info(
            "launch",
            &format!("激活完成 前台={:?} 目标={hwnd:?} 成功={ok}", GetForegroundWindow()),
        );
    }
}

/// 收尾：取消临时置顶 + 提到普通层顶部（避免掉到底层被 Launcher 挡住）。
fn finish_activation(hwnd: HWND) {
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_NOTOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE,
        );
        let _ = BringWindowToTop(hwnd);
        let fg_after = GetForegroundWindow();
        super::log::info("launch", &format!("取消置顶后前台={fg_after:?}"));
    }
}

/// 创建带 `KILL_ON_JOB_CLOSE` 的 Job（Launcher 崩溃时 OS 回收句柄 → 终止外部程序进程树）。
unsafe fn create_job() -> windows::core::Result<HANDLE> {
    let job = CreateJobObjectW(None, w!("WinNasLauncherJob"))?;
    let mut jeli = JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
        BasicLimitInformation: JOBOBJECT_BASIC_LIMIT_INFORMATION {
            LimitFlags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            ..Default::default()
        },
        ..Default::default()
    };
    SetInformationJobObject(
        job,
        JobObjectExtendedLimitInformation,
        &mut jeli as *mut _ as *const core::ffi::c_void,
        core::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
    )?;
    Ok(job)
}

/// 把 `.lnk` 解析为目标路径；非 `.lnk` 直接返回原路径。
fn resolve_target(path: &str) -> String {
    if path.to_lowercase().ends_with(".lnk") {
        super::icon::resolve_lnk_target(path).unwrap_or_else(|| path.to_string())
    } else {
        path.to_string()
    }
}

/// 启动外部程序（`.exe` / `.lnk`），返回进程 PID（`.lnk` 由 Shell 启动，无法可靠取 PID，返回 0）。
///
/// - 先解析 `.lnk` 目标，检测目标是否已在运行：是则激活窗口，不重复启动；
/// - `.exe`：`CreateProcessW` + Job Object（进程树退出检测，见 4.9）；
/// - `.lnk`：`ShellExecuteExW`（解析快捷方式启动，见 4.9）。
pub fn launch(path: &str) -> windows::core::Result<u32> {
    // 统一 .lnk 与 .exe：解析目标后先检测「已在运行」→ 激活窗口
    let target = resolve_target(path);
    if target.to_lowercase().ends_with(".exe") {
        if let Some(pid) = find_running_process(&target) {
            super::log::info("launch", &format!("已运行，激活窗口 pid={pid}"));
            bring_to_foreground(pid, &exe_file_name(&target));
            return Ok(pid);
        }
    }

    // 未运行：按原类型启动
    if path.to_lowercase().ends_with(".lnk") {
        // .lnk 优先解析目标直接 CreateProcessW + Job 启动（重点修订，见 4.9）：
        // ShellExecuteExW 对慢启动应用（Playnite 等）返回的 hProcess 是 Shell 进程而非目标进程，
        // 会激活 Shell 的「正在打开」对话框、退出检测提前误判、看门狗误夺焦点（Playnite 无法置前
        // 的根因）。解析出 .exe 目标就走 launch_exe（真实 pid + Job 退出检测 + 前台激活）；
        // 解析失败/非 .exe（bat/url/带参快捷方式）退回 ShellExecuteExW 保留原语义。
        if target.to_lowercase().ends_with(".exe") {
            super::log::info("launch", &format!("解析 .lnk 目标直接启动: {target}"));
            return launch_exe(&target);
        }
        launch_lnk(path)
    } else {
        launch_exe(path)
    }
}

/// 启动 `.lnk`（ShellExecuteExW + `SEE_MASK_NOCLOSEPROCESS` 拿 PID），并前台激活。
fn launch_lnk(path: &str) -> windows::core::Result<u32> {
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    let mut sei: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    sei.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    sei.fMask = SEE_MASK_NOCLOSEPROCESS;
    sei.lpVerb = w!("open");
    sei.lpFile = PCWSTR::from_raw(wide.as_ptr());
    sei.nShow = SW_SHOWNORMAL.0 as i32;
    unsafe {
        ShellExecuteExW(&mut sei)?;
    }

    // Shell 可能通过 DDE 激活已有实例（无进程句柄）：解析目标并尝试切前台
    if sei.hProcess.0.is_null() {
        let target = resolve_target(path);
        if target.to_lowercase().ends_with(".exe") {
            if let Some(pid) = find_running_process(&target) {
                super::log::info("launch", &format!("Shell 激活已有实例，切换前台 pid={pid}"));
                bring_to_foreground(pid, &exe_file_name(&target));
                return Ok(pid);
            }
        }
        return Ok(0);
    }
    let pid = unsafe { GetProcessId(sei.hProcess) };

    // 更新状态 + 前台激活（.lnk 非 exe 目标无确定文件名，仅按 pid 匹配窗口）
    state::inc_running();
    state::set_app_running(true);
    bring_to_foreground(pid, "");

    // 等进程退出（.lnk 无 Job Object，用 hProcess 句柄）
    let hprocess_raw = sei.hProcess.0 as usize;
    std::thread::spawn(move || unsafe {
        let hprocess = HANDLE(hprocess_raw as *mut core::ffi::c_void);
        WaitForSingleObject(hprocess, u32::MAX);
        let _ = CloseHandle(hprocess);
        let remaining = state::dec_running();
        if remaining == 0 {
            state::set_app_running(false);
            focus::schedule_reclaim();
        }
    });

    Ok(pid)
}

/// 启动一个 `.exe`（CreateProcessW + Job Object），返回进程 PID。
///
/// - GUI 程序用普通创建标志即可（`CREATE_NEW_PROCESS_GROUP` 仅对控制台程序有意义，不加）。
/// - 注意：windows crate 中 `CreateProcessW`/`CreateJobObjectW` 被 `Win32_Security` feature 门控。
fn launch_exe(exe_path: &str) -> windows::core::Result<u32> {
    let job = unsafe { create_job()? };

    // lpCommandLine 要求可变 PWSTR：路径加引号保活（含空格路径如 "C:\Program Files\..." 若不引号，
    // CreateProcessW 会按空格拆出错误的应用名导致启动失败；.lnk 解析出的目标路径多为含空格路径）
    let mut cmd: Vec<u16> = format!("\"{exe_path}\"").encode_utf16().chain(std::iter::once(0)).collect();
    let mut si = STARTUPINFOW::default();
    si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
    let mut pi = PROCESS_INFORMATION::default();

    unsafe {
        CreateProcessW(
            PCWSTR::null(),
            Some(windows::core::PWSTR(cmd.as_mut_ptr())),
            None,
            None,
            false,
            Default::default(),
            None,
            PCWSTR::null(),
            &si,
            &mut pi,
        )?;
    }
    let pid = pi.dwProcessId;

    // 纳入 Job（子进程自动继承 Job 归属，进程树整体追踪）
    unsafe {
        AssignProcessToJobObject(job, pi.hProcess)?;
    }

    // 更新状态：AppRunning
    state::inc_running();
    state::set_app_running(true);

    // 主动把前台给程序（程序不会自动抢前台，Launcher 全屏会挡住它）
    bring_to_foreground(pid, &exe_file_name(exe_path));

    // HANDLE 是 *mut c_void 不实现 Send，转原始地址值跨线程传递后重建
    let job_raw = job.0 as usize;
    let hthread_raw = pi.hThread.0 as usize;
    let hprocess_raw = pi.hProcess.0 as usize;

    // 线程等待进程树全部退出（Job active=0 → signaled）
    std::thread::spawn(move || unsafe {
        let job = HANDLE(job_raw as *mut core::ffi::c_void);
        let r = WaitForSingleObject(job, u32::MAX);
        // 清理
        let _ = CloseHandle(HANDLE(hthread_raw as *mut core::ffi::c_void));
        let _ = CloseHandle(HANDLE(hprocess_raw as *mut core::ffi::c_void));
        let _ = CloseHandle(job);

        if r == WAIT_OBJECT_0 {
            let remaining = state::dec_running();
            super::log::info("process", &format!("程序退出，剩余 {remaining} 个进程"));
            if remaining == 0 {
                state::set_app_running(false);
                focus::schedule_reclaim();
            }
        }
    });

    Ok(pid)
}
