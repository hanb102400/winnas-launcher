//! 开机自启（设计文档 4.4）
//!
//! M2 实现安装版默认方案：注册表 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`（用户级，无需管理员）。
//! 用系统自带 `reg.exe` 写入，避免额外依赖。
//! 便携版启动文件夹 `.lnk` 方案在 M5 打包时实现。

const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "WinNasLauncher";

fn current_exe_path() -> String {
    std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// 启用开机自启（写入当前 exe 路径到 Run 键）。
pub fn enable() -> bool {
    let exe = current_exe_path();
    if exe.is_empty() {
        return false;
    }
    std::process::Command::new("reg")
        .args([
            "add",
            RUN_KEY,
            "/v",
            VALUE_NAME,
            "/t",
            "REG_SZ",
            "/d",
            &exe,
            "/f",
        ])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// 禁用开机自启（删除 Run 键值）。
pub fn disable() -> bool {
    std::process::Command::new("reg")
        .args(["delete", RUN_KEY, "/v", VALUE_NAME, "/f"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// 应用自启默认值（启动时调用一次）：
/// - 便携版：默认不自动开启自启；
/// - 安装版：首次启动默认开启自启，用户显式关闭后不再自动干预。
pub fn apply_autostart_default() {
    if super::system_state::is_portable() {
        return;
    }
    let mut config = super::config::get();
    if config.autostart_initialized || config.autostart {
        return; // 已初始化过 或 已开启
    }
    let ok = enable();
    config.autostart = ok;
    config.autostart_initialized = true;
    super::config::save(&config);
    super::log::info("autostart", &format!("安装版首次启动，默认开启自启 -> {ok}"));
}
