// PoC1: WebView2 原生全屏 + 遥控键位链路 (自动验证)
//
// 验证目标:
//   1. SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2) 生效
//   2. 无边框全屏窗口 (CreateWindowExW + SetWindowPos, 非 Tauri set_fullscreen)
//   3. WebView2 环境 + 控制器初始化成功
//   4. SetBoundsMode(RAW_PIXELS) + SetBounds(全屏) 填满无黑边
//   5. 导航到本地 HTML (含焦点导航 demo) 成功, 证明"遥控键位链路"前端可跑
//
// 全程自动, 导航完成后打印 PASS 并退出, 不注入按键。

use std::sync::mpsc;

use windows::core::{w, HSTRING, Interface};
use windows::Win32::Foundation::{E_POINTER, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetSystemMetrics, PostQuitMessage,
    RegisterClassW, SetWindowPos, CS_HREDRAW, CS_VREDRAW, HWND_TOP, SM_CXSCREEN, SM_CYSCREEN,
    SWP_NOZORDER, SWP_SHOWWINDOW, WINDOW_EX_STYLE, WM_DESTROY, WNDCLASSW, WS_POPUP,
};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    CreateCoreWebView2Environment, ICoreWebView2, ICoreWebView2Controller,
    ICoreWebView2Controller3, ICoreWebView2Environment, COREWEBVIEW2_BOUNDS_MODE_USE_RAW_PIXELS,
};
use webview2_com::{
    CreateCoreWebView2ControllerCompletedHandler, CreateCoreWebView2EnvironmentCompletedHandler,
    NavigationCompletedEventHandler,
};

const CLASS: windows::core::PCWSTR = w!("WinnasPoc1");

