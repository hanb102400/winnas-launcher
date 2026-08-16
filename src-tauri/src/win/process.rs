//! 启动外部程序 + 进程树退出检测（设计文档 4.9）
//!
//! 每程序一个 Job Object（`KILL_ON_JOB_CLOSE`），进程树全部退出（active=0）时 Job 句柄 signaled，
//! 天然覆盖"父进程退出、子进程存活"场景（Steam / 模拟器 / 启动器）。已由 M0 PoC4 验证。
//!
//! 退出后递减运行计数；归零则 `AppRunning=false` 并触发 focus 延时夺焦（4.8）。

use std::time::Duration;

use windows::core::{w, BOOL, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, WAIT_OBJECT_0};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_BASIC_LIMIT_INFORMATION,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    AttachThreadInput, CreateProcessW, GetCurrentThreadId, GetProcessId, OpenProcess,
    QueryFullProcessImageNameW, WaitForSingleObject, PROCESS_INFORMATION, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, STARTUPINFOW,
};
use windows::Win32::UI::Shell::{ShellExecuteExW, SHELLEXECUTEINFOW, SEE_MASK_NOCLOSEPROCESS};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, EnumWindows, GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
    IsWindowVisible, SetForegroundWindow, SetWindowPos, ShowWindow, HWND_NOTOPMOST, HWND_TOPMOST,
    SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SW_RESTORE, SW_SHOWNORMAL,
};

use super::{focus, state, window};

/// 枚举窗口回调的线程局部上下文（通过 lParam 传入，避免全局静态的并发竞态）。
struct FindWindowState {
    pid: u32,
    hwnd: HWND,
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let state = &mut *(lparam.0 as *mut FindWindowState);
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == state.pid && IsWindowVisible(hwnd).as_bool() {
        state.hwnd = hwnd;
        return BOOL(0); // 停止枚举
    }
    BOOL(1) // 继续
}

/// 找进程的第一个可见主窗口。
fn find_main_window(pid: u32) -> Option<HWND> {
    unsafe {
        let mut state = FindWindowState {
            pid,
            hwnd: HWND(std::ptr::null_mut()),
        };
        let _ = EnumWindows(
            Some(enum_proc),
            LPARAM(&mut state as *mut FindWindowState as isize),
        )
        .ok();
        if state.hwnd.0.is_null() {
            None
        } else {
            Some(state.hwnd)
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
fn bring_to_foreground(pid: u32) {
    std::thread::spawn(move || {
        for _ in 0..50 {
            // 最多轮询 5 秒等窗口出现
            if let Some(hwnd) = find_main_window(pid) {
                unsafe {
                    let mut title = [0u16; 256];
                    let n = GetWindowTextW(hwnd, &mut title);
                    let title = String::from_utf16_lossy(&title[..n as usize]);
                    super::log::info("launch", &format!("激活窗口 pid={pid} 标题=\"{title}\""));
                    // 1. 取消 Launcher 置顶（让目标窗口能显示最前）
                    let launcher = state::launcher_hwnd();
                    if launcher != 0 {
                        window::not_topmost(HWND(launcher as *mut core::ffi::c_void));
                    }
                    // 2. 恢复目标窗口（若最小化）
                    let _ = ShowWindow(hwnd, SW_RESTORE);
                    // 3. 挂接输入线程，绕过前台锁定限制，确保 SetForegroundWindow 生效
                    let target_thread = GetWindowThreadProcessId(hwnd, None);
                    let our_thread = GetCurrentThreadId();
                    let _ = AttachThreadInput(our_thread, target_thread, true);
                    let r = SetForegroundWindow(hwnd);
                    let _ = SetWindowPos(
                        hwnd,
                        Some(HWND_TOPMOST),
                        0,
                        0,
                        0,
                        0,
                        SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
                    );
                    let _ = AttachThreadInput(our_thread, target_thread, false);
                    let fg = GetForegroundWindow();
                    super::log::info(
                        "launch",
                        &format!("SetForegroundWindow 返回 {r:?}，前台={fg:?} 目标={hwnd:?}"),
                    );
                }
                // 4. 延时后取消临时置顶 + 提到普通层顶部（避免掉到底层被 Launcher 挡住）
                std::thread::sleep(Duration::from_millis(600));
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
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    });
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
            bring_to_foreground(pid);
            return Ok(pid);
        }
    }

    // 未运行：按原类型启动
    if path.to_lowercase().ends_with(".lnk") {
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
                bring_to_foreground(pid);
                return Ok(pid);
            }
        }
        return Ok(0);
    }
    let pid = unsafe { GetProcessId(sei.hProcess) };

    // 更新状态 + 前台激活
    state::inc_running();
    state::set_app_running(true);
    bring_to_foreground(pid);

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

    // lpCommandLine 要求可变 PWSTR：用栈上 Vec<u16> 保活
    let mut cmd: Vec<u16> = exe_path.encode_utf16().chain(std::iter::once(0)).collect();
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
    bring_to_foreground(pid);

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
