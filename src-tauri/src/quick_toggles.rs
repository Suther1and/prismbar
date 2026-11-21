use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Mutex, OnceLock};
use std::thread::JoinHandle;
use windows::core::{GUID, PCWSTR};
use windows::Win32::Foundation::{LocalFree, ERROR_SUCCESS, HLOCAL, HWND};
use windows::Win32::System::Power::{
    PowerGetActiveScheme, PowerSetActiveScheme, SetThreadExecutionState, ES_CONTINUOUS,
    ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED,
};
use windows::Win32::UI::Shell::{
    SHEmptyRecycleBinW, SHQueryRecycleBinW, SHERB_NOCONFIRMATION, SHERB_NOPROGRESSUI,
    SHERB_NOSOUND, SHQUERYRBINFO,
};

// Standard Windows Power Plans
const GUID_BALANCED: GUID = GUID::from_u128(0x381b4222_f694_41f0_9685_ff5bb260df2e);
const GUID_HIGH_PERF: GUID = GUID::from_u128(0x8c5e7fda_e8bf_4a96_9a85_a6e23a8c635c);
const GUID_POWER_SAVER: GUID = GUID::from_u128(0xa1841308_3541_4fab_bc81_f71556f20b4a);
const GUID_ULTIMATE_PERF: GUID = GUID::from_u128(0xe9a42b02_d5df_448d_aa00_03f14749eb61);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QuickTogglesState {
    pub keep_awake: bool,
    pub power_plan: String,
    pub power_plan_label: String,
    pub recycle_bin_items: u64,
    pub recycle_bin_bytes: u64,
}

enum KeepAwakeCmd {
    Set(bool, SyncSender<bool>),
    Shutdown(SyncSender<()>),
}

struct KeepAwakeWorker {
    tx: mpsc::Sender<KeepAwakeCmd>,
    handle: Option<JoinHandle<()>>,
}

static KEEP_AWAKE_ACTIVE: AtomicBool = AtomicBool::new(false);
static KEEP_AWAKE_WORKER: OnceLock<Mutex<Option<KeepAwakeWorker>>> = OnceLock::new();

fn worker_slot() -> &'static Mutex<Option<KeepAwakeWorker>> {
    KEEP_AWAKE_WORKER.get_or_init(|| Mutex::new(None))
}

fn spawn_keep_awake_worker() -> KeepAwakeWorker {
    let (tx, rx) = mpsc::channel::<KeepAwakeCmd>();
    let handle = std::thread::Builder::new()
        .name("testbar-keep-awake".into())
        .spawn(move || {
            let mut active = false;
            while let Ok(cmd) = rx.recv() {
                match cmd {
                    KeepAwakeCmd::Set(enable, reply) => {
                        let flags = if enable {
                            ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED
                        } else {
                            ES_CONTINUOUS
                        };
                        let ok = unsafe { SetThreadExecutionState(flags) }.0 != 0;
                        if ok {
                            active = enable;
                            KEEP_AWAKE_ACTIVE.store(enable, Ordering::Release);
                        }
                        let _ = reply.send(active);
                    }
                    KeepAwakeCmd::Shutdown(reply) => {
                        if active {
                            unsafe {
                                let _ = SetThreadExecutionState(ES_CONTINUOUS);
                            }
                            KEEP_AWAKE_ACTIVE.store(false, Ordering::Release);
                        }
                        let _ = reply.send(());
                        break;
                    }
                }
            }
            if active {
                unsafe {
                    let _ = SetThreadExecutionState(ES_CONTINUOUS);
                }
                KEEP_AWAKE_ACTIVE.store(false, Ordering::Release);
            }
        })
        .ok();
    KeepAwakeWorker { tx, handle }
}

pub fn set_keep_awake(enable: bool) -> Result<QuickTogglesState, String> {
    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
    {
        let mut guard = worker_slot().lock().unwrap_or_else(|e| e.into_inner());
        let needs_spawn = match guard.as_ref() {
            None => true,
            Some(w) => w.tx.send(KeepAwakeCmd::Set(enable, reply_tx.clone())).is_err(),
        };
        if needs_spawn {
            let worker = spawn_keep_awake_worker();
            worker
                .tx
                .send(KeepAwakeCmd::Set(enable, reply_tx))
                .map_err(|e| format!("Не удалось обновить режим бодрствования: {e}"))?;
            *guard = Some(worker);
        }
    }
    let _ = reply_rx.recv_timeout(std::time::Duration::from_secs(2));
    Ok(get_quick_toggles_state())
}

pub fn shutdown() {
    let worker = {
        let mut guard = worker_slot().lock().unwrap_or_else(|e| e.into_inner());
        guard.take()
    };
    if let Some(mut w) = worker {
        let (tx, rx) = mpsc::sync_channel(1);
        if w.tx.send(KeepAwakeCmd::Shutdown(tx)).is_ok() {
            let _ = rx.recv_timeout(std::time::Duration::from_millis(500));
        }
        if let Some(h) = w.handle.take() {
            let _ = h.join();
        }
    }
    KEEP_AWAKE_ACTIVE.store(false, Ordering::Release);
}