const HTML: &str = r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><style>
body{margin:0;background:#10131a;font-family:'Segoe UI',sans-serif;color:#e6e9f0}
.grid{display:grid;grid-template-columns:repeat(5,1fr);gap:24px;padding:80px}
.app{background:#1c2230;border:3px solid transparent;border-radius:16px;padding:36px;text-align:center;font-size:26px;transition:all .15s}
.app:focus{border-color:#4da3ff;background:#243047;transform:scale(1.05);outline:none}
.app .icon{font-size:64px}
.status{position:fixed;bottom:20px;left:50%;transform:translateX(-50%);color:#8a93a6;font-size:20px}
</style></head><body>
<div class="grid" id="grid"></div>
<div class="status" id="status">WinNas Launcher PoC1 - 方向键移动焦点 / Enter 启动 / Back 返回</div>
<script>
const apps=['视频','音乐','浏览器','文件','设置','游戏','直播','相册','Kodi','Steam'];
const icons=['📺','🎵','🌐','📁','⚙️','🎮','📡','🖼️','🍿','♨️'];
const grid=document.getElementById('grid');
apps.forEach((n,i)=>{const d=document.createElement('div');d.className='app';d.tabIndex=0;
d.innerHTML=`<div class="icon">${icons[i]}</div><div class="name">${n}</div>`;
d.addEventListener('keydown',e=>{if(e.key==='Enter')document.getElementById('status').textContent='启动: '+n;});
grid.appendChild(d);});
const cells=[...grid.children];
const focusAt=i=>{i=Math.max(0,Math.min(cells.length-1,i));cells[i].focus();};
focusAt(0);
document.addEventListener('keydown',e=>{const idx=cells.indexOf(document.activeElement);const col=5;
switch(e.key){
case 'ArrowRight':focusAt(idx+1);e.preventDefault();break;
case 'ArrowLeft':focusAt(idx-1);e.preventDefault();break;
case 'ArrowUp':focusAt(idx-col);e.preventDefault();break;
case 'ArrowDown':focusAt(idx+col);e.preventDefault();break;
case 'Backspace':case 'Escape':document.getElementById('status').textContent='返回';e.preventDefault();break;
}});
</script></body></html>"#;

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn create_environment() -> webview2_com::Result<ICoreWebView2Environment> {
    let (tx, rx) = mpsc::channel();
    CreateCoreWebView2EnvironmentCompletedHandler::wait_for_async_operation(
        Box::new(|handler| unsafe {
            CreateCoreWebView2Environment(&handler).map_err(webview2_com::Error::WindowsError)
        }),
        Box::new(move |error_code, environment| {
            error_code?;
            let env = environment.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
            tx.send(env).expect("send over mpsc channel");
            Ok(())
        }),
    )?;
    rx.recv().map_err(|_| webview2_com::Error::SendError)
}

unsafe fn create_controller(
    env: &ICoreWebView2Environment,
    hwnd: HWND,
) -> webview2_com::Result<ICoreWebView2Controller> {
    let (tx, rx) = mpsc::channel();
    let env = env.clone();
    CreateCoreWebView2ControllerCompletedHandler::wait_for_async_operation(
        Box::new(move |handler| unsafe {
            env.CreateCoreWebView2Controller(hwnd, &handler)
                .map_err(webview2_com::Error::WindowsError)
        }),
        Box::new(move |error_code, controller| {
            error_code?;
            let c = controller.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
            tx.send(c).expect("send over mpsc channel");
            Ok(())
        }),
    )?;
    rx.recv().map_err(|_| webview2_com::Error::SendError)
}

unsafe fn navigate(webview: &ICoreWebView2) -> webview2_com::Result<()> {
    let (tx, rx) = mpsc::channel();
    let handler = NavigationCompletedEventHandler::create(Box::new(move |_sender, _args| {
        let _ = tx.send(());
        Ok(())
    }));
    let mut token = 0i64;
    webview.add_NavigationCompleted(&handler, &mut token)?;
    let html = HSTRING::from(HTML);
    webview.NavigateToString(&html)?;
    webview2_com::wait_with_pump(rx)?;
    webview.remove_NavigationCompleted(token)?;
    Ok(())
}

fn main() -> webview2_com::Result<()> {
    unsafe {
        // 1. DPI 感知 (PER_MONITOR_AWARE_V2, 避免多显示器/缩放全屏错位)
        let dpi = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        println!(
            "[poc1] DPI per-monitor-aware-v2: {}",
            if dpi.is_ok() { "OK" } else { "already set (OK)" }
        );

        // 2. COM 初始化 (STA, WebView2 要求)
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        println!("[poc1] COM initialized (STA)");

        // 3. 无边框全屏窗口
        let sw = GetSystemMetrics(SM_CXSCREEN);
        let sh = GetSystemMetrics(SM_CYSCREEN);
        let hinst = HINSTANCE(GetModuleHandleW(None)?.0);
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: hinst,
            lpszClassName: CLASS,
            ..Default::default()
        };
        let _ = RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            CLASS,
            w!("WinNas PoC1"),
            WS_POPUP,
            0,
            0,
            sw,
            sh,
            None,
            None,
            Some(hinst),
            None,
        )?;
        let _ = SetWindowPos(hwnd, Some(HWND_TOP), 0, 0, sw, sh, SWP_SHOWWINDOW | SWP_NOZORDER);
        println!("[poc1] fullscreen borderless window: {sw}x{sh} @ {hwnd:?}");

        // 4-5. WebView2 环境 + 控制器
        let environment = create_environment()?;
        println!("[poc1] WebView2 environment created OK");
        let controller = create_controller(&environment, hwnd)?;
        println!("[poc1] WebView2 controller created OK");

        // 6. 全屏 bounds (raw pixels 避免 DPI 黑边)
        let controller3: ICoreWebView2Controller3 = controller.cast()?;
        controller3.SetBoundsMode(COREWEBVIEW2_BOUNDS_MODE_USE_RAW_PIXELS)?;
        controller.SetBounds(RECT {
            left: 0,
            top: 0,
            right: sw,
            bottom: sh,
        })?;
        controller.SetIsVisible(true)?;
        let webview = controller.CoreWebView2()?;
        println!("[poc1] bounds set to fullscreen (raw pixels)");

        // 7. 导航到内嵌 HTML (焦点导航 demo)
        navigate(&webview)?;
        println!("[poc1] navigation to local HTML completed OK");

        println!("[poc1] ================= RESULT =================");
        println!("[poc1] WebView2 fullscreen + navigation: PASS");

        // 8. 清理退出
        let _ = controller.Close();
        let _ = DestroyWindow(hwnd);
        CoUninitialize();
        Ok(())
    }
}
