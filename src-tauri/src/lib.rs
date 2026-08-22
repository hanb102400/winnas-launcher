mod win;

use tauri::Manager;

/// 启动外部程序（前端 `invoke('launch_app', { path })`）。
#[tauri::command]
fn launch_app(path: String) -> Result<u32, String> {
    win::log::info("launch", &format!("启动程序: {path}"));
    win::process::launch(&path)
        .map(|pid| {
            win::config::increment_launch_count(&path);
            pid
        })
        .map_err(|e| {
            win::log::info("launch", &format!("启动失败: {path} -> {e}"));
            e.to_string()
        })
}

/// 一键恢复系统桌面状态（前端 `invoke('restore_desktop')`，见 4.13 应急按钮）。
#[tauri::command]
fn restore_desktop() {
    win::log::info("restore", "一键恢复桌面状态");
    win::system_state::restore_system();
}

/// 启用开机自启（前端 `invoke('set_autostart', { enabled })`，见 4.4）。
#[tauri::command]
fn set_autostart(enabled: bool) -> bool {
    let ok = if enabled {
        win::autostart::enable()
    } else {
        win::autostart::disable()
    };
    // 仅在实际成功时同步更新配置，避免注册表写入失败导致状态脱钩
    if ok {
        let mut config = win::config::get();
        config.autostart = enabled;
        config.autostart_initialized = true;
        win::config::save(&config);
    }
    win::log::info("autostart", &format!("设置开机自启 {enabled} -> {ok}"));
    ok
}

/// 读取配置（前端 `invoke('get_config')`）。
#[tauri::command]
fn get_config() -> win::config::Config {
    win::config::get()
}

/// 设置界面语言（前端 `invoke('set_language', { code })`，FR-20；选择后前端整页重载应用新语言）。
/// 仅接受 10 种受支持 locale；成功返回 true。
#[tauri::command]
fn set_language(code: String) -> bool {
    if !win::i18n::SUPPORTED.contains(&code.as_str()) {
        win::log::info("i18n", &format!("非法语言代码: {code}"));
        return false;
    }
    let mut config = win::config::get();
    config.language = code.clone();
    config.language_auto = false; // 用户显式选择，后续不再跟随系统语言
    win::config::save(&config);
    win::log::info("i18n", &format!("设置语言: {code}"));
    true
}

/// 退出应用（前端 `invoke('exit_app')`，Esc 确认框确认后调用）。
#[tauri::command]
fn exit_app(app: tauri::AppHandle) {
    win::log::info("exit", "退出应用");
    app.exit(0);
}

/// 扫描系统已安装程序（前端 `invoke('scan_apps')`，见 4.12）。
/// 与首页首次「加载全部菜单」同一来源（开始菜单 `.lnk`），保证两个列表一致。
#[tauri::command]
fn scan_apps() -> Vec<win::config::AppItem> {
    win::scanner::scan()
}

/// 读取网格菜单列表（前端 `invoke('get_apps')`）。
#[tauri::command]
fn get_apps() -> Vec<win::config::AppItem> {
    win::config::load_apps()
}

/// 添加应用到网格菜单（前端 `invoke('add_app', { app })`），返回新列表。
#[tauri::command]
fn add_app(app: win::config::AppItem) -> Vec<win::config::AppItem> {
    let mut apps = win::config::load_apps();
    win::log::info("apps", &format!("添加应用: {}", app.name));
    apps.push(app);
    win::config::save_apps(&apps);
    apps
}

/// 读取主音量（0.0~1.0）。
#[tauri::command]
fn get_volume() -> f32 {
    win::volume::get_volume()
}

/// 设置主音量（0.0~1.0）。
#[tauri::command]
fn set_volume(level: f32) {
    win::volume::set_volume(level);
}

/// 切换静音，返回切换后是否静音。
#[tauri::command]
fn toggle_mute() -> bool {
    win::volume::toggle_mute()
}

/// 清除桌面菜单缓存：清空 apps.json + initialized=false，下次启动重新走首次引导。
#[tauri::command]
fn clear_menu_cache() {
    win::config::save_apps(&[]);
    let mut config = win::config::get();
    config.initialized = false;
    config.menu_mode = "manual".to_string();
    win::config::save(&config);
    win::log::info("apps", "清除菜单缓存，下次启动重新初始化");
}

/// 进入维护模式：恢复任务栏 + 关闭置顶（便于调试 Windows 桌面）。
#[tauri::command]
fn enter_maintenance() {
    win::taskbar::restore();
    win::power::restore_power();
    let launcher = win::state::launcher_hwnd();
    if launcher != 0 {
        win::window::not_topmost(windows::Win32::Foundation::HWND(launcher as *mut _));
    }
    win::state::set_maintenance(true);
    win::log::info("maintenance", "进入维护模式");
}

/// 退出维护模式：隐藏任务栏 + 置顶，回到 TV 模式。
#[tauri::command]
fn exit_maintenance() {
    win::taskbar::hide();
    win::power::keep_awake();
    let launcher = win::state::launcher_hwnd();
    if launcher != 0 {
        win::window::topmost(windows::Win32::Foundation::HWND(launcher as *mut _));
    }
    win::state::set_maintenance(false);
    win::log::info("maintenance", "退出维护模式");
}

/// 系统操作：关机 / 重启 / 睡眠 / 锁屏。
#[tauri::command]
fn system_shutdown() {
    win::log::info("system", "关机");
    win::system::shutdown();
}

#[tauri::command]
fn system_reboot() {
    win::log::info("system", "重启");
    win::system::reboot();
}

#[tauri::command]
fn system_sleep() {
    win::log::info("system", "睡眠");
    win::system::sleep();
}

#[tauri::command]
fn system_lock() {
    win::log::info("system", "锁屏");
    win::system::lock();
}

