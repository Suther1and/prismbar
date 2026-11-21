use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use windows::core::{Interface, GUID, HRESULT, PCWSTR, PWSTR};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::{RPC_E_CHANGED_MODE, S_FALSE, S_OK};
use windows::Win32::Media::Audio::Endpoints::{IAudioEndpointVolume, IAudioMeterInformation};
use windows::Win32::Media::Audio::{
    eCapture, eCommunications, eConsole, eMultimedia, eRender, AudioSessionStateActive,
    AudioSessionStateExpired, EDataFlow, IAudioSessionControl2, IAudioSessionManager2, IMMDevice,
    IMMDeviceEnumerator, ISimpleAudioVolume, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
    COINIT_MULTITHREADED, STGM_READ,
};

use crate::icon_cache::{self, IconCache};
use crate::win_shell::wide;
use crate::windows_tracker::{
    extract_icon_for_path, extract_uwp_icon_by_aumid, friendly_exe_name, is_process_alive,
    process_aumid, process_exe_path,
};

pub const AUDIO_SESSION_STATE_INACTIVE: i32 = 0;
pub const AUDIO_SESSION_STATE_ACTIVE: i32 = 1;
pub const AUDIO_SESSION_STATE_EXPIRED: i32 = 2;

/// Noise floor below which hardware WASAPI peak values snap cleanly to `0.0` so silent devices
/// never churn diff checks or animate phantom VU bars.
const PEAK_NOISE_FLOOR: f32 = 0.002;

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

/// RAII wrapper around a COM-allocated `PWSTR` guaranteeing `CoTaskMemFree` even on early `?`
/// return or UTF-16 decode error (fixes COM string leak on `dev.GetId()` / `GetDisplayName()`).
struct CoTaskPwstr(PWSTR);

impl CoTaskPwstr {
    fn into_string_opt(self) -> Option<String> {
        if self.0 .0.is_null() {
            return None;
        }
        unsafe { self.0.to_string().ok() }
    }
}

