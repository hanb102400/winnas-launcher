// PoC2: WH_KEYBOARD_LL 低层键盘钩子 (纯函数单测 + 安装验证, 零按键注入)
//
// 验证目标:
//   1. 拦截判定逻辑 classify() 正确 (纯函数单元测试, 不注入任何真实按键)
//   2. 钩子能安装/卸载 (SetWindowsHookExW / UnhookWindowsHookEx)
//   3. 记录已知限制: 无法看到高完整性进程; Ctrl+Alt+Del (SAS) 无法拦截
//
// 重要: 不再用 SendInput 注入真实键盘事件做自测 —— 注入修饰键(Ctrl/Shift/Alt/Win)
//       一旦 KEYUP 未送达就会粘住真实键盘 (Ctrl 锁死 / Enter 失灵)。
//       真实按键拦截能力已由手动验证(Alt+F4 / Win+D / Win / Win+Tab 均被拦截)证明。

use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_D, VK_ESCAPE, VK_F4, VK_LWIN, VK_MENU, VK_RETURN, VK_RWIN,
    VK_SHIFT, VK_TAB,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, KBDLLHOOKSTRUCT, SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL,
};

const WM_KEYDOWN: u32 = 0x0100;
const WM_KEYUP: u32 = 0x0101;
const WM_SYSKEYDOWN: u32 = 0x0104;
const WM_SYSKEYUP: u32 = 0x0105;

fn is_down(vk: u16) -> bool {
    unsafe { (GetAsyncKeyState(vk as i32) as u16) & 0x8000 != 0 }
}

fn msg_kind(m: u32) -> &'static str {
    match m {
        WM_KEYDOWN | WM_SYSKEYDOWN => "DOWN",
        WM_KEYUP | WM_SYSKEYUP => "UP",
        _ => "?",
    }
}

/// 拦截判定纯函数: 输入 (按键 vk + 各修饰键按下状态) -> 返回应拦截的组合名。
/// 与 GetAsyncKeyState 解耦, 便于单元测试。
fn classify(vk: u16, win: bool, alt: bool, ctrl: bool, shift: bool) -> Option<&'static str> {
    if win {
        match vk {
            _ if vk == VK_D.0 => Some("WIN+D"),
            _ if vk == VK_TAB.0 => Some("WIN+TAB"),
            // 单独按 Win 键: keydown + keyup 都要拦, 否则开始菜单在 keyup 弹出
            _ if vk == VK_LWIN.0 || vk == VK_RWIN.0 => Some("WIN"),
            _ => None,
        }
    } else if alt && vk == VK_F4.0 {
        Some("ALT+F4")
    } else if ctrl && shift && vk == VK_ESCAPE.0 {
        Some("CTRL+SHIFT+ESC")
    } else if ctrl && !shift && vk == VK_ESCAPE.0 {
        Some("CTRL+ESC")
    } else {
        None
    }
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
            println!(
                "[hook] INTERCEPT {name} ({}, vk=0x{:04x})",
                msg_kind(msg),
                vk
            );
            return LRESULT(1); // 吞掉该击键, 阻止系统处理
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

/// 单元测试: (vk, win, alt, ctrl, shift, 期望结果)
fn run_self_tests() -> bool {
    let cases: &[(u16, bool, bool, bool, bool, Option<&str>)] = &[
        // 拦截项
        (VK_F4.0, false, true, false, false, Some("ALT+F4")),
        (VK_D.0, true, false, false, false, Some("WIN+D")),
        (VK_TAB.0, true, false, false, false, Some("WIN+TAB")),
        (VK_LWIN.0, true, false, false, false, Some("WIN")),
        (VK_RWIN.0, true, false, false, false, Some("WIN")),
        (VK_ESCAPE.0, false, false, true, true, Some("CTRL+SHIFT+ESC")),
        (VK_ESCAPE.0, false, false, true, false, Some("CTRL+ESC")),
        // 非拦截项 (关键: 不误伤正常键, 尤其 Enter / Ctrl+A)
        (VK_RETURN.0, false, false, false, false, None),
        (VK_RETURN.0, false, false, true, false, None), // Ctrl+Enter 不拦
        (0x41, false, false, true, false, None),        // Ctrl+A 不拦
        (VK_ESCAPE.0, false, false, false, false, None), // 单独 Esc 不拦
        (VK_F4.0, false, false, false, false, None),     // 单独 F4 不拦
    ];

    let mut all_pass = true;
    for &(vk, win, alt, ctrl, shift, expected) in cases {
        let got = classify(vk, win, alt, ctrl, shift);
        let ok = got == expected;
        if !ok {
            all_pass = false;
        }
        println!(
            "[test] vk=0x{vk:04x} win={win} alt={alt} ctrl={ctrl} shift={shift} -> {:?} (expect {:?}) {}",
            got,
            expected,
            if ok { "PASS" } else { "FAIL" }
        );
    }
    all_pass
}

fn main() {
    println!("[poc2] WH_KEYBOARD_LL hook (pure-fn self-test + install check, no key injection)");
    println!("[poc2] Known limit: Ctrl+Alt+Del (SAS) cannot be intercepted; hook cannot see high-integrity processes.");

    // 1. 拦截判定逻辑单测 (零按键注入)
    println!("[poc2] --- classify() unit tests ---");
    let logic_ok = run_self_tests();
    println!(
        "[poc2] classify(): {}",
        if logic_ok { "PASS" } else { "FAIL" }
    );

    // 2. 钩子安装/卸载验证
    println!("[poc2] --- hook install/uninstall ---");
    unsafe {
        let hmod = GetModuleHandleW(None).expect("GetModuleHandleW");
        let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), Some(HINSTANCE(hmod.0)), 0);
        let install_ok = match hook {
            Ok(h) => {
                println!("[poc2] hook installed OK: {h:?}");
                let _ = UnhookWindowsHookEx(h);
                println!("[poc2] hook uninstalled OK");
                true
            }
            Err(e) => {
                println!("[poc2] SetWindowsHookExW FAILED: {e:?}");
                false
            }
        };

        println!(
            "[poc2] ================= RESULT ================="
        );
        println!(
            "[poc2] OVERALL: {} (classify={}, install={})",
            if logic_ok && install_ok { "PASS" } else { "FAIL" },
            if logic_ok { "PASS" } else { "FAIL" },
            if install_ok { "PASS" } else { "FAIL" },
        );
        println!(
            "[poc2] note: real-key interception (Alt+F4/Win+D/Win/Win+Tab) was already proven manually;"
        );
        println!("[poc2]       this run verifies classification logic + hook lifecycle without touching the keyboard.");
    }
}
