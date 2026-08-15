//! PoC 共享工具：统一语义键模型 + 日志。

/// 统一语义键：蓝牙遥控 / USB IR / 飞鼠 / 手柄 / 键盘 全部收敛到此枚举。
/// 前端只消费语义键，不感知物理来源。
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Up,
    Down,
    Left,
    Right,
    Ok,
    Back,
    Menu,
    VolUp,
    VolDown,
    Power,
}

impl Action {
    pub fn as_str(&self) -> &'static str {
        match self {
            Action::Up => "Up",
            Action::Down => "Down",
            Action::Left => "Left",
            Action::Right => "Right",
            Action::Ok => "Ok",
            Action::Back => "Back",
            Action::Menu => "Menu",
            Action::VolUp => "VolUp",
            Action::VolDown => "VolDown",
            Action::Power => "Power",
        }
    }
}

#[allow(dead_code)]
pub fn log(tag: &str, msg: &str) {
    println!("[{tag}] {msg}");
}
