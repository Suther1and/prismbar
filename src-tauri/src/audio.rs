use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use windows::core::{GUID, HRESULT, Interface, PCWSTR};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::{RPC_E_CHANGED_MODE, S_FALSE, S_OK};
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{
    eCapture, eCommunications, eConsole, eMultimedia, eRender, AudioSessionStateExpired,
    IAudioSessionControl2, IAudioSessionManager2, IMMDevice, IMMDeviceEnumerator,
    ISimpleAudioVolume, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
    COINIT_MULTITHREADED, STGM_READ,
};

use crate::win_shell::wide;
use crate::windows_tracker::{friendly_exe_name, is_process_alive, process_exe_path};

pub const AUDIO_SESSION_STATE_INACTIVE: i32 = 0;
pub const AUDIO_SESSION_STATE_ACTIVE: i32 = 1;
pub const AUDIO_SESSION_STATE_EXPIRED: i32 = 2;

/// All three Windows audio endpoint roles that `IPolicyConfig::SetDefaultEndpoint` must update (AD-5).
pub const POLICY_CONFIG_ROLES: [(&str, u32); 3] = [
    ("eConsole", eConsole.0 as u32),
    ("eMultimedia", eMultimedia.0 as u32),
    ("eCommunications", eCommunications.0 as u32),
];

/// Decides whether `CoUninitialize` must be called for a `CoInitializeEx` result (AD-5).
/// Only `S_OK` and `S_FALSE` incremented the thread's COM init count; `RPC_E_CHANGED_MODE`
/// and failure HRESULTs MUST NEVER call `CoUninitialize`.
pub fn should_uninitialize_com(hr: HRESULT) -> bool {
    hr == S_OK || hr == S_FALSE
}

/// RAII COM apartment guard for WASAPI/IPolicyConfig honoring `CoInitializeEx` HRESULT semantics (AD-5).
pub struct AudioComGuard {
    should_uninit: bool,
    usable: bool,
}

impl AudioComGuard {
    pub fn init_mta() -> Self {
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        Self::from_hresult(hr)
    }

    pub(crate) fn from_hresult(hr: HRESULT) -> Self {
        let should_uninit = should_uninitialize_com(hr);
        let usable = should_uninit || hr == RPC_E_CHANGED_MODE;
        Self {
            should_uninit,
            usable,
        }
    }

    pub fn is_usable(&self) -> bool {
        self.usable
    }

    pub fn will_uninitialize(&self) -> bool {
        self.should_uninit
    }
}