impl Drop for CoTaskPwstr {
    fn drop(&mut self) {
        if !self.0 .0.is_null() {
            unsafe {
                CoTaskMemFree(Some(self.0 .0 as *const _));
            }
            self.0 = PWSTR::null();
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AudioSessionInfo {
    pub id: String,
    pub pid: u32,
    pub pids: Vec<u32>,
    pub name: String,
    pub volume: u32,
    pub muted: bool,
    pub is_active: bool,
    #[serde(default)]
    pub peak: f32,
    #[serde(default)]
    pub icon_key: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AudioState {
    pub available: bool,
    pub master_volume: u32,
    pub master_muted: bool,
    pub mic_muted: bool,
    #[serde(default)]
    pub master_peak: f32,
    #[serde(default)]
    pub mic_peak: f32,
    pub devices: Vec<AudioDeviceInfo>,
    #[serde(default)]
    pub input_devices: Vec<AudioDeviceInfo>,
    pub sessions: Vec<AudioSessionInfo>,
}

impl Default for AudioState {
    fn default() -> Self {
        Self {
            available: false,
            master_volume: 0,
            master_muted: false,
            mic_muted: false,
            master_peak: 0.0,
            mic_peak: 0.0,
            devices: Vec::new(),
            input_devices: Vec::new(),
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

/// Ultra-lightweight hardware peak snapshot polled ONLY while Sound flyout or Start audio card is visible.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AudioPeaksSnapshot {
    pub master_peak: f32,
    pub mic_peak: f32,
    pub session_peaks: Vec<(String, f32)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawAudioSession {
    pub pid: u32,
    pub exe_path: Option<String>,
    pub display_name: Option<String>,
    pub state: i32,
    pub pid_alive: bool,
    pub volume: u32,
    pub muted: bool,
    pub peak: f32,
}

/// Clamps raw WASAPI `IAudioMeterInformation::GetPeakValue()` into finite `0.0..=1.0`,
/// snapping sub-noise-floor readings (`<= 0.002`) cleanly to `0.0`.
pub fn sanitize_peak(raw: f32) -> f32 {
    if !raw.is_finite() || raw <= PEAK_NOISE_FLOOR {
        0.0
    } else {
        raw.clamp(0.0, 1.0)
    }
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

fn parse_pid_from_app_key(app_key: &str) -> Option<u32> {
    let trimmed = app_key.trim();
    let rest = trimmed
        .strip_prefix("pid:")
        .or_else(|| trimmed.strip_prefix("PID:"))?;
    rest.trim().parse::<u32>().ok().filter(|&p| p != 0)
}

/// Pure WASAPI session filter & multi-process aggregator returning `(AudioSessionInfo, Option<exe_path>)`
/// so callers with an `IconCache` can resolve per-app native icons without re-querying process paths.
fn aggregate_audio_sessions_with_exe(
    raw: Vec<RawAudioSession>,
) -> Vec<(AudioSessionInfo, Option<String>)> {
    struct GroupAcc {
        id: String,
        name: String,
        exe_path: Option<String>,
        pids: HashSet<u32>,
        active_primary_pid: Option<u32>,
        fallback_primary_pid: u32,
        has_active: bool,
        active_max_vol: u32,
        active_all_muted: bool,
        active_max_peak: f32,
        any_max_vol: u32,
        any_all_muted: bool,
        any_max_peak: f32,
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
        let peak = if s.muted { 0.0 } else { sanitize_peak(s.peak) };

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

        let clean_exe = s
            .exe_path
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(str::to_string);

        let entry = groups.entry(key.clone()).or_insert_with(|| GroupAcc {
            id: key,
            name,
            exe_path: clean_exe.clone(),
            pids: HashSet::new(),
            active_primary_pid: None,
            fallback_primary_pid: s.pid,
            has_active: false,
            active_max_vol: 0,
            active_all_muted: true,
            active_max_peak: 0.0,
            any_max_vol: 0,
            any_all_muted: true,
            any_max_peak: 0.0,
        });

        if entry.exe_path.is_none() && clean_exe.is_some() {
            entry.exe_path = clean_exe;
        }

        entry.pids.insert(s.pid);
        entry.any_max_vol = entry.any_max_vol.max(vol);
        entry.any_all_muted &= s.muted;
        entry.any_max_peak = entry.any_max_peak.max(peak);

        if is_active {
            if !entry.has_active {
                entry.has_active = true;
                entry.active_primary_pid = Some(s.pid);
                entry.active_max_vol = vol;
                entry.active_all_muted = s.muted;
                entry.active_max_peak = peak;
            } else {
                entry.active_max_vol = entry.active_max_vol.max(vol);
                entry.active_all_muted &= s.muted;
                entry.active_max_peak = entry.active_max_peak.max(peak);
            }
        }
    }

    let mut out: Vec<(AudioSessionInfo, Option<String>)> = groups
        .into_values()
        .map(|acc| {
            let mut pids: Vec<u32> = acc.pids.into_iter().collect();
            pids.sort_unstable();
            let pid = acc.active_primary_pid.unwrap_or(acc.fallback_primary_pid);
            let (volume, muted, peak) = if acc.has_active {
                (acc.active_max_vol, acc.active_all_muted, acc.active_max_peak)
            } else {
                (acc.any_max_vol, acc.any_all_muted, acc.any_max_peak)
            };
            (
                AudioSessionInfo {
                    id: acc.id,
                    pid,
                    pids,
                    name: acc.name,
                    volume,
                    muted,
                    is_active: acc.has_active,
                    peak: if muted { 0.0 } else { sanitize_peak(peak) },
                    icon_key: None,
                },
                acc.exe_path,
            )
        })
        .collect();

    out.sort_by(|(a, _), (b, _)| {
        b.is_active
            .cmp(&a.is_active)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    out
}

/// Pure WASAPI session filter & multi-process aggregator:
/// - Excludes PID 0, dead PIDs, and `AudioSessionStateExpired` (`2`) / invalid states.
/// - Groups multi-process app sessions by normalized executable basename.
/// - Aggregates volume/mute/peak preferring active (`AudioSessionStateActive`) sub-sessions when present.
pub fn aggregate_audio_sessions(raw: Vec<RawAudioSession>) -> Vec<AudioSessionInfo> {
    aggregate_audio_sessions_with_exe(raw)
        .into_iter()
        .map(|(info, _)| info)
        .collect()
}

/// Pure helper resolving per-app session icons through `IconCache` and collecting deduplicated
/// `(icon_key, source_path)` entries so `AppState.icon_sources` can re-hydrate evicted icons.
/// Checks `cache` BEFORE invoking `extract_uri` for both Win32 (`exe:`) and UWP (`aumid:`) keys
/// so steady-state polls never repeat GDI/Shell icon extraction.
fn attach_session_icons<FKey, FExtract>(
    sessions_with_exe: Vec<(AudioSessionInfo, Option<String>)>,
    cache: &mut IconCache,
    mut resolve_key_source: FKey,
    mut extract_uri: FExtract,
) -> (Vec<AudioSessionInfo>, Vec<(String, String)>)
where
    FKey: FnMut(u32, &str) -> (String, String, Option<String>),
    FExtract: FnMut(&str, Option<&str>) -> Option<String>,
{
    let mut sessions = Vec::with_capacity(sessions_with_exe.len());
    let mut sources = Vec::new();
    let mut seen_source_keys = HashSet::new();

    for (mut session, exe_opt) in sessions_with_exe {
        if let Some(exe) = exe_opt.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            let default_key = icon_cache::path_icon_key(exe);
            if cache.get(&default_key).is_some() {
                session.icon_key = Some(default_key.clone());
                if seen_source_keys.insert(default_key.clone()) {
                    sources.push((default_key, exe.to_string()));
                }
            } else {
                let (key, src, aumid_opt) = resolve_key_source(session.pid, exe);
                let cached = if cache.get(&key).is_some() {
                    true
                } else if let Some(uri) = extract_uri(exe, aumid_opt.as_deref()) {
                    cache.put(key.clone(), uri);
                    true
                } else {
                    false
                };
                if cached {
                    session.icon_key = Some(key.clone());
                    if seen_source_keys.insert(key.clone()) {
                        sources.push((key, src));
                    }
                }
            }
        }
        sessions.push(session);
    }

    (sessions, sources)
}

unsafe fn read_device_id(dev: &IMMDevice) -> Option<String> {
    let pw = dev.GetId().ok()?;
    CoTaskPwstr(pw).into_string_opt()
}

unsafe fn read_device_friendly_name(dev: &IMMDevice) -> String {
    if let Ok(store) = dev.OpenPropertyStore(STGM_READ) {
        if let Ok(pv) = store.GetValue(&PKEY_Device_FriendlyName) {
            if let Ok(pstr) = PropVariantToStringAlloc(&pv) {
                if let Some(s) = CoTaskPwstr(pstr)
                    .into_string_opt()
                    .filter(|s| !s.trim().is_empty())
                {
                    return s;
                }
            }
        }
    }
    "Аудио устройство".to_string()
}

unsafe fn read_endpoint_peak(dev: &IMMDevice, muted: bool) -> f32 {
    if muted {
        return 0.0;
    }
    dev.Activate::<IAudioMeterInformation>(CLSCTX_ALL, None)
        .ok()
        .and_then(|m| m.GetPeakValue().ok())
        .map(sanitize_peak)
        .unwrap_or(0.0)
}

unsafe fn enumerate_audio_endpoints(
    enumerator: &IMMDeviceEnumerator,
    flow: EDataFlow,
    default_id: &str,
) -> Vec<AudioDeviceInfo> {
    let mut devices = Vec::new();
    if let Ok(coll) = enumerator.EnumAudioEndpoints(flow, DEVICE_STATE_ACTIVE) {
        let count = coll.GetCount().unwrap_or(0);
        for i in 0..count {
            let Ok(dev) = coll.Item(i) else {
                continue;
            };
            let Some(id) = read_device_id(&dev) else {
                continue;
            };
            let friendly = read_device_friendly_name(&dev);
            let is_default = !default_id.is_empty() && id == default_id;
            let short_name = shorten_device_name(&friendly);
            devices.push(AudioDeviceInfo {
                id,
                name: friendly,
                short_name,
                is_default,
            });
        }
    }
    devices
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

fn collect_audio_state_raw() -> Result<(AudioState, Vec<(AudioSessionInfo, Option<String>)>), String> {
    let com = AudioComGuard::init_mta();
    if !com.is_usable() {
        return Err("Не удалось инициализировать COM для аудиоподсистемы".to_string());
    }

    unsafe {
        let mut state = AudioState::default();
        let mut sessions_with_exe: Vec<(AudioSessionInfo, Option<String>)> = Vec::new();

        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|e| format!("Не удалось создать MMDeviceEnumerator: {e}"))?;

        let default_render_id = enumerator
            .GetDefaultAudioEndpoint(eRender, eConsole)
            .ok()
            .and_then(|dev| {
                let id_str = read_device_id(&dev)?;

                if let Ok(vol) = dev.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) {
                    if let Ok(scalar) = vol.GetMasterVolumeLevelScalar() {
                        state.master_volume = (scalar * 100.0).round().clamp(0.0, 100.0) as u32;
                        state.available = true;
                    }
                    if let Ok(muted) = vol.GetMute() {
                        state.master_muted = muted.as_bool();
                    }
                }
                state.master_peak = read_endpoint_peak(&dev, state.master_muted);

                // Per-app audio sessions: collect raw live sessions, then filter & aggregate cleanly.
                let mut raw_sessions = Vec::new();
                let mut exe_memo: HashMap<u32, (bool, Option<String>)> = HashMap::new();
                if let Ok(mgr) = dev.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) {
                    if let Ok(sess_enum) = mgr.GetSessionEnumerator() {
                        let count = sess_enum.GetCount().unwrap_or(0);
                        for i in 0..count {
                            let Ok(ctrl) = sess_enum.GetSession(i) else {
                                continue;
                            };
                            let sess_state = ctrl
                                .GetState()
                                .map(|st| st.0)
                                .unwrap_or(AUDIO_SESSION_STATE_EXPIRED);
                            if sess_state == AudioSessionStateExpired.0 {
                                continue;
                            }
                            let Ok(ctrl2) = ctrl.cast::<IAudioSessionControl2>() else {
                                continue;
                            };
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
                            let Ok(svol) = ctrl.cast::<ISimpleAudioVolume>() else {
                                continue;
                            };
                            let v = (svol.GetMasterVolume().unwrap_or(1.0) * 100.0)
                                .round()
                                .clamp(0.0, 100.0) as u32;
                            let m = svol.GetMute().map(|b| b.as_bool()).unwrap_or(false);
                            let peak = if m || sess_state != AudioSessionStateActive.0 {
                                0.0
                            } else {
                                ctrl2
                                    .cast::<IAudioMeterInformation>()
                                    .ok()
                                    .and_then(|meter| meter.GetPeakValue().ok())
                                    .map(sanitize_peak)
                                    .unwrap_or(0.0)
                            };
                            let display_name = ctrl
                                .GetDisplayName()
                                .ok()
                                .and_then(|pw| CoTaskPwstr(pw).into_string_opt());
                            raw_sessions.push(RawAudioSession {
                                pid,
                                exe_path,
                                display_name,
                                state: sess_state,
                                pid_alive,
                                volume: v,
                                muted: m,
                                peak,
                            });
                        }
                    }
                }
                sessions_with_exe = aggregate_audio_sessions_with_exe(raw_sessions);

                Some(id_str)
            })
            .unwrap_or_default();

        // Default capture endpoint: mute status, live peak & default input ID
        let default_capture_id = enumerator
            .GetDefaultAudioEndpoint(eCapture, eConsole)
            .ok()
            .and_then(|mic| {
                let id_str = read_device_id(&mic);
                if let Ok(vol) = mic.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) {
                    if let Ok(muted) = vol.GetMute() {
                        state.mic_muted = muted.as_bool();
                    }
                }
                state.mic_peak = read_endpoint_peak(&mic, state.mic_muted);
                id_str
            })
            .unwrap_or_default();

        // Enumerate active render & capture devices
        state.devices = enumerate_audio_endpoints(&enumerator, eRender, &default_render_id);
        state.input_devices =
            enumerate_audio_endpoints(&enumerator, eCapture, &default_capture_id);

        Ok((state, sessions_with_exe))
    }
}

pub fn get_audio_state() -> AudioState {
    match collect_audio_state_raw() {
        Ok((mut state, sessions_with_exe)) => {
            state.sessions = sessions_with_exe.into_iter().map(|(s, _)| s).collect();
            state
        }
        Err(_) => AudioState::default(),
    }
}

/// Full WASAPI audio state + per-app native icon resolution through `IconCache`.
/// Returns `(AudioState, icon_sources)` where `icon_sources` is a deduplicated list of
/// `(icon_key, source_path)` pairs for registering in `AppState.icon_sources`.
pub fn get_audio_state_with_icons(
    cache: &mut IconCache,
) -> Result<(AudioState, Vec<(String, String)>), String> {
    let (mut state, sessions_with_exe) = collect_audio_state_raw()?;
    let (sessions, sources) = attach_session_icons(
        sessions_with_exe,
        cache,
        |pid, exe_path| {
            if let Some(aumid) = process_aumid(pid) {
                let key = icon_cache::aumid_key(&aumid);
                let src = format!("shell:AppsFolder\\{aumid}");
                (key, src, Some(aumid))
            } else {
                let key = icon_cache::path_icon_key(exe_path);
                (key, exe_path.to_string(), None)
            }
        },
        |exe_path, aumid_opt| {
            if let Some(aumid) = aumid_opt {
                extract_uwp_icon_by_aumid(aumid).or_else(|| extract_icon_for_path(exe_path))
            } else {
                extract_icon_for_path(exe_path)
            }
        },
    );
    state.sessions = sessions;
    Ok((state, sources))
}

/// Ultra-lightweight live hardware peak meter query (`IAudioMeterInformation::GetPeakValue()`).
/// Intended to be polled ONLY while the Sound flyout or Start Audio card is open (0% idle cost).
pub fn get_audio_peaks() -> Result<AudioPeaksSnapshot, String> {
    let com = AudioComGuard::init_mta();
    if !com.is_usable() {
        return Err("Не удалось инициализировать COM для индикаторов громкости".to_string());
    }

    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|e| format!("Не удалось создать MMDeviceEnumerator: {e}"))?;

        let mut master_peak = 0.0f32;
        let mut mic_peak = 0.0f32;
        let mut peak_by_app: HashMap<String, f32> = HashMap::new();

        if let Ok(dev) = enumerator.GetDefaultAudioEndpoint(eRender, eConsole) {
            let master_muted = dev
                .Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)
                .ok()
                .and_then(|v| v.GetMute().ok())
                .map(|b| b.as_bool())
                .unwrap_or(false);
            master_peak = read_endpoint_peak(&dev, master_muted);

            let mut key_memo: HashMap<u32, String> = HashMap::new();
            if let Ok(mgr) = dev.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) {
                if let Ok(sess_enum) = mgr.GetSessionEnumerator() {
                    let count = sess_enum.GetCount().unwrap_or(0);
                    for i in 0..count {
                        let Ok(ctrl) = sess_enum.GetSession(i) else {
                            continue;
                        };
                        let sess_state = ctrl
                            .GetState()
                            .map(|st| st.0)
                            .unwrap_or(AUDIO_SESSION_STATE_EXPIRED);
                        if sess_state != AudioSessionStateActive.0 {
                            continue;
                        }
                        let Ok(ctrl2) = ctrl.cast::<IAudioSessionControl2>() else {
                            continue;
                        };
                        if ctrl2.IsSystemSoundsSession() == S_OK {
                            continue;
                        }
                        let pid = ctrl2.GetProcessId().unwrap_or(0);
                        if pid == 0 {
                            continue;
                        }
                        let muted = ctrl
                            .cast::<ISimpleAudioVolume>()
                            .ok()
                            .and_then(|sv| sv.GetMute().ok())
                            .map(|b| b.as_bool())
                            .unwrap_or(false);
                        let peak = if muted {
                            0.0
                        } else {
                            ctrl2
                                .cast::<IAudioMeterInformation>()
                                .ok()
                                .and_then(|meter| meter.GetPeakValue().ok())
                                .map(sanitize_peak)
                                .unwrap_or(0.0)
                        };
                        let app_key = key_memo
                            .entry(pid)
                            .or_insert_with(|| {
                                let exe = process_exe_path(pid);
                                canonical_audio_app_key(exe.as_deref(), pid)
                            })
                            .clone();
                        let slot = peak_by_app.entry(app_key).or_insert(0.0);
                        if peak > *slot {
                            *slot = peak;
                        }
                    }
                }
            }
        }

        if let Ok(mic) = enumerator.GetDefaultAudioEndpoint(eCapture, eConsole) {
            let mic_muted = mic
                .Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)
                .ok()
                .and_then(|v| v.GetMute().ok())
                .map(|b| b.as_bool())
                .unwrap_or(false);
            mic_peak = read_endpoint_peak(&mic, mic_muted);
        }

        let mut session_peaks: Vec<(String, f32)> = peak_by_app.into_iter().collect();
        session_peaks.sort_by(|a, b| a.0.cmp(&b.0));

        Ok(AudioPeaksSnapshot {
            master_peak,
            mic_peak,
            session_peaks,
        })
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

/// Pure, testable session mutation applier:
/// - Invokes `set_volume` ONLY when `volume` is `Some(v)` (clamped to `0.0..=1.0`).
/// - Invokes `set_mute` ONLY when `muted` is `Some(m)` (never overwriting volume on mute-only toggle!).
pub fn apply_session_mutation<FVol, FMute>(
    volume: Option<f32>,
    muted: Option<bool>,
    mut set_volume: FVol,
    mut set_mute: FMute,
) -> Result<(), String>
where
    FVol: FnMut(f32) -> Result<(), String>,
    FMute: FnMut(bool) -> Result<(), String>,
{
    let mut first_err: Option<String> = None;
    if let Some(raw_vol) = volume {
        if !raw_vol.is_finite() {
            return Err("Некорректное значение громкости приложения".to_string());
        }
        let clamped = raw_vol.clamp(0.0, 1.0);
        if let Err(err) = set_volume(clamped) {
            first_err = Some(err);
        }
    }
    if let Some(m) = muted {
        if let Err(err) = set_mute(m) {
            if first_err.is_none() {
                first_err = Some(err);
            }
        }
    }
    if let Some(err) = first_err {
        Err(err)
    } else {
        Ok(())
    }
}

fn apply_session_state_internal(
    pid: u32,
    pids: Option<&[u32]>,
    app_key: Option<&str>,
    volume_scalar: Option<f32>,
    muted: Option<bool>,
) -> Result<(), String> {
    if volume_scalar.is_none() && muted.is_none() {
        return Ok(());
    }
    if let Some(v) = volume_scalar {
        if !v.is_finite() {
            return Err("Некорректное значение громкости приложения".to_string());
        }
    }

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
    if let Some(parsed_pid) = app_key.and_then(parse_pid_from_app_key) {
        target_pids.insert(parsed_pid);
    }

    let target_key = app_key
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.to_ascii_lowercase().starts_with("pid:"))
        .map(|s| s.to_ascii_lowercase())
        .or_else(|| {
            if pid != 0 {
                process_exe_path(pid).map(|p| canonical_audio_app_key(Some(&p), pid))
            } else {
                None
            }
        });

    if target_pids.is_empty() && target_key.is_none() {
        return Err("Не указано приложение для изменения звука".to_string());
    }

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
        let mut exe_key_memo: HashMap<u32, String> = HashMap::new();
        let mut first_err: Option<String> = None;

        for i in 0..count {
            let Ok(ctrl) = sess_enum.GetSession(i) else {
                continue;
            };
            let sess_state = ctrl
                .GetState()
                .map(|st| st.0)
                .unwrap_or(AUDIO_SESSION_STATE_EXPIRED);
            if sess_state == AudioSessionStateExpired.0 {
                continue;
            }
            let Ok(ctrl2) = ctrl.cast::<IAudioSessionControl2>() else {
                continue;
            };
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
                    let res = apply_session_mutation(
                        volume_scalar,
                        muted,
                        |scalar| {
                            svol.SetMasterVolume(scalar, std::ptr::null())
                                .map_err(|err| format!("Не удалось изменить громкость приложения: {err}"))
                        },
                        |m| {
                            svol.SetMute(m, std::ptr::null())
                                .map_err(|err| format!("Не удалось переключить звук приложения: {err}"))
                        },
                    );
                    if let Err(err) = res {
                        if first_err.is_none() {
                            first_err = Some(err);
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

/// Adjusts volume (`0.0..=1.0`) and/or mute for ALL live non-expired WASAPI sessions matching
/// `app_key` (exe basename like `"chrome.exe"` or `"pid:<id>"`).
/// ONLY calls `SetMasterVolume` when `volume.is_some()` and ONLY calls `SetMute` when `muted.is_some()`.
pub fn set_session_state_for_app(
    app_key: &str,
    volume: Option<f32>,
    muted: Option<bool>,
) -> Result<(), String> {
    let trimmed = app_key.trim();
    if trimmed.is_empty() {
        return Err("Не указано приложение для изменения звука".to_string());
    }
    apply_session_state_internal(0, None, Some(trimmed), volume, muted)
}

/// Optional-percentage variant of `set_session_state_for_app` supporting `pid`/`pids`/`app_key`
/// with `volume_pct: Option<u32>` (`0..=100`) so IPC handlers can mute without overwriting volume.
pub fn set_session_state_for_app_pct(
    pid: u32,
    pids: Option<&[u32]>,
    app_key: Option<&str>,
    volume_pct: Option<u32>,
    muted: Option<bool>,
) -> Result<(), String> {
    let scalar = volume_pct.map(|pct| (pct.min(100) as f32) / 100.0);
    apply_session_state_internal(pid, pids, app_key, scalar, muted)
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
    set_session_state_for_app_pct(pid, pids, app_key, Some(volume_pct), muted)
}

// Standard Windows IPolicyConfig COM interface (used by EarTrumpet / SoundSwitch)
// to switch default audio output/input devices across all 3 roles without launching Control Panel.
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

fn apply_default_endpoint_all_roles_labeled<F>(
    device_id: &str,
    direction_label: &str,
    mut set_role: F,
) -> Result<(), String>
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
                "Не удалось переключить устройство {direction_label} ({role_name}, 0x{:08X})",
                hr.0 as u32
            ));
        }
    }
    Ok(())
}

/// Pure testable helper applying `IPolicyConfig::SetDefaultEndpoint` across ALL three roles
/// (`eConsole`, `eMultimedia`, `eCommunications`) and propagating any failing `HRESULT` (AD-5).
pub fn apply_default_endpoint_all_roles<F>(device_id: &str, set_role: F) -> Result<(), String>
where
    F: FnMut(u32) -> HRESULT,
{
    apply_default_endpoint_all_roles_labeled(device_id, "вывода", set_role)
}

fn set_default_endpoint_via_policy(device_id: &str, direction_label: &str) -> Result<(), String> {
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
        apply_default_endpoint_all_roles_labeled(trimmed, direction_label, |role| {
            policy.set_default_endpoint(PCWSTR(w.as_ptr()), role)
        })
    }
}