pub fn classify_power_plan_guid(guid: &GUID) -> (&'static str, &'static str) {
    if *guid == GUID_BALANCED {
        ("balanced", "Сбалансированная")
    } else if *guid == GUID_HIGH_PERF {
        ("high_performance", "Высокая производительность")
    } else if *guid == GUID_ULTIMATE_PERF {
        ("high_performance", "Максимальная производительность")
    } else if *guid == GUID_POWER_SAVER {
        ("power_saver", "Экономия энергии")
    } else {
        ("custom", "Другая схема")
    }
}

pub fn parse_power_plan_guid(plan: &str) -> Result<GUID, String> {
    match plan.trim().to_ascii_lowercase().as_str() {
        "balanced" => Ok(GUID_BALANCED),
        "high_performance" | "performance" => Ok(GUID_HIGH_PERF),
        "power_saver" | "saver" => Ok(GUID_POWER_SAVER),
        other => Err(format!("Неизвестная схема питания: '{other}'")),
    }
}

fn query_active_power_plan() -> (&'static str, &'static str) {
    struct LocalGuidGuard(*mut GUID);
    impl Drop for LocalGuidGuard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    let _ = LocalFree(HLOCAL(self.0 as *mut _));
                }
            }
        }
    }

    unsafe {
        let mut ptr: *mut GUID = std::ptr::null_mut();
        if PowerGetActiveScheme(None, &mut ptr) != ERROR_SUCCESS || ptr.is_null() {
            return ("balanced", "Сбалансированная");
        }
        let _guard = LocalGuidGuard(ptr);
        classify_power_plan_guid(&*ptr)
    }
}

pub fn set_power_plan(plan: &str) -> Result<QuickTogglesState, String> {
    let target_guid = parse_power_plan_guid(plan)?;
    let status = unsafe { PowerSetActiveScheme(None, Some(&target_guid)) };
    if status != ERROR_SUCCESS {
        return Err(format!(
            "Не удалось переключить схему питания (код {})",
            status.0
        ));
    }
    Ok(get_quick_toggles_state())
}

fn query_recycle_bin() -> (u64, u64) {
    unsafe {
        let mut info = SHQUERYRBINFO {
            cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32,
            ..Default::default()
        };
        if SHQueryRecycleBinW(PCWSTR::null(), &mut info).is_ok() {
            (
                info.i64NumItems.max(0) as u64,
                info.i64Size.max(0) as u64,
            )
        } else {
            (0, 0)
        }
    }
}

pub fn empty_recycle_bin_and_refresh() -> Result<QuickTogglesState, String> {
    let (items, _) = query_recycle_bin();
    if items > 0 {
        unsafe {
            let flags = SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND;
            let _ = SHEmptyRecycleBinW(HWND::default(), PCWSTR::null(), flags);
        }
    }
    Ok(get_quick_toggles_state())
}

pub fn get_quick_toggles_state() -> QuickTogglesState {
    let (plan, label) = query_active_power_plan();
    let (items, bytes) = query_recycle_bin();
    QuickTogglesState {
        keep_awake: KEEP_AWAKE_ACTIVE.load(Ordering::Acquire),
        power_plan: plan.to_string(),
        power_plan_label: label.to_string(),
        recycle_bin_items: items,
        recycle_bin_bytes: bytes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn power_plan_guid_mapping_and_validation() {
        assert_eq!(
            classify_power_plan_guid(&GUID_BALANCED),
            ("balanced", "Сбалансированная")
        );
        assert_eq!(
            classify_power_plan_guid(&GUID_HIGH_PERF),
            ("high_performance", "Высокая производительность")
        );
        assert_eq!(
            classify_power_plan_guid(&GUID_POWER_SAVER),
            ("power_saver", "Экономия энергии")
        );
        assert_eq!(
            classify_power_plan_guid(&GUID_ULTIMATE_PERF),
            ("high_performance", "Максимальная производительность")
        );
        let custom = GUID::from_u128(0x11111111_2222_3333_4444_555555555555);
        assert_eq!(classify_power_plan_guid(&custom).0, "custom");

        assert_eq!(parse_power_plan_guid("balanced").unwrap(), GUID_BALANCED);
        assert_eq!(
            parse_power_plan_guid("high_performance").unwrap(),
            GUID_HIGH_PERF
        );
        assert_eq!(
            parse_power_plan_guid("power_saver").unwrap(),
            GUID_POWER_SAVER
        );
        assert!(parse_power_plan_guid("turbo").is_err());
    }

    #[test]
    fn quick_toggles_state_reads_without_panic() {
        let st = get_quick_toggles_state();
        assert!(!st.power_plan.is_empty());
        assert!(!st.power_plan_label.is_empty());
    }
}
