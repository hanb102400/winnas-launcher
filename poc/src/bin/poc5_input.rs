// PoC5: 多路输入收敛为统一语义键 (纯映射单测 + gilrs 后端检测, 零注入)
//
// 验证目标:
//   1. 键盘 -> Action 映射正确 (纯函数单测)
//   2. 手柄 (gilrs / XInput+GameInput 后端) -> 同一套 Action (纯函数单测)
//   3. gilrs 后端能初始化并枚举设备 (无手柄时返回空列表 = 后端可用)
//   4. 蓝牙遥控/IR/飞鼠 复用同一张映射表即可收敛 (设计说明)
//
// 关键: 键盘部分只用 GetAsyncKeyState *读取*状态, 不注入任何键, 不锁键。

#[path = "../common.rs"]
mod common;

use common::Action;
use gilrs::{Axis, Button, Gilrs};

/// 键盘虚拟键 -> 语义键 (模拟蓝牙遥控 / IR / 飞鼠统一到同一张映射表)
fn map_key(vk: u16) -> Option<Action> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        VK_BACK, VK_DOWN, VK_ESCAPE, VK_LEFT, VK_NEXT, VK_PRIOR, VK_RETURN, VK_RIGHT, VK_UP, VK_A,
        VK_D, VK_F1, VK_S, VK_W,
    };
    match vk {
        _ if vk == VK_UP.0 || vk == VK_W.0 => Some(Action::Up),
        _ if vk == VK_DOWN.0 || vk == VK_S.0 => Some(Action::Down),
        _ if vk == VK_LEFT.0 || vk == VK_A.0 => Some(Action::Left),
        _ if vk == VK_RIGHT.0 || vk == VK_D.0 => Some(Action::Right),
        _ if vk == VK_RETURN.0 => Some(Action::Ok),
        _ if vk == VK_BACK.0 || vk == VK_ESCAPE.0 => Some(Action::Back),
        _ if vk == VK_F1.0 => Some(Action::Menu),
        _ if vk == VK_PRIOR.0 => Some(Action::VolUp),
        _ if vk == VK_NEXT.0 => Some(Action::VolDown),
        _ => None,
    }
}

/// 手柄按键 -> 语义键
fn map_button(btn: Button) -> Option<Action> {
    match btn {
        Button::DPadUp => Some(Action::Up),
        Button::DPadDown => Some(Action::Down),
        Button::DPadLeft => Some(Action::Left),
        Button::DPadRight => Some(Action::Right),
        Button::South => Some(Action::Ok),         // A
        Button::East => Some(Action::Back),        // B
        Button::Start => Some(Action::Menu),
        Button::RightTrigger2 => Some(Action::VolUp), // RT
        Button::LeftTrigger2 => Some(Action::VolDown), // LT
        _ => None,
    }
}

