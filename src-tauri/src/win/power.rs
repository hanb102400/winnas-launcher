//! 电源管理（设计文档 4.6 / FR-17）
//!
//! - `SetThreadExecutionState` 阻止休眠 + 屏幕常亮（Launcher 运行期间持续）；
//! - 电源按钮操作（PBUTTONACTION）设为「睡眠」，退出时还原（配合 FR-19 遥控器电源键）。

use std::path::PathBuf;
use std::sync::OnceLock;

use windows::Win32::System::Power::{
    SetThreadExecutionState, ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED,
};

/// 启动前记录的原始电源按钮操作 (AC, DC)，退出时还原。
static ORIG_POWER_BUTTON: OnceLock<(u32, u32)> = OnceLock::new();

fn powercfg_exe() -> PathBuf {
    std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
        .join("System32")
        .join("powercfg.exe")
}

/// 阻止休眠 + 保持屏幕常亮（Launcher 运行期间持续）。
pub fn keep_awake() {
    unsafe {
        SetThreadExecutionState(ES_CONTINUOUS | ES_DISPLAY_REQUIRED | ES_SYSTEM_REQUIRED);
    }
}

/// 恢复系统默认电源策略（仅清除 continuous 标志，让系统可正常休眠）。
pub fn restore_power() {
    unsafe {
        SetThreadExecutionState(ES_CONTINUOUS);
    }
}

/// 读取当前电源按钮操作（PBUTTONACTION），返回 (AC, DC)。
/// 解析 `powercfg /q` 输出里的 `0x` 十六进制值（AC 在前、DC 在后，与语言无关）。
fn read_power_button_action() -> Option<(u32, u32)> {
    let out = std::process::Command::new(powercfg_exe())
        .args(["/q", "SCHEME_CURRENT", "SUB_BUTTONS", "PBUTTONACTION"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut vals: Vec<u32> = Vec::new();
    for line in text.lines() {
        if let Some(idx) = line.find("0x") {
            let hex: String = line[idx + 2..]
                .chars()
                .take_while(|c| c.is_ascii_hexdigit())
                .collect();
            if let Ok(v) = u32::from_str_radix(&hex, 16) {
                vals.push(v);
            }
        }
    }
    if vals.len() >= 2 {
        Some((vals[0], vals[1]))
    } else {
        None
    }
}

/// 写入电源按钮操作 (AC, DC)，并重新激活方案使其生效。
fn write_power_button_action(ac: u32, dc: u32) {
    let ac_str = ac.to_string();
    let dc_str = dc.to_string();
    let _ = std::process::Command::new(powercfg_exe())
        .args([
            "/setacvalueindex",
            "SCHEME_CURRENT",
            "SUB_BUTTONS",
            "PBUTTONACTION",
            &ac_str,
        ])
        .status();
    let _ = std::process::Command::new(powercfg_exe())
        .args([
            "/setdcvalueindex",
            "SCHEME_CURRENT",
            "SUB_BUTTONS",
            "PBUTTONACTION",
            &dc_str,
        ])
        .status();
    let _ = std::process::Command::new(powercfg_exe())
        .args(["/setactive", "SCHEME_CURRENT"])
        .status();
}

/// 设置电源按钮操作为「睡眠」（1），记录原始值供退出还原。返回是否成功。
pub fn set_power_button_sleep() -> bool {
    let Some((ac, dc)) = read_power_button_action() else {
        super::log::info("power", "读取电源按钮操作失败（可能无物理电源键），跳过");
        return false;
    };
    let _ = ORIG_POWER_BUTTON.set((ac, dc));
    write_power_button_action(1, 1);
    super::log::info("power", &format!("电源按钮操作已设为睡眠（原 AC={ac} DC={dc}）"));
    true
}

/// 还原电源按钮操作到启动前的原始值（退出时调用）。
pub fn restore_power_button() {
    if let Some((ac, dc)) = ORIG_POWER_BUTTON.get() {
        write_power_button_action(*ac, *dc);
        super::log::info("power", &format!("已还原电源按钮操作 AC={ac} DC={dc}"));
    }
}
