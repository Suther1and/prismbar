use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::{Mutex, OnceLock};

const MAX_TRASH_WALK_DEPTH: usize = 4;
const MAX_TRASH_WALK_ENTRIES: usize = 8_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QuickTogglesState {
    pub keep_awake: bool,
    pub power_plan: String,
    pub power_plan_label: String,
    pub recycle_bin_items: u64,
    pub recycle_bin_bytes: u64,
}

static CAFFEINATE_CHILD: OnceLock<Mutex<Option<Child>>> = OnceLock::new();

fn caffeinate_slot() -> &'static Mutex<Option<Child>> {
    CAFFEINATE_CHILD.get_or_init(|| Mutex::new(None))
}

fn is_keep_awake_active() -> bool {
    let mut guard = caffeinate_slot().lock().unwrap_or_else(|e| e.into_inner());
    let Some(child) = guard.as_mut() else {
        return false;
    };
    match child.try_wait() {
        Ok(None) => true,
        _ => {
            *guard = None;
            false
        }
    }
}

/// Enables or disables macOS `caffeinate -di` (keep display and system awake).
pub fn set_keep_awake(enable: bool) -> Result<QuickTogglesState, String> {
    {
        let mut guard = caffeinate_slot().lock().unwrap_or_else(|e| e.into_inner());
        if enable {
            let already_running = guard
                .as_mut()
                .and_then(|c| c.try_wait().ok())
                .is_some_and(|st| st.is_none());
            if !already_running {
                let child = Command::new("caffeinate")
                    .args(["-di", "-w", &std::process::id().to_string()])
                    .spawn()
                    .map_err(|e| format!("Не удалось запустить caffeinate: {e}"))?;
                *guard = Some(child);
            }
        } else if let Some(mut child) = guard.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
    Ok(get_quick_toggles_state())
}

/// Terminates any active `caffeinate` child process cleanly on app exit.
pub fn shutdown() {
    let child_opt = {
        let mut guard = caffeinate_slot().lock().unwrap_or_else(|e| e.into_inner());
        guard.take()
    };
    if let Some(mut child) = child_opt {
        let _ = child.kill();
        let _ = child.wait();
    }
}

/// Launches interactive macOS selection screenshot to clipboard (`screencapture -ic`).
pub fn trigger_screenshot() -> Result<(), String> {
    Command::new("screencapture")
        .arg("-ic")
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Не удалось запустить снимок экрана: {e}"))
}

fn trash_dir_path() -> Option<PathBuf> {
    dirs::home_dir()
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .map(|h| h.join(".Trash"))
}

fn is_trash_metadata_file(name: &str) -> bool {
    matches!(name, ".DS_Store" | ".localized")
}

fn accumulate_dir_bytes(
    dir: &Path,
    depth: usize,
    visited: &mut usize,
    total_bytes: &mut u64,
) {
    if depth > MAX_TRASH_WALK_DEPTH || *visited >= MAX_TRASH_WALK_ENTRIES {
        return;
    }
    let Ok(iter) = fs::read_dir(dir) else {
        return;
    };
    for entry in iter.flatten() {
        if *visited >= MAX_TRASH_WALK_ENTRIES {
            break;
        }
        *visited += 1;
        let path = entry.path();
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            accumulate_dir_bytes(&path, depth + 1, visited, total_bytes);
        } else {
            *total_bytes = total_bytes.saturating_add(meta.len());
        }
    }
}

/// Counts top-level items and recursive byte size inside `trash_dir` safely bounded.
pub fn scan_trash_dir(trash_dir: &Path) -> (u64, u64) {
    let Ok(iter) = fs::read_dir(trash_dir) else {
        return (0, 0);
    };
    let mut items = 0u64;
    let mut bytes = 0u64;
    let mut visited = 0usize;

    for entry in iter.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if is_trash_metadata_file(&name) {
            continue;
        }
        items = items.saturating_add(1);
        if visited >= MAX_TRASH_WALK_ENTRIES {
            continue;
        }
        visited += 1;
        let path = entry.path();
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            accumulate_dir_bytes(&path, 1, &mut visited, &mut bytes);
        } else {
            bytes = bytes.saturating_add(meta.len());
        }
    }
    (items, bytes)
}

fn empty_trash_direct(trash_dir: &Path) -> Result<(), String> {
    let iter = fs::read_dir(trash_dir)
        .map_err(|e| format!("Не удалось прочитать корзину: {e}"))?;
    let mut first_err: Option<String> = None;

    for entry in iter.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if is_trash_metadata_file(&name) {
            continue;
        }
        let path = entry.path();
        let res = match fs::symlink_metadata(&path) {
            Ok(m) if m.is_dir() && !m.file_type().is_symlink() => fs::remove_dir_all(&path),
            _ => fs::remove_file(&path),
        };
        if let Err(err) = res {
            if first_err.is_none() {
                first_err = Some(format!("Не удалось удалить «{name}»: {err}"));
            }
        }
    }
    if let Some(err) = first_err {
        Err(err)
    } else {
        Ok(())
    }
}

/// Empties macOS Trash via Finder AppleScript (with direct `~/.Trash` fallback) and refreshes state.
pub fn empty_recycle_bin_and_refresh() -> Result<QuickTogglesState, String> {
    let Some(trash_dir) = trash_dir_path() else {
        return Ok(get_quick_toggles_state());
    };
    let (items, _) = scan_trash_dir(&trash_dir);
    if items > 0 {
        let finder_ok = Command::new("osascript")
            .args(["-e", r#"tell application "Finder" to empty trash"#])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !finder_ok {
            empty_trash_direct(&trash_dir)?;
        }
    }
    Ok(get_quick_toggles_state())
}

/// Reads current Keep-Awake state and `~/.Trash` item count + size.
pub fn get_quick_toggles_state() -> QuickTogglesState {
    let (items, bytes) = trash_dir_path()
        .map(|p| scan_trash_dir(&p))
        .unwrap_or((0, 0));
    QuickTogglesState {
        keep_awake: is_keep_awake_active(),
        power_plan: "custom".to_string(),
        power_plan_label: String::new(),
        recycle_bin_items: items,
        recycle_bin_bytes: bytes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn scans_and_empties_temp_trash_dir() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("tb_mac_trash_{}_{}", std::process::id(), nanos));
        fs::create_dir_all(dir.join("folder")).unwrap();
        fs::write(dir.join(".DS_Store"), b"ignore").unwrap();
        fs::write(dir.join("doc.txt"), b"hello").unwrap();
        fs::write(dir.join("folder/sub.bin"), b"1234567890").unwrap();

        let (items, bytes) = scan_trash_dir(&dir);
        assert_eq!(items, 2);
        assert_eq!(bytes, 15);

        empty_trash_direct(&dir).unwrap();
        let (items_after, bytes_after) = scan_trash_dir(&dir);
        assert_eq!(items_after, 0);
        assert_eq!(bytes_after, 0);

        let _ = fs::remove_dir_all(&dir);
    }
}