/// 手柄摇杆轴 -> 方向 (飞鼠类设备可复用)
fn map_axis(axis: Axis, value: f32) -> Option<Action> {
    match axis {
        Axis::LeftStickX => {
            if value < -0.5 {
                Some(Action::Left)
            } else if value > 0.5 {
                Some(Action::Right)
            } else {
                None
            }
        }
        Axis::LeftStickY => {
            if value < -0.5 {
                Some(Action::Up)
            } else if value > 0.5 {
                Some(Action::Down)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn main() {
    println!("[poc5] unified semantic-key input (keyboard + gamepad -> Action)");
    println!("[poc5] zero injection: keyboard is read-only via GetAsyncKeyState, no lock risk.");

    // 1. 键盘映射单测
    println!("[poc5] --- keyboard mapping tests ---");
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        VK_BACK, VK_DOWN, VK_ESCAPE, VK_LEFT, VK_NEXT, VK_PRIOR, VK_RETURN, VK_RIGHT, VK_UP, VK_A,
        VK_D, VK_F1, VK_S, VK_W,
    };
    let kcases: &[(u16, Option<Action>)] = &[
        (VK_UP.0, Some(Action::Up)),
        (VK_W.0, Some(Action::Up)),
        (VK_DOWN.0, Some(Action::Down)),
        (VK_S.0, Some(Action::Down)),
        (VK_LEFT.0, Some(Action::Left)),
        (VK_A.0, Some(Action::Left)),
        (VK_RIGHT.0, Some(Action::Right)),
        (VK_D.0, Some(Action::Right)),
        (VK_RETURN.0, Some(Action::Ok)),
        (VK_BACK.0, Some(Action::Back)),
        (VK_ESCAPE.0, Some(Action::Back)),
        (VK_F1.0, Some(Action::Menu)),
        (VK_PRIOR.0, Some(Action::VolUp)),
        (VK_NEXT.0, Some(Action::VolDown)),
        (0x46, None), // 'F' 之类非映射键 -> None
    ];
    let mut kpass = true;
    for &(vk, expected) in kcases {
        let got = map_key(vk);
        let ok = got == expected;
        if !ok {
            kpass = false;
        }
        println!(
            "[test] key vk=0x{vk:04x} -> {:?} (expect {:?}) {}",
            got.map(|a| a.as_str()),
            expected.map(|a| a.as_str()),
            if ok { "PASS" } else { "FAIL" }
        );
    }

    // 2. 手柄按键映射单测
    println!("[poc5] --- gamepad button mapping tests ---");
    let bcases: &[(Button, Option<Action>)] = &[
        (Button::DPadUp, Some(Action::Up)),
        (Button::DPadDown, Some(Action::Down)),
        (Button::DPadLeft, Some(Action::Left)),
        (Button::DPadRight, Some(Action::Right)),
        (Button::South, Some(Action::Ok)),
        (Button::East, Some(Action::Back)),
        (Button::Start, Some(Action::Menu)),
        (Button::RightTrigger2, Some(Action::VolUp)),
        (Button::LeftTrigger2, Some(Action::VolDown)),
        (Button::North, None), // Y 未映射 -> None
    ];
    let mut bpass = true;
    for &(btn, expected) in bcases {
        let got = map_button(btn);
        let ok = got == expected;
        if !ok {
            bpass = false;
        }
        println!(
            "[test] button {:?} -> {:?} (expect {:?}) {}",
            btn,
            got.map(|a| a.as_str()),
            expected.map(|a| a.as_str()),
            if ok { "PASS" } else { "FAIL" }
        );
    }

    // 3. 摇杆轴映射单测
    println!("[poc5] --- gamepad axis mapping tests ---");
    let acases: &[(Axis, f32, Option<Action>)] = &[
        (Axis::LeftStickX, -0.9, Some(Action::Left)),
        (Axis::LeftStickX, 0.9, Some(Action::Right)),
        (Axis::LeftStickX, 0.0, None),
        (Axis::LeftStickY, -0.9, Some(Action::Up)),
        (Axis::LeftStickY, 0.9, Some(Action::Down)),
        (Axis::LeftStickY, 0.0, None),
    ];
    let mut apass = true;
    for &(axis, value, expected) in acases {
        let got = map_axis(axis, value);
        let ok = got == expected;
        if !ok {
            apass = false;
        }
        println!(
            "[test] axis {:?}={value:+.1} -> {:?} (expect {:?}) {}",
            axis,
            got.map(|a| a.as_str()),
            expected.map(|a| a.as_str()),
            if ok { "PASS" } else { "FAIL" }
        );
    }

    // 4. gilrs 后端检测 (初始化 + 枚举设备)
    println!("[poc5] --- gilrs backend ---");
    let gilrs_ok = match Gilrs::new() {
        Ok(g) => {
            let count = g.gamepads().count();
            for (id, gp) in g.gamepads() {
                println!("[gamepad] connected: id={id} name={}", gp.name());
            }
            println!(
                "[poc5] gilrs initialized OK, {} gamepad(s) connected (0 = backend available, none plugged in)",
                count
            );
            true
        }
        Err(e) => {
            println!("[poc5] gilrs init FAILED: {e:?}");
            false
        }
    };

    println!("[poc5] ================= RESULT =================");
    let all_ok = kpass && bpass && apass && gilrs_ok;
    println!(
        "[poc5] keyboard={} button={} axis={} gilrs={}",
        if kpass { "PASS" } else { "FAIL" },
        if bpass { "PASS" } else { "FAIL" },
        if apass { "PASS" } else { "FAIL" },
        if gilrs_ok { "PASS" } else { "FAIL" },
    );
    println!(
        "[poc5] OVERALL: {}",
        if all_ok { "PASS" } else { "FAIL" }
    );
    println!(
        "[poc5] note: BT-remote / IR / air-mouse reuse the same mapping table via Action enum; only the physical source differs."
    );
}
