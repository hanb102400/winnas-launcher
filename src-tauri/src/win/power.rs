//! 防休眠 / 屏幕常亮（设计文档 4.6）
//!
//! `SetThreadExecutionState(ES_CONTINUOUS | ES_DISPLAY_REQUIRED | ES_SYSTEM_REQUIRED)` 一次设置即可，
//! 无轮询；电源事件（休眠唤醒/显示器恢复）后由系统保持，必要时重断言（M3 电源事件钩子）。

use windows::Win32::System::Power::{
    SetThreadExecutionState, ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED,
};

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