pub fn set_default_audio_device(device_id: &str) -> Result<(), String> {
    set_default_endpoint_via_policy(device_id, "вывода")
}

/// Switches the default audio capture (microphone/input) device across all 3 Windows roles
/// (`eConsole`, `eMultimedia`, `eCommunications`) via `IPolicyConfig`.
pub fn set_default_input_device(device_id: &str) -> Result<(), String> {
    set_default_endpoint_via_policy(device_id, "ввода")
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
                peak: 0.7,
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
                peak: 0.5,
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
                peak: 0.9,
            },
            // Chrome multi-process: one inactive muted tab + two active tabs (one 65% unmuted @ 0.42 peak, one 40% muted @ 0.8)
            RawAudioSession {
                pid: 2010,
                exe_path: Some(r"C:\Program Files\Google\Chrome\Application\chrome.exe".into()),
                display_name: None,
                state: AUDIO_SESSION_STATE_INACTIVE,
                pid_alive: true,
                volume: 100,
                muted: true,
                peak: 0.9,
            },
            RawAudioSession {
                pid: 2020,
                exe_path: Some(r"C:\Program Files\Google\Chrome\Application\Chrome.EXE".into()),
                display_name: None,
                state: AUDIO_SESSION_STATE_ACTIVE,
                pid_alive: true,
                volume: 65,
                muted: false,
                peak: 0.42,
            },
            RawAudioSession {
                pid: 2030,
                exe_path: Some(r"c:/program files/google/chrome/application/chrome.exe".into()),
                display_name: None,
                state: AUDIO_SESSION_STATE_ACTIVE,
                pid_alive: true,
                volume: 40,
                muted: true,
                peak: 0.8,
            },
            // Discord multi-process: both inactive & muted -> peak 0.0
            RawAudioSession {
                pid: 3010,
                exe_path: Some(r"C:\Users\u\AppData\Local\Discord\app-1\Discord.exe".into()),
                display_name: None,
                state: AUDIO_SESSION_STATE_INACTIVE,
                pid_alive: true,
                volume: 80,
                muted: true,
                peak: 0.3,
            },
            RawAudioSession {
                pid: 3020,
                exe_path: Some(r"C:\Users\u\AppData\Local\Discord\app-1\discord.exe".into()),
                display_name: None,
                state: AUDIO_SESSION_STATE_INACTIVE,
                pid_alive: true,
                volume: 55,
                muted: true,
                peak: 0.2,
            },
            // Active Telegram with out-of-range volume (120 -> 100) and peak (1.4 -> 1.0)
            RawAudioSession {
                pid: 4001,
                exe_path: Some(r"C:\Apps\Telegram\Telegram.exe".into()),
                display_name: None,
                state: AUDIO_SESSION_STATE_ACTIVE,
                pid_alive: true,
                volume: 120,
                muted: false,
                peak: 1.4,
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
        assert!((sessions[0].peak - 0.42).abs() < 1e-6); // muted active tab peak ignored

        assert_eq!(sessions[1].name, "Telegram");
        assert_eq!(sessions[1].volume, 100);
        assert!(sessions[1].is_active);
        assert_eq!(sessions[1].peak, 1.0);

        assert_eq!(sessions[2].name, "Discord");
        assert_eq!(sessions[2].id, "discord.exe");
        assert_eq!(sessions[2].pids, vec![3010, 3020]);
        assert_eq!(sessions[2].volume, 80);
        assert!(sessions[2].muted);
        assert!(!sessions[2].is_active);
        assert_eq!(sessions[2].peak, 0.0);
    }

    #[test]
    fn sanitize_peak_clamps_bounds_noise_floor_and_non_finite() {
        assert_eq!(sanitize_peak(f32::NAN), 0.0);
        assert_eq!(sanitize_peak(f32::INFINITY), 0.0);
        assert_eq!(sanitize_peak(-0.5), 0.0);
        assert_eq!(sanitize_peak(0.001), 0.0);
        assert_eq!(sanitize_peak(0.002), 0.0);
        assert!((sanitize_peak(0.05) - 0.05).abs() < 1e-6);
        assert_eq!(sanitize_peak(1.75), 1.0);
    }

    #[test]
    fn apply_session_mutation_separates_volume_and_mute_cleanly() {
        // Mute-only toggle MUST NEVER call set_volume
        let mut vol_calls = Vec::new();
        let mut mute_calls = Vec::new();
        apply_session_mutation(
            None,
            Some(true),
            |v| {
                vol_calls.push(v);
                Ok(())
            },
            |m| {
                mute_calls.push(m);
                Ok(())
            },
        )
        .expect("mute-only must succeed");
        assert!(vol_calls.is_empty(), "mute toggle must not overwrite volume");
        assert_eq!(mute_calls, vec![true]);

        // Volume-only drag MUST NEVER call set_mute
        vol_calls.clear();
        mute_calls.clear();
        apply_session_mutation(
            Some(0.65),
            None,
            |v| {
                vol_calls.push(v);
                Ok(())
            },
            |m| {
                mute_calls.push(m);
                Ok(())
            },
        )
        .expect("volume-only must succeed");
        assert_eq!(vol_calls, vec![0.65]);
        assert!(mute_calls.is_empty(), "volume-only must not touch mute");

        // Both provided clamps volume to 0.0..=1.0
        vol_calls.clear();
        mute_calls.clear();
        apply_session_mutation(
            Some(1.5),
            Some(false),
            |v| {
                vol_calls.push(v);
                Ok(())
            },
            |m| {
                mute_calls.push(m);
                Ok(())
            },
        )
        .expect("combined mutation must succeed");
        assert_eq!(vol_calls, vec![1.0]);
        assert_eq!(mute_calls, vec![false]);

        // Non-finite volume is rejected before touching mute
        mute_calls.clear();
        assert!(apply_session_mutation(
            Some(f32::NAN),
            Some(true),
            |_| Ok(()),
            |m| {
                mute_calls.push(m);
                Ok(())
            },
        )
        .is_err());
        assert!(mute_calls.is_empty());

        // Empty app_key rejected in set_session_state_for_app
        assert!(set_session_state_for_app("   ", None, Some(true)).is_err());
        assert_eq!(parse_pid_from_app_key("pid:4242"), Some(4242));
        assert_eq!(parse_pid_from_app_key("chrome.exe"), None);
    }

    #[test]
    fn attach_session_icons_populates_cache_reuses_hits_and_deduplicates_sources() {
        let mut cache = IconCache::new(16);
        let pre_exe = r"C:\Apps\Spotify\Spotify.exe";
        let pre_key = icon_cache::path_icon_key(pre_exe);
        cache.put(pre_key.clone(), "data:image/png;base64,PRE".into());

        let raw = vec![
            RawAudioSession {
                pid: 100,
                exe_path: Some(pre_exe.into()),
                display_name: None,
                state: AUDIO_SESSION_STATE_ACTIVE,
                pid_alive: true,
                volume: 80,
                muted: false,
                peak: 0.25,
            },
            RawAudioSession {
                pid: 200,
                exe_path: Some(r"C:\Apps\Discord\Discord.exe".into()),
                display_name: None,
                state: AUDIO_SESSION_STATE_ACTIVE,
                pid_alive: true,
                volume: 70,
                muted: false,
                peak: 0.1,
            },
        ];
        let grouped = aggregate_audio_sessions_with_exe(raw.clone());

        let mut extract_calls = 0usize;
        let (sessions, sources) = attach_session_icons(
            grouped,
            &mut cache,
            |pid, exe| {
                if pid == 200 {
                    // Simulate UWP app keyed by AUMID
                    let aumid = "Discord.UWP!App".to_string();
                    (
                        icon_cache::aumid_key(&aumid),
                        format!("shell:AppsFolder\\{aumid}"),
                        Some(aumid),
                    )
                } else {
                    (icon_cache::path_icon_key(exe), exe.to_string(), None)
                }
            },
            |_exe, _aumid| {
                extract_calls += 1;
                Some("data:image/png;base64,NEW".into())
            },
        );

        // Spotify hit exe cache (0 extracts); UWP Discord extracted once
        assert_eq!(extract_calls, 1);
        assert_eq!(sessions.len(), 2);
        let discord = sessions.iter().find(|s| s.id == "discord.exe").unwrap();
        let spotify = sessions.iter().find(|s| s.id == "spotify.exe").unwrap();
        assert_eq!(spotify.icon_key.as_deref(), Some(pre_key.as_str()));
        assert!(discord.icon_key.as_deref().is_some_and(|k| k.starts_with("aumid:")));
        assert_eq!(sources.len(), 2);
        assert!(cache.contains_key(discord.icon_key.as_deref().unwrap()));

        // Second poll: BOTH Win32 (exe:) and UWP (aumid:) hit cache with 0 icon extractions!
        let grouped2 = aggregate_audio_sessions_with_exe(raw);
        let (sessions2, _) = attach_session_icons(
            grouped2,
            &mut cache,
            |pid, exe| {
                if pid == 200 {
                    let aumid = "Discord.UWP!App".to_string();
                    (
                        icon_cache::aumid_key(&aumid),
                        format!("shell:AppsFolder\\{aumid}"),
                        Some(aumid),
                    )
                } else {
                    (icon_cache::path_icon_key(exe), exe.to_string(), None)
                }
            },
            |_exe, _aumid| {
                extract_calls += 1;
                Some("data:image/png;base64,SHOULD_NOT_RUN".into())
            },
        );
        assert_eq!(extract_calls, 1, "steady-state UWP & Win32 polls must hit IconCache");
        assert_eq!(sessions2.len(), 2);
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

        // Input device empty-ID guard also rejects cleanly
        assert!(set_default_input_device("   ").is_err());
    }

    #[test]
    fn default_audio_state_uses_zero_not_fake_68_and_projects_master_state() {
        let def = AudioState::default();
        assert!(!def.available);
        assert_eq!(def.master_volume, 0);
        assert!(!def.master_muted);
        assert!(!def.mic_muted);
        assert_eq!(def.master_peak, 0.0);
        assert_eq!(def.mic_peak, 0.0);
        assert!(def.input_devices.is_empty());

        let full = AudioState {
            available: true,
            master_volume: 44,
            master_muted: true,
            mic_muted: true,
            master_peak: 0.0,
            mic_peak: 0.0,
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
            input_devices: vec![AudioDeviceInfo {
                id: "m1".into(),
                name: "USB Mic (FIFINE)".into(),
                short_name: "USB Mic".into(),
                is_default: true,
            }],
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

        // Serde backward compatibility: legacy JSON missing new fields deserializes cleanly
        let legacy_json = r#"{"available":true,"masterVolume":50,"masterMuted":false,"micMuted":false,"devices":[],"sessions":[{"id":"a.exe","pid":1,"pids":[1],"name":"A","volume":50,"muted":false,"isActive":true}]}"#;
        let parsed: AudioState = serde_json::from_str(legacy_json).expect("legacy json");
        assert_eq!(parsed.master_peak, 0.0);
        assert_eq!(parsed.mic_peak, 0.0);
        assert!(parsed.input_devices.is_empty());
        assert_eq!(parsed.sessions[0].peak, 0.0);
        assert_eq!(parsed.sessions[0].icon_key, None);
    }

    #[test]
    fn real_audio_peaks_and_state_with_icons_probe_cleanly() {
        let mut cache = IconCache::new(64);
        let (st, _sources) = get_audio_state_with_icons(&mut cache)
            .expect("get_audio_state_with_icons must succeed on Windows");
        assert!((0.0..=1.0).contains(&st.master_peak));
        assert!((0.0..=1.0).contains(&st.mic_peak));
        for s in &st.sessions {
            assert!((0.0..=1.0).contains(&s.peak));
        }

        let peaks = get_audio_peaks().expect("get_audio_peaks must succeed on Windows");
        assert!((0.0..=1.0).contains(&peaks.master_peak));
        assert!((0.0..=1.0).contains(&peaks.mic_peak));
        for (_, p) in &peaks.session_peaks {
            assert!((0.0..=1.0).contains(p));
        }
    }
}
