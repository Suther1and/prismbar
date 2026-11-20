use serde::{Deserialize, Serialize};
use windows::core::{GUID, HRESULT, Interface, PCWSTR};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{
    eCapture, eConsole, eMultimedia, eRender, IAudioSessionControl2, IAudioSessionManager2,
    IMMDeviceEnumerator, ISimpleAudioVolume, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
    COINIT_MULTITHREADED, STGM_READ,
};

use crate::win_shell::wide;
use crate::windows_tracker::{friendly_exe_name, process_exe_path};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDeviceInfo {
    pub id: String,
    pub name: String,
    pub short_name: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioSessionInfo {
    pub pid: u32,
    pub name: String,
    pub volume: u32,
    pub muted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioState {
    pub master_volume: u32,
    pub master_muted: bool,
    pub mic_muted: bool,
    pub devices: Vec<AudioDeviceInfo>,
    pub sessions: Vec<AudioSessionInfo>,
}

fn shorten_device_name(name: &str) -> String {
    if let Some(idx) = name.find('(') {
        let head = name[..idx].trim();
        if !head.is_empty() && head.len() <= 20 {
            return head.to_string();
        }
    }
    name.chars().take(18).collect()
}

fn with_com<T, F: FnOnce() -> T>(f: F) -> T {
    unsafe {
        let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
        let res = f();
        if hr.is_ok() {
            CoUninitialize();
        }
        res
    }
}

pub fn get_audio_state() -> AudioState {
    with_com(|| unsafe {
        let mut state = AudioState {
            master_volume: 68,
            master_muted: false,
            mic_muted: false,
            devices: vec![],
            sessions: vec![],
        };

        let enumerator: IMMDeviceEnumerator =
            match CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) {
                Ok(e) => e,
                Err(_) => return state,
            };

        let default_id = enumerator
            .GetDefaultAudioEndpoint(eRender, eConsole)
            .ok()
            .and_then(|dev| {
                let id_pw = dev.GetId().ok()?;
                let id_str = id_pw.to_string().ok()?;
                CoTaskMemFree(Some(id_pw.0 as *const _));

                if let Ok(vol) = dev.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) {
                    if let Ok(scalar) = vol.GetMasterVolumeLevelScalar() {
                        state.master_volume = (scalar * 100.0).round().clamp(0.0, 100.0) as u32;
                    }
                    if let Ok(muted) = vol.GetMute() {
                        state.master_muted = muted.as_bool();
                    }
                }

                // Per-app audio sessions
                if let Ok(mgr) = dev.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) {
                    if let Ok(sess_enum) = mgr.GetSessionEnumerator() {
                        let count = sess_enum.GetCount().unwrap_or(0);
                        let mut seen_pids = std::collections::HashSet::new();
                        for i in 0..count {
                            if let Ok(ctrl) = sess_enum.GetSession(i) {
                                if let Ok(ctrl2) = ctrl.cast::<IAudioSessionControl2>() {
                                    let pid = ctrl2.GetProcessId().unwrap_or(0);
                                    if pid == 0 || !seen_pids.insert(pid) {
                                        continue;
                                    }
                                    if let Ok(svol) = ctrl.cast::<ISimpleAudioVolume>() {
                                        let v = (svol.GetMasterVolume().unwrap_or(1.0) * 100.0)
                                            .round()
                                            .clamp(0.0, 100.0)
                                            as u32;
                                        let m = svol.GetMute().map(|b| b.as_bool()).unwrap_or(false);
                                        let name = process_exe_path(pid)
                                            .map(|p| friendly_exe_name(&p))
                                            .unwrap_or_else(|| format!("PID {pid}"));
                                        state.sessions.push(AudioSessionInfo {
                                            pid,
                                            name,
                                            volume: v,
                                            muted: m,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }

                Some(id_str)
            })
            .unwrap_or_default();

        // Mic mute status
        if let Ok(mic) = enumerator.GetDefaultAudioEndpoint(eCapture, eConsole) {
            if let Ok(vol) = mic.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) {
                if let Ok(muted) = vol.GetMute() {
                    state.mic_muted = muted.as_bool();
                }
            }
        }

        // Enumerate active render devices
        if let Ok(coll) = enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) {
            let count = coll.GetCount().unwrap_or(0);
            for i in 0..count {
                if let Ok(dev) = coll.Item(i) {
                    if let Ok(id_pw) = dev.GetId() {
                        let id = id_pw.to_string().unwrap_or_default();
                        CoTaskMemFree(Some(id_pw.0 as *const _));
                        let mut friendly = "Аудио устройство".to_string();
                        if let Ok(store) = dev.OpenPropertyStore(STGM_READ) {
                            if let Ok(pv) = store.GetValue(&PKEY_Device_FriendlyName) {
                                if let Ok(pstr) = PropVariantToStringAlloc(&pv) {
                                    if let Ok(s) = pstr.to_string() {
                                        friendly = s;
                                    }
                                    CoTaskMemFree(Some(pstr.0 as *const _));
                                }
                            }
                        }
                        let is_default = id == default_id;
                        let short_name = shorten_device_name(&friendly);
                        state.devices.push(AudioDeviceInfo {
                            id,
                            name: friendly,
                            short_name,
                            is_default,
                        });
                    }
                }
            }
        }

        state
    })
}

pub fn set_master_volume(volume_pct: u32, muted: Option<bool>) -> Result<(), String> {
    with_com(|| unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(|e| e.to_string())?;
        let dev = enumerator
            .GetDefaultAudioEndpoint(eRender, eConsole)
            .map_err(|e| e.to_string())?;
        let vol = dev
            .Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)
            .map_err(|e| e.to_string())?;
        let scalar = (volume_pct.min(100) as f32) / 100.0;
        vol.SetMasterVolumeLevelScalar(scalar, std::ptr::null())
            .map_err(|e| e.to_string())?;
        if let Some(m) = muted {
            vol.SetMute(m, std::ptr::null()).map_err(|e| e.to_string())?;
        }
        Ok(())
    })
}

pub fn set_mic_muted(muted: bool) -> Result<(), String> {
    with_com(|| unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(|e| e.to_string())?;
        let dev = enumerator
            .GetDefaultAudioEndpoint(eCapture, eConsole)
            .map_err(|e| e.to_string())?;
        let vol = dev
            .Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)
            .map_err(|e| e.to_string())?;
        vol.SetMute(muted, std::ptr::null())
            .map_err(|e| e.to_string())?;
        Ok(())
    })
}

