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
