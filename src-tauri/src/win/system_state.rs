//! 系统状态快照与恢复（设计文档 4.13）——产品级底线
//!
//! 不依赖"正常退出才还原"：所有异常场景（强杀/蓝屏/断电/崩溃）下退出还原代码都不会执行，
//! 必须靠"下次启动自愈"。核心机制：启动写 `running` 标记 → 正常退出清标记 → 下次启动若发现
//! 残留 `running` 标记则判定上次异常退出，触发自愈恢复。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{log, power, taskbar};

/// 系统状态快照（持久化到 `conf/state_snapshot.json`）
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StateSnapshot {
    /// 运行标记：`true` = 上次运行未正常退出（残留）
    pub running: bool,
    /// 任务栏原始 AutoHide 状态（M2 隐藏任务栏前记录）
    pub taskbar_autohide: Option<i32>,
    /// 桌面图标原始可见状态（M2 隐藏图标前记录）
    pub desktop_icons_visible: Option<bool>,
}

/// 是否为便携版（exe 同目录存在 `portable.flag`）。
pub fn is_portable() -> bool {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            return dir.join("portable.flag").exists();
        }
    }
    false
}

/// 配置目录：
/// - 便携版（exe 同目录存在 `portable.flag`）：exe 同目录 `conf/`
/// - 安装版 / 默认：`%APPDATA%\WinNasLauncher\conf\`
pub fn conf_dir() -> PathBuf {
    if is_portable() {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                return dir.join("conf");
            }
        }
    }
    let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(appdata).join("WinNasLauncher").join("conf")
}

fn snapshot_path() -> PathBuf {
    conf_dir().join("state_snapshot.json")
}

fn read_snapshot() -> StateSnapshot {
    std::fs::read_to_string(snapshot_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// 原子写（写临时文件 + rename），避免半截文件。
fn atomic_write(path: &Path, data: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path)
}

fn write_snapshot(snap: &StateSnapshot) -> std::io::Result<()> {
    let data = serde_json::to_string_pretty(snap)?;
    atomic_write(&snapshot_path(), &data)
}

/// 启动标记 running（setup 阶段调用）。
pub fn mark_running() {
    let mut snap = read_snapshot();
    snap.running = true;
    let _ = write_snapshot(&snap);
}

/// 正常退出清除标记（`RunEvent::Exit` 调用）。
pub fn mark_clean() {
    let mut snap = read_snapshot();
    snap.running = false;
    let _ = write_snapshot(&snap);
}

/// 启动时自愈检测：若残留 `running` 标记，说明上次异常退出，执行恢复。
/// 返回 `true` 表示检测到残留并已触发自愈。
pub fn check_self_heal() -> bool {
    let snap = read_snapshot();
    if snap.running {
        restore_system();
        return true;
    }
    false
}

/// 一键恢复系统桌面状态（任务栏、电源策略等），并清除残留标记。
/// 供两处调用：启动自愈（4.13 兜底）与设置面板【一键恢复】按钮。
pub fn restore_system() {
    taskbar::restore();
    power::restore_power();
    let mut snap = read_snapshot();
    snap.running = false;
    // TODO(M3): 依据 snap.taskbar_autohide / snap.desktop_icons_visible 还原更多系统状态
    let _ = write_snapshot(&snap);
    log::info("system_state", "restored (taskbar + power + running flag)");
}
