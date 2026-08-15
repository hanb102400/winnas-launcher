//! 音量控制（设计文档 4.10）：Core Audio 主输出设备音量读写 / 静音。
//!
//! 流程：`CoCreateInstance(MMDeviceEnumerator)` → `GetDefaultAudioEndpoint(eRender, eConsole)`
//! → `Activate<IAudioEndpointVolume>` → 音量 / 静音读写。

use windows::core::GUID;
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{eConsole, eRender, IMMDeviceEnumerator};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
};

/// MMDeviceEnumerator 的 CLSID：BCDE0395-E52F-467C-8E3D-C4579291692E
const CLSID_MMDEVICE_ENUMERATOR: GUID =
    GUID::from_u128(0xBCDE0395_E52F_467C_8E3D_C4579291692E);

fn endpoint_volume() -> Option<IAudioEndpointVolume> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&CLSID_MMDEVICE_ENUMERATOR, None, CLSCTX_ALL).ok()?;
        let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
        device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok()
    }
}

/// 当前主音量（0.0 ~ 1.0）。
pub fn get_volume() -> f32 {
    endpoint_volume()
        .and_then(|v| unsafe { v.GetMasterVolumeLevelScalar().ok() })
        .unwrap_or(0.5)
}

/// 设置主音量（0.0 ~ 1.0）。
pub fn set_volume(level: f32) {
    let level = level.clamp(0.0, 1.0);
    if let Some(v) = endpoint_volume() {
        unsafe {
            let _ = v.SetMasterVolumeLevelScalar(level, std::ptr::null());
        }
    }
}

/// 切换静音，返回切换后的静音状态（true = 已静音）。
pub fn toggle_mute() -> bool {
    if let Some(v) = endpoint_volume() {
        unsafe {
            let muted = v.GetMute().unwrap_or(windows::core::BOOL(0)).as_bool();
            let _ = v.SetMute(!muted, std::ptr::null());
            return !muted;
        }
    }
    false
}
