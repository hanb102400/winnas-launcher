//! 低层键盘钩子拦截系统组合键（设计文档 4.7）
//!
//! 用 `WH_KEYBOARD_LL` 拦截 Win / Win+D / Win+Tab / Alt+F4 / Ctrl+Shift+Esc，防止误触切出 Launcher。
//! 已由 M0 PoC2 验证：判定逻辑 12 用例全过 + 钩子生命周期 + 真实按键拦截。
//!
//! 注意（PoC2 教训）：不注入真实按键自测（修饰键 KEYUP 丢失会粘住真实键盘）；`Ctrl+Alt+Del` 属 SAS 无法拦截。

use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use tauri::Emitter;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_BROWSER_HOME, VK_CONTROL, VK_D, VK_ESCAPE, VK_F4, VK_LWIN, VK_MENU,
    VK_RWIN, VK_SHIFT, VK_SLEEP, VK_TAB, VK_VOLUME_DOWN, VK_VOLUME_MUTE, VK_VOLUME_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, TranslateMessage, KBDLLHOOKSTRUCT,
    SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL, MSG,
};

use super::{focus, log, state, system, volume};

const WM_KEYDOWN: u32 = 0x0100;
const WM_SYSKEYDOWN: u32 = 0x0104;

fn is_down(vk: u16) -> bool {
    unsafe { (GetAsyncKeyState(vk as i32) as u16) & 0x8000 != 0 }
}

/// 拦截判定纯函数：输入 (按键 vk + 各修饰键状态) -> 应拦截的组合名。
/// 与 `GetAsyncKeyState` 解耦，便于单测（PoC2 已验证）。
fn classify(vk: u16, win: bool, alt: bool, ctrl: bool, shift: bool) -> Option<&'static str> {
    // 音量键 / 睡眠键优先（不依赖修饰键，吞掉避免系统重复处理）
    match vk {
        _ if vk == VK_VOLUME_UP.0 => return Some("VOL_UP"),
        _ if vk == VK_VOLUME_DOWN.0 => return Some("VOL_DOWN"),
        _ if vk == VK_VOLUME_MUTE.0 => return Some("VOL_MUTE"),
        _ if vk == VK_SLEEP.0 => return Some("SLEEP"),
        _ if vk == VK_BROWSER_HOME.0 => return Some("HOME"),
        _ => {}
    }
    if win {
        match vk {
            _ if vk == VK_D.0 => Some("WIN+D"),
            _ if vk == VK_TAB.0 => Some("WIN+TAB"),
            // 单独按 Win 键：keydown + keyup 都要拦，否则开始菜单在 keyup 弹出
            _ if vk == VK_LWIN.0 || vk == VK_RWIN.0 => Some("WIN"),
            _ => None,
        }
    } else if alt && vk == VK_F4.0 {
        Some("ALT+F4")
    } else if ctrl && shift && vk == VK_ESCAPE.0 {
        Some("CTRL+SHIFT+ESC")
    } else {
        None
    }
}

/// 音量变化后，emit 事件到前端显示 OSD。
fn emit_volume_changed() {
    if let Some(handle) = state::app_handle() {
        let v = volume::get_volume();
        let _ = handle.emit("volume-changed", v);
    }
}

/// 后台线程执行拦截动作（钩子回调须尽快返回，禁止在回调内做重活/写文件/flush）。
fn run_action(name: &'static str) {
    std::thread::spawn(move || match name {
        "VOL_UP" => {
            volume::set_volume(volume::get_volume() + 0.05);
            emit_volume_changed();
        }
        "VOL_DOWN" => {
            volume::set_volume(volume::get_volume() - 0.05);
            emit_volume_changed();
        }
        "VOL_MUTE" => {
            volume::toggle_mute();
            emit_volume_changed();
        }
        "SLEEP" => {
            // 遥控器电源键 → S3 睡眠（而非关机），吞掉系统默认行为
            log::info("keyhook", "电源键 → S3 睡眠");
            system::sleep();
        }
        "HOME" => {
            // 遥控器 Home 键 → 唤起 Launcher（置顶 + 前台 + 焦点）
            focus::show_launcher();
        }
        _ => eprintln!("[keyhook] INTERCEPT {name}"),
    });
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let msg = wparam.0 as u32;
        let kbd = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        let vk = kbd.vkCode as u16;

        let win = is_down(VK_LWIN.0) || is_down(VK_RWIN.0);
        let alt = is_down(VK_MENU.0);
        let ctrl = is_down(VK_CONTROL.0);
        let shift = is_down(VK_SHIFT.0);

        if let Some(name) = classify(vk, win, alt, ctrl, shift) {
            if msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN {
                // 重活（音量/睡眠）移到后台线程，钩子回调只判定 + 吞键（LL 钩子有超时限制）
                run_action(name);
            }
            return LRESULT(1); // 吞掉该击键，阻止系统处理
        }

        // 诊断：记录未拦截的特殊键（vk>=0xA6 浏览器/媒体键，或 0x24 Home），定位遥控器电源键实际 VK 码
        if (msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN) && (vk == 0x24 || vk >= 0xA6) {
            log::info("keyhook", &format!("未拦截按键 vk=0x{vk:02X}"));
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

/// 启动钩子线程（独立线程，回调需要该线程泵消息）。进程退出时随主线程终止。
pub fn start() {
    std::thread::spawn(|| unsafe {
        let hmod = match GetModuleHandleW(None) {
            Ok(h) => h,
            Err(e) => {
                log::info("keyhook", &format!("GetModuleHandleW failed: {e:?}"));
                return;
            }
        };
        let hook = SetWindowsHookExW(
            WH_KEYBOARD_LL,
            Some(hook_proc),
            Some(HINSTANCE(hmod.0)),
            0,
        );
        match hook {
            Ok(h) => {
                log::info("keyhook", &format!("LL hook installed: {h:?}"));
                // 消息泵：LL 钩子回调需要安装线程泵消息，否则回调不会触发
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    let _ = TranslateMessage(&msg);
                    let _ = DispatchMessageW(&msg);
                }
                let _ = UnhookWindowsHookEx(h);
            }
            Err(e) => {
                // 降级策略（4.7 强制）：钩子失败不可崩溃，仅保留 WebView2 窗口内按键
                log::info("keyhook", &format!("install failed (degrade): {e:?}"));
            }
        }
    });
}