pub fn set_session_volume(pid: u32, volume_pct: u32, muted: Option<bool>) -> Result<(), String> {
    with_com(|| unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(|e| e.to_string())?;
        let dev = enumerator
            .GetDefaultAudioEndpoint(eRender, eConsole)
            .map_err(|e| e.to_string())?;
        let mgr = dev
            .Activate::<IAudioSessionManager2>(CLSCTX_ALL, None)
            .map_err(|e| e.to_string())?;
        let sess_enum = mgr.GetSessionEnumerator().map_err(|e| e.to_string())?;
        let count = sess_enum.GetCount().unwrap_or(0);
        let scalar = (volume_pct.min(100) as f32) / 100.0;
        for i in 0..count {
            if let Ok(ctrl) = sess_enum.GetSession(i) {
                if let Ok(ctrl2) = ctrl.cast::<IAudioSessionControl2>() {
                    if ctrl2.GetProcessId().unwrap_or(0) == pid {
                        if let Ok(svol) = ctrl.cast::<ISimpleAudioVolume>() {
                            let _ = svol.SetMasterVolume(scalar, std::ptr::null());
                            if let Some(m) = muted {
                                let _ = svol.SetMute(m, std::ptr::null());
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    })
}

// Standard Windows IPolicyConfig COM interface (used by EarTrumpet / SoundSwitch)
// to switch default audio output device cleanly without launching Control Panel.
const CLSID_POLICY_CONFIG: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

#[windows::core::interface("f8679f50-850a-41cf-9c72-430f290290c8")]
unsafe trait IPolicyConfig: windows::core::IUnknown {
    fn get_mix_format(&self) -> HRESULT;
    fn get_device_format(&self) -> HRESULT;
    fn reset_device_format(&self) -> HRESULT;
    fn set_device_format(&self) -> HRESULT;
    fn get_processing_period(&self) -> HRESULT;
    fn set_processing_period(&self) -> HRESULT;
    fn get_share_mode(&self) -> HRESULT;
    fn set_share_mode(&self) -> HRESULT;
    fn get_property_value(&self) -> HRESULT;
    fn set_property_value(&self) -> HRESULT;
    fn set_default_endpoint(&self, device_id: PCWSTR, role: u32) -> HRESULT;
}

pub fn set_default_audio_device(device_id: &str) -> Result<(), String> {
    with_com(|| unsafe {
        let policy: IPolicyConfig = CoCreateInstance(&CLSID_POLICY_CONFIG, None, CLSCTX_ALL)
            .map_err(|e| format!("IPolicyConfig: {e}"))?;
        let w = wide(device_id);
        let _ = policy.set_default_endpoint(PCWSTR(w.as_ptr()), eConsole.0 as u32);
        let _ = policy.set_default_endpoint(PCWSTR(w.as_ptr()), eMultimedia.0 as u32);
        Ok(())
    })
}
