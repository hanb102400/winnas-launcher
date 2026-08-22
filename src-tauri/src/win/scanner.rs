//! 扫描已安装程序（设计文档 4.12）
//!
//! 数据来源：开始菜单 `*.lnk`（%APPDATA% + %ProgramData% 的 Programs 目录，递归）。
//! 「首页首次加载全部菜单」与「设置添加 APP」共用此列表，保证两边一致。
//!
//! 去重：按名称。返回 `Vec<AppItem>`（icon 由 icon::extract_icon_data_url 提取）。

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use super::config::AppItem;
use super::icon;
use super::log;

/// 扫描开始菜单 `.lnk`（Windows 开始菜单"全部程序"的来源，数量少且干净）。
/// 用于首次启动「加载全部菜单」和「设置添加 APP」页（两者共用，列表一致）。
pub fn scan() -> Vec<AppItem> {
    let mut apps: Vec<AppItem> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    scan_start_menu(&mut apps, &mut seen);

    log::info("scanner", &format!("扫描开始菜单完成，共 {} 个程序", apps.len()));
    apps
}

fn scan_start_menu(apps: &mut Vec<AppItem>, seen: &mut HashSet<String>) {
    let dirs = [
        std::env::var("APPDATA")
            .ok()
            .map(|d| Path::new(&d).join(r"Microsoft\Windows\Start Menu\Programs")),
        std::env::var("ProgramData")
            .ok()
            .map(|d| Path::new(&d).join(r"Microsoft\Windows\Start Menu\Programs")),
    ];
    for dir in dirs.iter().flatten() {
        scan_lnk_dir(dir, apps, seen);
    }
}

fn scan_lnk_dir(dir: &Path, apps: &mut Vec<AppItem>, seen: &mut HashSet<String>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_lnk_dir(&path, apps, seen);
        } else if path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .as_deref()
            == Some("lnk")
        {
            let name = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            // 过滤名字带"修复/卸载/帮助/repair/uninstall/help"的快捷方式（不区分大小写）
            let lower = name.to_lowercase();
            if lower.contains("修复")
                || lower.contains("卸载")
                || lower.contains("帮助")
                || lower.contains("repair")
                || lower.contains("uninstall")
                || lower.contains("help")
            {
                continue;
            }
            if seen.insert(name.clone()) {
                let exe = path.to_string_lossy().to_string();
                let icon = icon::extract_icon_data_url(&exe).unwrap_or_default();
                apps.push(AppItem { name, exe, icon, launch_count: 0, pinned: false, pinned_end: false });
            }
        }
    }
}
