// PoC4: Job Object 进程树退出检测
//
// 验证目标:
//   1. 父进程退出、子进程常驻时, Job 内 active 计数仍追踪整个进程树 (不只看根进程)
//   2. TerminateJobObject 可回收整棵进程树 (KILL_ON_JOB_CLOSE 同理)
//   3. Job 句柄在 active process count = 0 时成为 signaled -> WaitForSingleObject 返回
//      (等价于 JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO 结论)
//
// 场景: 根进程 powershell 放入 Job, powershell 启动常驻 ping 后立即退出 (父退子活),
//       观察 active=1 (ping 仍存活) -> TerminateJobObject 全杀 -> active=0 -> signaled。
//
// 使用: 运行后自动演示, 全程 ~12s。

use std::time::Duration;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicAccountingInformation,
    JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
    TerminateJobObject, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_BASIC_LIMIT_INFORMATION,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows::Win32::System::Threading::{
    CreateProcessW, WaitForSingleObject, CREATE_NO_WINDOW, STARTUPINFOW, PROCESS_INFORMATION,
};

fn active_count(job: HANDLE) -> Result<u32, windows::core::Error> {
    let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
    unsafe {
        QueryInformationJobObject(
            Some(job),
            JobObjectBasicAccountingInformation,
            &mut info as *mut _ as *mut core::ffi::c_void,
            core::mem::size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
            None,
        )?;
    }
    Ok(info.ActiveProcesses)
}

fn poll(job: HANDLE, rounds: u32, gap_ms: u64) {
    for i in 0..rounds {
        match active_count(job) {
            Ok(n) => println!("[poc4]   active={n} (sample {})", i + 1),
            Err(e) => println!("[poc4]   query failed: {e:?}"),
        }
        std::thread::sleep(Duration::from_millis(gap_ms));
    }
}

fn main() -> windows::core::Result<()> {
    println!("[poc4] Job Object process-tree exit detection");
    unsafe {
        // 1. 创建 Job, 开启 KILL_ON_JOB_CLOSE (Job 句柄关闭时回收整棵进程树)
        let job = CreateJobObjectW(None, w!("WinnasPoc4Job"))?;
        println!("[poc4] job created: {job:?}");

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
        println!("[poc4] KILL_ON_JOB_CLOSE set");

        // 2. 启动根进程 powershell (会放入 Job), 它启动常驻 ping 后立即退出
        //    lpcommandline 要求可变 PWSTR: 用栈上 Vec<u16> 保活
        let mut cmd: Vec<u16> = "powershell.exe -NoProfile -WindowStyle Hidden -Command \"Start-Process -FilePath ping.exe -ArgumentList '-t','127.0.0.1' -WindowStyle Hidden; exit\""
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let mut si = STARTUPINFOW::default();
        si.cb = core::mem::size_of::<STARTUPINFOW>() as u32;
        let mut pi = PROCESS_INFORMATION::default();
        CreateProcessW(
            PCWSTR::null(),
            Some(windows::core::PWSTR(cmd.as_mut_ptr())),
            None,
            None,
            false,
            CREATE_NO_WINDOW,
            None,
            PCWSTR::null(),
            &si,
            &mut pi,
        )?;
        println!("[poc4] powershell spawned (root pid={})", pi.dwProcessId);

        // 3. 根进程放入 Job (其子进程自动继承 Job 归属)
        AssignProcessToJobObject(job, pi.hProcess)?;
        println!("[poc4] root process assigned to job");

        // 4. 等根 powershell 退出 (父退), 观察 active 计数仍 = 1 (ping 常驻)
        WaitForSingleObject(pi.hProcess, 6000);
        println!("[poc4] root powershell exited -> active count:");
        poll(job, 3, 300);

        // 5. 回收整棵进程树 -> active=0 -> Job signaled
        println!("[poc4] TerminateJobObject -> killing ping tree");
        TerminateJobObject(job, 0)?;
        poll(job, 4, 300);

        // 6. WaitForSingleObject(job) 应在 active=0 后返回 signaled
        let r = WaitForSingleObject(job, 5000);
        let ok = r == WAIT_OBJECT_0;
        println!(
            "[poc4] WaitForSingleObject(job) = {r:?} -> JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO-equivalent: {}",
            if ok { "PASS" } else { "FAIL" }
        );

        CloseHandle(pi.hThread)?;
        CloseHandle(pi.hProcess)?;
        CloseHandle(job)?;
        Ok(())
    }
}
