//! 配置模块：exe 目录 `setting.conf`（轻量配置）+ `apps.json`（网格菜单列表），内存缓存。
//!
//! 启动时：文件不存在 → 创建默认配置并读取；存在 → 直接读取。
//! 读取后存入内存缓存（`RwLock`），后续读写走内存，变更时写回文件。

use std::fs;
use std::path::PathBuf;
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

use super::log;

/// 网格菜单应用项。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppItem {
    pub name: String,
    pub exe: String,
    /// 图标（emoji 或本地图片路径；M3 阶段先空/emoji，图标提取见 4.11）
    pub icon: String,
    /// 打开次数（每次启动 +1，首页按此降序排序）
    #[serde(default)]
    pub launch_count: u32,
    /// 固定标记（手动「移到最前」后为 true，排到最前）
    #[serde(default)]
    pub pinned: bool,
    /// 移到最后标记（手动「移到最后」后为 true，排到所有项最后）
    #[serde(default)]
    pub pinned_end: bool,
}

/// 程序默认配置项。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// 开机自启
    pub autostart: bool,
    /// 自启是否已初始化（安装版首次启动自动开启后置 true，用户显式切换后也置 true，避免反复自动干预）
    #[serde(default)]
    pub autostart_initialized: bool,
    /// Esc 退出是否弹确认框
    pub exit_confirm: bool,
    /// UI 缩放倍数（1.0 = 1920×1080 基准，见设计文档 5.6）
    pub scale: f64,
    /// 首次启动引导是否完成（false = 首次启动，弹引导窗）
    pub initialized: bool,
    /// 菜单模式："manual"（不加载，手动维护）| "all"（加载全部程序）
    pub menu_mode: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            autostart: false,
            autostart_initialized: false,
            exit_confirm: true,
            scale: 1.0,
            initialized: false,
            menu_mode: "manual".to_string(),
        }
    }
}

/// 内存缓存
static CONFIG: RwLock<Option<Config>> = RwLock::new(None);
static APPS: RwLock<Option<Vec<AppItem>>> = RwLock::new(None);

fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn config_path() -> PathBuf {
    exe_dir().join("setting.conf")
}

fn apps_path() -> PathBuf {
    exe_dir().join("apps.json")
}

/// 原子写（写临时文件 + rename），避免写一半崩溃损坏 JSON（与 system_state 一致）。
fn atomic_write(path: &std::path::Path, data: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, data)?;
    fs::rename(&tmp, path)
}

/// 启动时初始化：没有则创建默认配置并读取，有则直接读取，存入内存缓存。
pub fn init() {
    let path = config_path();
    let config = if path.exists() {
        match fs::read_to_string(&path).and_then(|s| Ok(serde_json::from_str::<Config>(&s)?)) {
            Ok(c) => {
                log::info("config", &format!("读取配置 {}", path.display()));
                c
            }
            Err(e) => {
                log::info("config", &format!("配置解析失败({e})，使用默认值"));
                Config::default()
            }
        }
    } else {
        let config = Config::default();
        match serde_json::to_string_pretty(&config) {
            Ok(data) => match atomic_write(&path, &data) {
                Ok(_) => log::info("config", &format!("创建默认配置 {}", path.display())),
                Err(e) => log::info("config", &format!("创建配置失败: {e}")),
            },
            Err(e) => log::info("config", &format!("序列化默认配置失败: {e}")),
        }
        config
    };
    *CONFIG.write().unwrap() = Some(config);

    // 顺便加载菜单列表到内存缓存
    load_apps();
}

/// 读取配置（内存缓存）。
pub fn get() -> Config {
    CONFIG.read().unwrap().clone().unwrap_or_default()
}

/// 更新配置（写内存缓存 + 写回文件）。
pub fn save(config: &Config) {
    *CONFIG.write().unwrap() = Some(config.clone());
    let path = config_path();
    if let Ok(data) = serde_json::to_string_pretty(config) {
        let _ = atomic_write(&path, &data);
    }
}

/// 读取网格菜单列表（内存缓存；首次调用从 apps.json 读）。
/// 排序：先按打开次数降序，再按名称升序。
pub fn load_apps() -> Vec<AppItem> {
    if let Some(apps) = APPS.read().unwrap().clone() {
        return apps;
    }
    let path = apps_path();
    let mut apps = if path.exists() {
        fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<Vec<AppItem>>(&s).ok())
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    sort_apps(&mut apps);
    *APPS.write().unwrap() = Some(apps.clone());
    apps
}

/// 排序：移到最前（pinned）排最前，移到最后（pinned_end）排最后，其余按打开次数降序 + 名称升序排中间。
fn sort_apps(apps: &mut [AppItem]) {
    let mut front: Vec<AppItem> = apps
        .iter()
        .filter(|a| a.pinned && !a.pinned_end)
        .cloned()
        .collect();
    let mut middle: Vec<AppItem> = apps
        .iter()
        .filter(|a| !a.pinned && !a.pinned_end)
        .cloned()
        .collect();
    let end: Vec<AppItem> = apps.iter().filter(|a| a.pinned_end).cloned().collect();
    middle.sort_by(|a, b| {
        b.launch_count
            .cmp(&a.launch_count)
            .then_with(|| a.name.cmp(&b.name))
    });
    front.extend(middle);
    front.extend(end);
    apps.clone_from_slice(&front);
}

/// 增加指定 exe 路径的打开次数。
pub fn increment_launch_count(exe: &str) {
    let mut apps = load_apps();
    if let Some(app) = apps.iter_mut().find(|a| a.exe == exe) {
        app.launch_count += 1;
        save_apps(&apps);
    }
}

/// 移到最前（固定到第一位）。
pub fn move_to_front(exe: &str) {
    let mut apps = load_apps();
    if let Some(i) = apps.iter().position(|a| a.exe == exe) {
        let mut app = apps.remove(i);
        app.pinned = true;
        app.pinned_end = false;
        apps.insert(0, app);
        save_apps(&apps);
    }
}

/// 移到最后（固定到最后一位）。
pub fn move_to_end(exe: &str) {
    let mut apps = load_apps();
    if let Some(i) = apps.iter().position(|a| a.exe == exe) {
        let mut app = apps.remove(i);
        app.pinned = false;
        app.pinned_end = true;
        apps.push(app);
        save_apps(&apps);
    }
}

/// 删除应用。
pub fn remove_app(exe: &str) {
    let mut apps = load_apps();
    apps.retain(|a| a.exe != exe);
    save_apps(&apps);
}

/// 重命名应用。
pub fn rename_app(exe: &str, new_name: &str) {
    let mut apps = load_apps();
    if let Some(app) = apps.iter_mut().find(|a| a.exe == exe) {
        app.name = new_name.to_string();
        save_apps(&apps);
    }
}

/// 写网格菜单列表（排序 + 写内存缓存 + apps.json）。
pub fn save_apps(apps: &[AppItem]) {
    let mut sorted = apps.to_vec();
    sort_apps(&mut sorted);
    *APPS.write().unwrap() = Some(sorted.clone());
    let path = apps_path();
    if let Ok(data) = serde_json::to_string_pretty(&sorted) {
        let _ = atomic_write(&path, &data);
    }
}