/// 首次启动引导选择（前端 `invoke('init_menu', { mode })`，mode = "manual" | "all"），返回菜单列表。
#[tauri::command]
fn init_menu(mode: String) -> Vec<win::config::AppItem> {
    let mut config = win::config::get();
    config.initialized = true;
    config.menu_mode = mode.clone();
    win::config::save(&config);

    let apps = if mode == "all" {
        let scanned = win::scanner::scan();
        win::config::save_apps(&scanned);
        scanned
    } else {
        win::config::save_apps(&[]);
        Vec::new()
    };
    win::log::info("boot", &format!("首次启动初始化: {mode}，菜单 {} 项", apps.len()));
    apps
}

/// 管理 APP：删除应用。
#[tauri::command]
fn remove_app(exe: String) -> Vec<win::config::AppItem> {
    win::log::info("apps", &format!("删除应用: {exe}"));
    win::config::remove_app(&exe);
    win::config::load_apps()
}

/// 管理 APP：移到最前。
#[tauri::command]
fn move_app_to_front(exe: String) -> Vec<win::config::AppItem> {
    win::config::move_to_front(&exe);
    win::config::load_apps()
}

/// 管理 APP：移到最后。
#[tauri::command]
fn move_app_to_end(exe: String) -> Vec<win::config::AppItem> {
    win::config::move_to_end(&exe);
    win::config::load_apps()
}

/// 管理 APP：重命名应用。
#[tauri::command]
fn rename_app(exe: String, name: String) -> Vec<win::config::AppItem> {
    win::log::info("apps", &format!("重命名应用: {exe} -> {name}"));
    win::config::rename_app(&exe, &name);
    win::config::load_apps()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 单实例：已有实例运行时直接退出，避免重复进程
    if !win::state::acquire_single_instance() {
        return;
    }

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // 日志初始化（生成 conf/logs/YYYY-MM-DD.log）
            win::log::init();
            // 保存全局 AppHandle（keyhook 等后台线程 emit 事件用）
            win::state::set_app_handle(app.handle().clone());
            // 注册崩溃自动重启（WER；仅崩溃时重启，正常退出不重启；失败则降级到 4.13 状态快照自愈）
            unsafe {
                // 仅崩溃时自动重启（排除挂起/补丁/重启触发的重启，避免误循环）
                let _ = windows::Win32::System::Recovery::RegisterApplicationRestart(
                    windows::core::PCWSTR::null(),
                    windows::Win32::System::Recovery::RESTART_NO_HANG
                        | windows::Win32::System::Recovery::RESTART_NO_PATCH
                        | windows::Win32::System::Recovery::RESTART_NO_REBOOT,
                );
            }
            win::log::info("boot", "已注册崩溃自动重启");
            // 配置初始化（exe 目录 setting.conf，读取或创建 + 内存缓存）
            win::config::init();
            // 自启默认值：便携版不自动开启，安装版首次启动默认开启（用户关闭后不再干预）
            win::autostart::apply_autostart_default();
            win::log::info("boot", "WinNas Launcher 启动");

            // 启动顺序：自愈检测 → 标记 running → 钩子 → 置顶/焦点 → 任务栏/电源
            if win::system_state::check_self_heal() {
                win::log::info("boot", "检测到上次异常退出，已自愈");
            }
            win::system_state::mark_running();
            win::keyhook::start();

            if let Some(window) = app.get_webview_window("main") {
                // 设置窗口/任务栏图标（bundle.icon 只嵌入 exe 图标，窗口图标需显式设置）
                if let Some(icon) = app.default_window_icon() {
                    let _ = window.set_icon(icon.clone());
                }
                let hwnd = window.hwnd()?;
                // 全屏由 tauri.conf `fullscreen: true` 负责；这里只做置顶
                win::window::topmost(hwnd);
                win::focus::init(hwnd);

                // 禁用 WebView2 默认右键菜单：遥控器「菜单键」= VK_APPS 0x5D，会触发宿主级右键
                // 菜单，DOM 的 contextmenu preventDefault 拦不住，须在宿主层关闭。
                // `with_webview` 排队到 UI 线程执行（WebView2 API 须在 UI/COM 线程调用）。
                #[cfg(windows)]
                let _ = window.with_webview(move |platform_webview| {
                    let controller = platform_webview.controller();
                    if let Ok(core) = unsafe { controller.CoreWebView2() } {
                        if let Ok(settings) = unsafe { core.Settings() } {
                            let _ = unsafe { settings.SetAreDefaultContextMenusEnabled(false) };
                        }
                    }
                });
            }

            win::taskbar::hide();
            win::power::keep_awake();
            // 电源按钮操作设为「睡眠」（遥控器电源键 → S3 睡眠），退出时还原
            win::power::set_power_button_sleep();
            win::log::info("boot", "启动完成");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            launch_app,
            restore_desktop,
            set_autostart,
            exit_app,
            get_config,
            set_language,
            scan_apps,
            get_apps,
            add_app,
            init_menu,
            get_volume,
            set_volume,
            toggle_mute,
            system_shutdown,
            system_reboot,
            system_sleep,
            system_lock,
            clear_menu_cache,
            enter_maintenance,
            exit_maintenance,
            remove_app,
            move_app_to_front,
            move_app_to_end,
            rename_app
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|_app_handle, event| {
        // 正常退出：恢复任务栏 + 电源 + 清除 running 标记
        if let tauri::RunEvent::Exit = event {
            win::taskbar::restore();
            win::power::restore_power();
            win::power::restore_power_button();
            win::focus::restore_foreground_lock_timeout();
            win::system_state::mark_clean();
            win::log::info("exit", "正常退出，已恢复系统状态");
        }
    });
}