impl Drop for AudioComGuard {
    fn drop(&mut self) {
        if self.should_uninit {
            unsafe {
                CoUninitialize();
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AudioDeviceInfo {
    pub id: String,
    pub name: String,
    pub short_name: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AudioSessionInfo {
    pub id: String,
    pub pid: u32,
    pub pids: Vec<u32>,
    pub name: String,
    pub volume: u32,
    pub muted: bool,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AudioMasterState {
    pub available: bool,
    pub master_volume: u32,
    pub master_muted: bool,
    pub mic_muted: bool,
    pub default_device_short_name: String,
}

impl Default for AudioMasterState {
    fn default() -> Self {
        Self {
            available: false,
            master_volume: 0,
            master_muted: false,
            mic_muted: false,
            default_device_short_name: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AudioState {
    pub available: bool,
    pub master_volume: u32,
    pub master_muted: bool,
    pub mic_muted: bool,
    pub devices: Vec<AudioDeviceInfo>,
    pub sessions: Vec<AudioSessionInfo>,
}

impl Default for AudioState {
    fn default() -> Self {
        Self {
            available: false,
            master_volume: 0,
            master_muted: false,
            mic_muted: false,
            devices: Vec::new(),
            sessions: Vec::new(),
        }
    }
}

impl AudioState {
    pub fn to_master_state(&self) -> AudioMasterState {
        let default_device_short_name = self
            .devices
            .iter()
            .find(|d| d.is_default)
            .map(|d| d.short_name.clone())
            .unwrap_or_default();
        AudioMasterState {
            available: self.available,
            master_volume: self.master_volume,
            master_muted: self.master_muted,
            mic_muted: self.mic_muted,
            default_device_short_name,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawAudioSession {
    pub pid: u32,
    pub exe_path: Option<String>,
    pub display_name: Option<String>,
    pub state: i32,
    pub pid_alive: bool,
    pub volume: u32,
    pub muted: bool,
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

/// Computes canonical lowercase app key for WASAPI session deduplication/grouping
/// (merging multi-process apps such as Chrome, Discord, Edge, or Zen into one mixer slider).
pub fn canonical_audio_app_key(exe_path: Option<&str>, pid: u32) -> String {
    if let Some(path) = exe_path.map(str::trim).filter(|s| !s.is_empty()) {
        let norm = path.replace('/', "\\").to_ascii_lowercase();
        let base = norm.rsplit('\\').next().unwrap_or(&norm).trim();
        if !base.is_empty() {
            return base.to_string();
        }
        return norm;
    }
    format!("pid:{pid}")
}

/// Pure WASAPI session filter & multi-process aggregator:
/// - Excludes PID 0, dead PIDs, and `AudioSessionStateExpired` (`2`) / invalid states.
/// - Groups multi-process app sessions by normalized executable basename.
/// - Aggregates volume/mute preferring active (`AudioSessionStateActive`) sub-sessions when present.
pub fn aggregate_audio_sessions(raw: Vec<RawAudioSession>) -> Vec<AudioSessionInfo> {
    struct GroupAcc {
        id: String,
        name: String,
        pids: HashSet<u32>,
        active_primary_pid: Option<u32>,
        fallback_primary_pid: u32,
        has_active: bool,
        active_max_vol: u32,
        active_all_muted: bool,
        any_max_vol: u32,
        any_all_muted: bool,
    }

    let mut groups: HashMap<String, GroupAcc> = HashMap::new();

    for s in raw {
        if s.pid == 0 || !s.pid_alive {
            continue;
        }
        if s.state == AUDIO_SESSION_STATE_EXPIRED
            || (s.state != AUDIO_SESSION_STATE_INACTIVE && s.state != AUDIO_SESSION_STATE_ACTIVE)
        {
            continue;
        }

        let key = canonical_audio_app_key(s.exe_path.as_deref(), s.pid);
        let is_active = s.state == AUDIO_SESSION_STATE_ACTIVE;
        let vol = s.volume.min(100);

        let name = s
            .exe_path
            .as_deref()
            .filter(|p| !p.trim().is_empty())
            .map(friendly_exe_name)
            .or_else(|| {
                s.display_name
                    .as_deref()
                    .map(str::trim)
                    .filter(|d| !d.is_empty() && !d.starts_with('@'))
                    .map(str::to_string)
            })
            .unwrap_or_else(|| format!("PID {}", s.pid));

        let entry = groups.entry(key.clone()).or_insert_with(|| GroupAcc {
            id: key,
            name,
            pids: HashSet::new(),
            active_primary_pid: None,
            fallback_primary_pid: s.pid,
            has_active: false,
            active_max_vol: 0,
            active_all_muted: true,
            any_max_vol: 0,
            any_all_muted: true,
        });

        entry.pids.insert(s.pid);
        entry.any_max_vol = entry.any_max_vol.max(vol);
        entry.any_all_muted &= s.muted;

        if is_active {
            if !entry.has_active {
                entry.has_active = true;
                entry.active_primary_pid = Some(s.pid);
                entry.active_max_vol = vol;
                entry.active_all_muted = s.muted;
            } else {
                entry.active_max_vol = entry.active_max_vol.max(vol);
                entry.active_all_muted &= s.muted;
            }
        }
    }

    let mut out: Vec<AudioSessionInfo> = groups
        .into_values()
        .map(|acc| {
            let mut pids: Vec<u32> = acc.pids.into_iter().collect();
            pids.sort_unstable();
            let pid = acc.active_primary_pid.unwrap_or(acc.fallback_primary_pid);
            let (volume, muted) = if acc.has_active {
                (acc.active_max_vol, acc.active_all_muted)
            } else {
                (acc.any_max_vol, acc.any_all_muted)
            };
            AudioSessionInfo {
                id: acc.id,
                pid,
                pids,
                name: acc.name,
                volume,
                muted,
                is_active: acc.has_active,
            }
        })
        .collect();

    out.sort_by(|a, b| {
        b.is_active
            .cmp(&a.is_active)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    out
}

unsafe fn read_device_friendly_name(dev: &IMMDevice) -> String {
    if let Ok(store) = dev.OpenPropertyStore(STGM_READ) {
        if let Ok(pv) = store.GetValue(&PKEY_Device_FriendlyName) {
            if let Ok(pstr) = PropVariantToStringAlloc(&pv) {
                let name = pstr.to_string().ok();
                CoTaskMemFree(Some(pstr.0 as *const _));
                if let Some(s) = name.filter(|s| !s.trim().is_empty()) {
                    return s;
                }
            }
        }
    }
    "Аудио устройство".to_string()
}

/// Lightweight WASAPI master-only query (zero per-app session enumeration, zero device list scan)
/// for low-overhead `audio-master-updated` polling when neither Start nor Quick Sound is open (AD-6).
pub fn get_audio_master_state() -> AudioMasterState {
    let com = AudioComGuard::init_mta();
    if !com.is_usable() {
        return AudioMasterState::default();
    }

    unsafe {
        let mut state = AudioMasterState::default();
        let enumerator: IMMDeviceEnumerator =
            match CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) {
                Ok(e) => e,
                Err(_) => return state,
            };

        if let Ok(dev) = enumerator.GetDefaultAudioEndpoint(eRender, eConsole) {
            let friendly = read_device_friendly_name(&dev);
            state.default_device_short_name = shorten_device_name(&friendly);
            if let Ok(vol) = dev.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) {
                if let Ok(scalar) = vol.GetMasterVolumeLevelScalar() {
                    state.master_volume = (scalar * 100.0).round().clamp(0.0, 100.0) as u32;
                    state.available = true;
                }
                if let Ok(muted) = vol.GetMute() {
                    state.master_muted = muted.as_bool();
                }
            }
        }

        if let Ok(mic) = enumerator.GetDefaultAudioEndpoint(eCapture, eConsole) {
            if let Ok(vol) = mic.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) {
                if let Ok(muted) = vol.GetMute() {
                    state.mic_muted = muted.as_bool();
                }
            }
        }

        state
    }
}

pub fn get_audio_state() -> AudioState {
    let com = AudioComGuard::init_mta();
    if !com.is_usable() {
        return AudioState::default();
    }

    unsafe {
        let mut state = AudioState::default();

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
                        state.available = true;
                    }
                    if let Ok(muted) = vol.GetMute() {
                        state.master_muted = muted.as_bool();
                    }
                }

                // Per-app audio sessions: collect raw live sessions, then filter & aggregate cleanly.
                let mut raw_sessions = Vec::new();
                let mut exe_memo: HashMap<u32, (bool, Option<String>)> = HashMap::new();
                if let Ok(mgr) = dev.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) {
                    if let Ok(sess_enum) = mgr.GetSessionEnumerator() {
                        let count = sess_enum.GetCount().unwrap_or(0);
                        for i in 0..count {
                            let Ok(ctrl) = sess_enum.GetSession(i) else { continue };
                            let sess_state = ctrl.GetState().map(|st| st.0).unwrap_or(AUDIO_SESSION_STATE_EXPIRED);
                            if sess_state == AudioSessionStateExpired.0 {
                                continue;
                            }
                            let Ok(ctrl2) = ctrl.cast::<IAudioSessionControl2>() else { continue };
                            if ctrl2.IsSystemSoundsSession() == S_OK {
                                continue;
                            }
                            let pid = ctrl2.GetProcessId().unwrap_or(0);
                            if pid == 0 {
                                continue;
                            }
                            let (pid_alive, exe_path) = exe_memo
                                .entry(pid)
                                .or_insert_with(|| {
                                    let alive = is_process_alive(pid);
                                    let exe = if alive { process_exe_path(pid) } else { None };
                                    (alive, exe)
                                })
                                .clone();
                            if !pid_alive {
                                continue;
                            }
                            let Ok(svol) = ctrl.cast::<ISimpleAudioVolume>() else { continue };
                            let v = (svol.GetMasterVolume().unwrap_or(1.0) * 100.0)
                                .round()
                                .clamp(0.0, 100.0) as u32;
                            let m = svol.GetMute().map(|b| b.as_bool()).unwrap_or(false);
                            let display_name = ctrl.GetDisplayName().ok().and_then(|pw| {
                                let s = pw.to_string().ok();
                                CoTaskMemFree(Some(pw.0 as *const _));
                                s
                            });
                            raw_sessions.push(RawAudioSession {
                                pid,
                                exe_path,
                                display_name,
                                state: sess_state,
                                pid_alive,
                                volume: v,
                                muted: m,
                            });
                        }
                    }
                }
                state.sessions = aggregate_audio_sessions(raw_sessions);

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
                        let friendly = read_device_friendly_name(&dev);
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
    }
}

pub fn set_master_volume(volume_pct: u32, muted: Option<bool>) -> Result<(), String> {
    let com = AudioComGuard::init_mta();
    if !com.is_usable() {
        return Err("Не удалось инициализировать COM для аудиоподсистемы".to_string());
    }
    unsafe {
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
    }
}

pub fn set_mic_muted(muted: bool) -> Result<(), String> {
    let com = AudioComGuard::init_mta();
    if !com.is_usable() {
        return Err("Не удалось инициализировать COM для микрофона".to_string());
    }
    unsafe {
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
    }
}

/// Adjusts volume/mute for ALL live non-expired WASAPI sessions belonging to a single or
/// multi-process application (matching by `pids`, `pid`, or normalized `app_key` / exe basename).
pub fn set_session_volume_for_app(
    pid: u32,
    pids: Option<&[u32]>,
    app_key: Option<&str>,
    volume_pct: u32,
    muted: Option<bool>,
) -> Result<(), String> {
    let com = AudioComGuard::init_mta();
    if !com.is_usable() {
        return Err("Не удалось инициализировать COM для микшера".to_string());
    }

    let mut target_pids = HashSet::new();
    if pid != 0 {
        target_pids.insert(pid);
    }
    if let Some(list) = pids {
        for &p in list {
            if p != 0 {
                target_pids.insert(p);
            }
        }
    }

    let target_key = app_key
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.starts_with("pid:"))
        .map(|s| s.to_ascii_lowercase())
        .or_else(|| {
            if pid != 0 {
                process_exe_path(pid).map(|p| canonical_audio_app_key(Some(&p), pid))
            } else {
                None
            }
        });

    unsafe {
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
        let mut exe_key_memo: HashMap<u32, String> = HashMap::new();
        let mut first_err: Option<String> = None;

        for i in 0..count {
            let Ok(ctrl) = sess_enum.GetSession(i) else { continue };
            let sess_state = ctrl.GetState().map(|st| st.0).unwrap_or(AUDIO_SESSION_STATE_EXPIRED);
            if sess_state == AudioSessionStateExpired.0 {
                continue;
            }
            let Ok(ctrl2) = ctrl.cast::<IAudioSessionControl2>() else { continue };
            let sess_pid = ctrl2.GetProcessId().unwrap_or(0);
            if sess_pid == 0 {
                continue;
            }

            let mut matched = target_pids.contains(&sess_pid);
            if !matched {
                if let Some(ref want_key) = target_key {
                    let sess_key = exe_key_memo.entry(sess_pid).or_insert_with(|| {
                        let exe = process_exe_path(sess_pid);
                        canonical_audio_app_key(exe.as_deref(), sess_pid)
                    });
                    matched = sess_key == want_key;
                }
            }

            if matched {
                if let Ok(svol) = ctrl.cast::<ISimpleAudioVolume>() {
                    if let Err(err) = svol.SetMasterVolume(scalar, std::ptr::null()) {
                        if first_err.is_none() {
                            first_err = Some(format!("Не удалось изменить громкость приложения: {err}"));
                        }
                    }
                    if let Some(m) = muted {
                        if let Err(err) = svol.SetMute(m, std::ptr::null()) {
                            if first_err.is_none() {
                                first_err = Some(format!("Не удалось переключить звук приложения: {err}"));
                            }
                        }
                    }
                }
            }
        }

        if let Some(err) = first_err {
            return Err(err);
        }
        Ok(())
    }
}

// Standard Windows IPolicyConfig COM interface (used by EarTrumpet / SoundSwitch)
// to switch default audio output device across all 3 roles without launching Control Panel.
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

/// Pure testable helper applying `IPolicyConfig::SetDefaultEndpoint` across ALL three roles
/// (`eConsole`, `eMultimedia`, `eCommunications`) and propagating any failing `HRESULT` (AD-5).
pub fn apply_default_endpoint_all_roles<F>(device_id: &str, mut set_role: F) -> Result<(), String>
where
    F: FnMut(u32) -> HRESULT,
{
    let trimmed = device_id.trim();
    if trimmed.is_empty() {
        return Err("Не указано аудиоустройство".to_string());
    }
    for (role_name, role_id) in POLICY_CONFIG_ROLES {
        let hr = set_role(role_id);
        if hr.is_err() {
            return Err(format!(
                "Не удалось переключить устройство вывода ({role_name}, 0x{:08X})",
                hr.0 as u32
            ));
        }
    }
    Ok(())
}

pub fn set_default_audio_device(device_id: &str) -> Result<(), String> {
    let trimmed = device_id.trim();
    if trimmed.is_empty() {
        return Err("Не указано аудиоустройство".to_string());
    }
    let com = AudioComGuard::init_mta();
    if !com.is_usable() {
        return Err("Не удалось инициализировать COM для переключения аудио".to_string());
    }
    unsafe {
        let policy: IPolicyConfig = CoCreateInstance(&CLSID_POLICY_CONFIG, None, CLSCTX_ALL)
            .map_err(|e| format!("Не удалось открыть IPolicyConfig: {e}"))?;
        let w = wide(trimmed);
        apply_default_endpoint_all_roles(trimmed, |role| {
            policy.set_default_endpoint(PCWSTR(w.as_ptr()), role)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::{E_ACCESSDENIED, E_FAIL};
    use windows::Win32::Media::Audio::{AudioSessionStateActive, AudioSessionStateInactive};

    #[test]
    fn audio_session_state_constants_match_win32_wasapi_enum() {
        assert_eq!(AUDIO_SESSION_STATE_INACTIVE, AudioSessionStateInactive.0);
        assert_eq!(AUDIO_SESSION_STATE_ACTIVE, AudioSessionStateActive.0);
        assert_eq!(AUDIO_SESSION_STATE_EXPIRED, AudioSessionStateExpired.0);
    }

    #[test]
    fn com_guard_hresult_matrix_and_rpc_e_changed_mode_safety() {
        let ok = AudioComGuard::from_hresult(S_OK);
        assert!(ok.is_usable());
        assert!(ok.will_uninitialize());
        // Prevent real CoUninitialize in pure unit test by mutating before drop
        let mut ok_no_drop = ok;
        ok_no_drop.should_uninit = false;

        let already = AudioComGuard::from_hresult(S_FALSE);
        assert!(already.is_usable());
        assert!(already.will_uninitialize());
        let mut already_no_drop = already;
        already_no_drop.should_uninit = false;

        // RPC_E_CHANGED_MODE: usable, MUST NOT call CoUninitialize (AD-5)
        let changed = AudioComGuard::from_hresult(RPC_E_CHANGED_MODE);
        assert!(changed.is_usable());
        assert!(!changed.will_uninitialize());
        drop(changed);

        // Other error HRESULT: neither usable nor uninitialized
        let failed = AudioComGuard::from_hresult(E_FAIL);
        assert!(!failed.is_usable());
        assert!(!failed.will_uninitialize());
        drop(failed);

        assert!(should_uninitialize_com(S_OK));
        assert!(should_uninitialize_com(S_FALSE));
        assert!(!should_uninitialize_com(RPC_E_CHANGED_MODE));
        assert!(!should_uninitialize_com(E_FAIL));
    }

    #[test]
    fn aggregate_audio_sessions_filters_expired_dead_and_groups_multi_process_apps() {
        let raw = vec![
            // Expired session -> excluded
            RawAudioSession {
                pid: 1001,
                exe_path: Some(r"C:\Apps\Spotify\Spotify.exe".into()),
                display_name: None,
                state: AUDIO_SESSION_STATE_EXPIRED,
                pid_alive: true,
                volume: 90,
                muted: false,
            },
            // Dead PID -> excluded
            RawAudioSession {
                pid: 1002,
                exe_path: Some(r"C:\Apps\VLC\vlc.exe".into()),
                display_name: None,
                state: AUDIO_SESSION_STATE_ACTIVE,
                pid_alive: false,
                volume: 75,
                muted: false,
            },
            // PID 0 -> excluded
            RawAudioSession {
                pid: 0,
                exe_path: None,
                display_name: Some("System".into()),
                state: AUDIO_SESSION_STATE_ACTIVE,
                pid_alive: true,
                volume: 100,
                muted: false,
            },
            // Chrome multi-process: one inactive muted tab + two active tabs (one 65% unmuted, one 40% muted)
            RawAudioSession {
                pid: 2010,
                exe_path: Some(r"C:\Program Files\Google\Chrome\Application\chrome.exe".into()),
                display_name: None,
                state: AUDIO_SESSION_STATE_INACTIVE,
                pid_alive: true,
                volume: 100,
                muted: true,
            },
            RawAudioSession {
                pid: 2020,
                exe_path: Some(r"C:\Program Files\Google\Chrome\Application\Chrome.EXE".into()),
                display_name: None,
                state: AUDIO_SESSION_STATE_ACTIVE,
                pid_alive: true,
                volume: 65,
                muted: false,
            },
            RawAudioSession {
                pid: 2030,
                exe_path: Some(r"c:/program files/google/chrome/application/chrome.exe".into()),
                display_name: None,
                state: AUDIO_SESSION_STATE_ACTIVE,
                pid_alive: true,
                volume: 40,
                muted: true,
            },
            // Discord multi-process: both inactive -> grouped, sorted after active Chrome
            RawAudioSession {
                pid: 3010,
                exe_path: Some(r"C:\Users\u\AppData\Local\Discord\app-1\Discord.exe".into()),
                display_name: None,
                state: AUDIO_SESSION_STATE_INACTIVE,
                pid_alive: true,
                volume: 80,
                muted: true,
            },
            RawAudioSession {
                pid: 3020,
                exe_path: Some(r"C:\Users\u\AppData\Local\Discord\app-1\discord.exe".into()),
                display_name: None,
                state: AUDIO_SESSION_STATE_INACTIVE,
                pid_alive: true,
                volume: 55,
                muted: true,
            },
            // Active Telegram
            RawAudioSession {
                pid: 4001,
                exe_path: Some(r"C:\Apps\Telegram\Telegram.exe".into()),
                display_name: None,
                state: AUDIO_SESSION_STATE_ACTIVE,
                pid_alive: true,
                volume: 120, // clamps to 100
                muted: false,
            },
        ];

        let sessions = aggregate_audio_sessions(raw);
        assert_eq!(sessions.len(), 3);

        // Active sessions come first, sorted alphabetically: Chrome, Telegram, then inactive Discord
        assert_eq!(sessions[0].name, "Chrome");
        assert_eq!(sessions[0].id, "chrome.exe");
        assert_eq!(sessions[0].pid, 2020);
        assert_eq!(sessions[0].pids, vec![2010, 2020, 2030]);
        assert_eq!(sessions[0].volume, 65); // active max, NOT inactive 100
        assert!(!sessions[0].muted); // one active tab is unmuted
        assert!(sessions[0].is_active);

        assert_eq!(sessions[1].name, "Telegram");
        assert_eq!(sessions[1].volume, 100);
        assert!(sessions[1].is_active);

        assert_eq!(sessions[2].name, "Discord");
        assert_eq!(sessions[2].id, "discord.exe");
        assert_eq!(sessions[2].pids, vec![3010, 3020]);
        assert_eq!(sessions[2].volume, 80);
        assert!(sessions[2].muted);
        assert!(!sessions[2].is_active);
    }

    #[test]
    fn apply_default_endpoint_all_roles_switches_all_three_and_propagates_hresult_errors() {
        let mut called = Vec::new();
        let res = apply_default_endpoint_all_roles("{dev-1}", |role| {
            called.push(role);
            S_OK
        });
        assert!(res.is_ok());
        assert_eq!(
            called,
            vec![eConsole.0 as u32, eMultimedia.0 as u32, eCommunications.0 as u32]
        );

        // Empty device ID rejected before touching COM
        let empty = apply_default_endpoint_all_roles("   ", |_| {
            panic!("must not call set_role for empty id");
        });
        assert!(empty.is_err());

        // Failure on eCommunications (3rd role) MUST propagate as Err with role & hex HRESULT
        let mut seen = Vec::new();
        let err = apply_default_endpoint_all_roles("{dev-2}", |role| {
            seen.push(role);
            if role == eCommunications.0 as u32 {
                E_ACCESSDENIED
            } else {
                S_OK
            }
        })
        .unwrap_err();
        assert_eq!(
            seen,
            vec![eConsole.0 as u32, eMultimedia.0 as u32, eCommunications.0 as u32]
        );
        assert!(err.contains("eCommunications"), "error should mention role: {err}");
        assert!(err.contains("80070005"), "error should include hex HRESULT: {err}");
    }

    #[test]
    fn default_audio_state_uses_zero_not_fake_68_and_projects_master_state() {
        let def = AudioState::default();
        assert!(!def.available);
        assert_eq!(def.master_volume, 0);
        assert!(!def.master_muted);
        assert!(!def.mic_muted);

        let full = AudioState {
            available: true,
            master_volume: 44,
            master_muted: true,
            mic_muted: true,
            devices: vec![
                AudioDeviceInfo {
                    id: "d1".into(),
                    name: "Speakers (Realtek)".into(),
                    short_name: "Speakers".into(),
                    is_default: false,
                },
                AudioDeviceInfo {
                    id: "d2".into(),
                    name: "AirPods Pro (Bluetooth)".into(),
                    short_name: "AirPods Pro".into(),
                    is_default: true,
                },
            ],
            sessions: vec![],
        };
        let master = full.to_master_state();
        assert_eq!(
            master,
            AudioMasterState {
                available: true,
                master_volume: 44,
                master_muted: true,
                mic_muted: true,
                default_device_short_name: "AirPods Pro".into(),
            }
        );
    }
}
