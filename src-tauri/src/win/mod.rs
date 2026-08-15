//! Win32 系统能力封装（对应设计文档 4.1 模块总览的 `win/*`）。
//! 各子模块独立、可单独测试，统一在 lib.rs 的 setup 阶段初始化。

pub mod autostart;
pub mod config;
pub mod focus;
pub mod icon;
pub mod keyhook;
pub mod log;
pub mod power;
pub mod process;
pub mod scanner;
pub mod state;
pub mod system;
pub mod system_state;
pub mod taskbar;
pub mod volume;
pub mod window;
