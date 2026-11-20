pub mod apps_indexer;
pub mod audio;
pub mod config;
pub mod media;
pub mod system_widgets;
pub mod telemetry;
pub mod win_shell;
pub mod windows_tracker;

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};

use apps_indexer::{IndexedApp, SteamGame};
use audio::AudioState;
use config::{CustomSpace, PinItem, TestBarConfig};
use media::MediaState;
use system_widgets::{ClipboardEntry, NetworkInfo, PeripheralBattery, TrayAppItem};
use telemetry::TelemetryState;
use win_shell::ShellHookEvent;
use windows_tracker::AppWindowGroup;

pub struct AppState {
    pub config: Mutex<TestBarConfig>,
    pub installed_apps: Mutex<Vec<IndexedApp>>,
    pub steam_games: Mutex<Vec<SteamGame>>,
    pub hide_native: bool,
    pub start_open: AtomicBool,
    pub flyout_open: AtomicBool,
    pub flyout_kind: Mutex<String>,
    pub last_start_toggle_ms: AtomicU64,
    pub taskbar_content_width: Mutex<i32>,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn win_hwnd(win: &WebviewWindow) -> Option<isize> {
    win.hwnd().ok().map(|h| h.0 as isize)
}

fn scale_factor(app: &AppHandle) -> f64 {
    app.get_webview_window("taskbar")
        .and_then(|w| w.scale_factor().ok())
        .unwrap_or(1.0)
}

fn position_taskbar_window(app: &AppHandle, bar_mode: &str, logical_content_w: i32) {
    let Some(tb) = app.get_webview_window("taskbar") else { return };
    let Some(hwnd) = win_hwnd(&tb) else { return };
    let sf = tb.scale_factor().unwrap_or(1.0);
    let mon = win_shell::primary_monitor_rect();

    let (w_px, h_px, x_px, y_px) = if bar_mode == "edge" {
        let h = (52.0 * sf).round() as i32;
        (mon.width, h, mon.left, mon.bottom - h)
    } else {
        // Dock mode: exact capsule width + subtle shadow margin so desktop left/right stays natively clickable
        let w = ((logical_content_w.clamp(380, 1600) + 28) as f64 * sf).round() as i32;
        let w = w.min(mon.width - 16);
        let h = (68.0 * sf).round() as i32;
        let x = mon.left + (mon.width - w) / 2;
        let y = mon.bottom - h;
        (w, h, x, y)
    };

    win_shell::set_window_bounds(hwnd, x_px, y_px, w_px, h_px, Some(true), true);
}

fn show_start_window(app: &AppHandle, show: bool) {
    let state = app.state::<AppState>();
    let Some(win) = app.get_webview_window("start") else { return };
    let Some(hwnd) = win_hwnd(&win) else { return };

    if show {
        // Close any open flyout first
        show_flyout_hidden(app);

        let sf = scale_factor(app);
        let mon = win_shell::primary_monitor_rect();
        let max_w = ((mon.width as f64 / sf) - 32.0).min(1084.0).max(860.0);
        let max_h = ((mon.height as f64 / sf) - 96.0).min(660.0).max(560.0);
        let w_px = (max_w * sf).round() as i32;
        let h_px = (max_h * sf).round() as i32;
        let bar_gap = (78.0 * sf).round() as i32;
        let x_px = mon.left + (mon.width - w_px) / 2;
        let y_px = (mon.bottom - bar_gap - h_px).max(mon.top + 16);

        win_shell::set_window_bounds(hwnd, x_px, y_px, w_px, h_px, Some(true), false);
        let _ = win.set_focus();
        state.start_open.store(true, Ordering::SeqCst);
        let _ = app.emit("start-visibility", true);
    } else {
        win_shell::set_window_bounds(hwnd, 0, 0, 1, 1, Some(false), true);
        state.start_open.store(false, Ordering::SeqCst);
        let _ = app.emit("start-visibility", false);
    }
}

fn show_flyout_hidden(app: &AppHandle) {
    let state = app.state::<AppState>();
    if let Some(win) = app.get_webview_window("flyout") {
        if let Some(hwnd) = win_hwnd(&win) {
            win_shell::set_window_bounds(hwnd, 0, 0, 1, 1, Some(false), true);
        }
    }
    state.flyout_open.store(false, Ordering::SeqCst);
    if let Ok(mut k) = state.flyout_kind.lock() {
        k.clear();
    }
    let _ = app.emit("flyout-hidden", ());
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlyoutRequest {
    pub kind: String, // "tray" | "quick" | "calendar" | "preview" | "ctx_app" | "ctx_taskbar" | "ctx_tray"
    pub anchor_x_logical: f64, // relative to taskbar window left, or screen logical X
    pub anchor_in_taskbar: bool,
    pub width: i32,
    pub height: i32,
    pub no_activate: bool,
    pub payload: serde_json::Value,
}

// ======================= TAURI IPC COMMANDS =======================

#[tauri::command]
fn get_config(state: State<AppState>) -> TestBarConfig {
    state.config.lock().unwrap().clone()
}

#[tauri::command]
fn set_bar_mode(app: AppHandle, state: State<AppState>, mode: String) -> Result<TestBarConfig, String> {
    let updated = {
        let mut cfg = state.config.lock().unwrap();
        cfg.bar_mode = if mode == "edge" { "edge".to_string() } else { "dock".to_string() };
        config::save_config(&cfg)?;
        cfg.clone()
    };
    let cw = *state.taskbar_content_width.lock().unwrap();
    position_taskbar_window(&app, &updated.bar_mode, cw);
    let _ = app.emit("config-updated", &updated);
    Ok(updated)
}

#[tauri::command]
fn set_autostart(app: AppHandle, state: State<AppState>, enable: bool) -> Result<TestBarConfig, String> {
    config::set_windows_autostart(enable)?;
    let updated = {
        let mut cfg = state.config.lock().unwrap();
        cfg.autostart = enable;
        config::save_config(&cfg)?;
        cfg.clone()
    };
    let _ = app.emit("config-updated", &updated);
    Ok(updated)
}

#[tauri::command]
fn resize_taskbar_dock(app: AppHandle, state: State<AppState>, logical_width: i32) {
    *state.taskbar_content_width.lock().unwrap() = logical_width;
    let mode = state.config.lock().unwrap().bar_mode.clone();
    position_taskbar_window(&app, &mode, logical_width);
}

#[tauri::command]
fn toggle_start(app: AppHandle, state: State<AppState>, force: Option<bool>) {
    let now = now_ms();
    let prev = state.last_start_toggle_ms.load(Ordering::SeqCst);
    if force.is_none() && now.saturating_sub(prev) < 160 {
        return;
    }
    state.last_start_toggle_ms.store(now, Ordering::SeqCst);
    let currently_open = state.start_open.load(Ordering::SeqCst);
    let next = force.unwrap_or(!currently_open);
    show_start_window(&app, next);
}

#[tauri::command]
fn open_flyout(app: AppHandle, state: State<AppState>, req: FlyoutRequest) {
    // Toggle off if clicking same flyout button again (except hover preview which updates in place)
    let cur_open = state.flyout_open.load(Ordering::SeqCst);
    let cur_kind = state.flyout_kind.lock().unwrap().clone();
    if cur_open && cur_kind == req.kind && req.kind != "preview" && !req.kind.starts_with("ctx_") {
        show_flyout_hidden(&app);
        return;
    }

    if req.kind != "preview" {
        if state.start_open.load(Ordering::SeqCst) {
            show_start_window(&app, false);
        }
    }

    let Some(fly) = app.get_webview_window("flyout") else { return };
    let Some(hwnd) = win_hwnd(&fly) else { return };
    let sf = scale_factor(&app);
    let mon = win_shell::primary_monitor_rect();

    let anchor_screen_x_px = if req.anchor_in_taskbar {
        if let Some(tb) = app.get_webview_window("taskbar") {
            let pos = tb.outer_position().map(|p| p.x).unwrap_or(mon.left);
            pos + (req.anchor_x_logical * sf).round() as i32
        } else {
            mon.left + mon.width / 2
        }
    } else {
        mon.left + (req.anchor_x_logical * sf).round() as i32
    };

    let w_px = ((req.width + 24) as f64 * sf).round() as i32;
    let h_px = ((req.height + 24) as f64 * sf).round() as i32;
    let bar_gap = (70.0 * sf).round() as i32;

    let mut x_px = anchor_screen_x_px - w_px / 2;
    x_px = x_px.clamp(mon.left + 8, mon.right - w_px - 8);
    let y_px = (mon.bottom - bar_gap - h_px).max(mon.top + 8);

    win_shell::style_overlay_window(hwnd, req.no_activate);
    win_shell::set_window_bounds(hwnd, x_px, y_px, w_px, h_px, Some(true), req.no_activate);
    if !req.no_activate {
        let _ = fly.set_focus();
    }

    state.flyout_open.store(true, Ordering::SeqCst);
    *state.flyout_kind.lock().unwrap() = req.kind.clone();
    let _ = app.emit("flyout-show", &req);
}

#[tauri::command]
fn close_flyout(app: AppHandle) {
    show_flyout_hidden(&app);
}

#[tauri::command]
fn close_preview_if_outside(app: AppHandle, state: State<AppState>) {
    if !state.flyout_open.load(Ordering::SeqCst) {
        return;
    }
    let kind = state.flyout_kind.lock().unwrap().clone();
    if kind != "preview" {
        return;
    }
    let Some(fly) = app.get_webview_window("flyout") else { return };
    let Some(hwnd_raw) = win_hwnd(&fly) else { return };
    unsafe {
        let mut pt = windows::Win32::Foundation::POINT::default();
        let mut rect = windows::Win32::Foundation::RECT::default();
        let hwnd = windows::Win32::Foundation::HWND(hwnd_raw as *mut _);
        if windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut pt).is_ok()
            && windows::Win32::UI::WindowsAndMessaging::GetWindowRect(hwnd, &mut rect).is_ok()
        {
            let inside = pt.x >= rect.left - 6
                && pt.x <= rect.right + 6
                && pt.y >= rect.top - 6
                && pt.y <= rect.bottom + 18;
            if inside {
                return;
            }
        }
    }
    show_flyout_hidden(&app);
}

#[tauri::command]
fn get_open_windows() -> Vec<AppWindowGroup> {
    windows_tracker::list_open_window_groups()
}

#[tauri::command]
fn get_window_thumbnail(hwnd: isize) -> Option<String> {
    windows_tracker::capture_window_thumbnail(hwnd, 240, 140)
}

#[tauri::command]
fn focus_window(app: AppHandle, hwnd: isize, toggle_minimize: bool) {
    show_flyout_hidden(&app);
    windows_tracker::focus_or_toggle_window(hwnd, toggle_minimize);
}

#[tauri::command]
fn close_window_hwnd(hwnd: isize) {
    windows_tracker::close_window(hwnd);
}

#[tauri::command]
fn kill_process_pid(pid: u32) -> Result<(), String> {
    windows_tracker::terminate_process_by_pid(pid)
}

#[tauri::command]
fn launch_item(app: AppHandle, target: String, as_admin: bool, close_start: bool) -> Result<(), String> {
    if close_start {
        show_start_window(&app, false);
        show_flyout_hidden(&app);
    }
    windows_tracker::launch_target(&target, as_admin)
}

#[tauri::command]
fn launch_combo(app: AppHandle, targets: Vec<String>) -> Result<(), String> {
    show_start_window(&app, false);
    for t in targets {
        if !t.trim().is_empty() {
            let _ = windows_tracker::launch_target(&t, false);
        }
    }
    Ok(())
}

#[tauri::command]
fn open_location(path: String) -> Result<(), String> {
    windows_tracker::open_file_location(&path)
}

#[tauri::command]
fn open_terminal_at(path: String) -> Result<(), String> {
    windows_tracker::open_in_terminal(&path)
}

#[tauri::command]
fn get_installed_apps(state: State<AppState>) -> Vec<IndexedApp> {
    state.installed_apps.lock().unwrap().clone()
}

#[tauri::command]
fn get_steam_games(state: State<AppState>) -> Vec<SteamGame> {
    state.steam_games.lock().unwrap().clone()
}

#[tauri::command]
fn add_pin(app: AppHandle, state: State<AppState>, space: String, mut item: PinItem) -> Result<TestBarConfig, String> {
    if item.icon_data.is_none() && (item.kind == "app" || item.kind == "file") {
        item.icon_data = windows_tracker::extract_icon_for_path(&item.target);
    }
    let updated = {
        let mut cfg = state.config.lock().unwrap();
        config::add_pin_to_space(&mut cfg, &space, item);
        config::save_config(&cfg)?;
        cfg.clone()
    };
    let _ = app.emit("config-updated", &updated);
    Ok(updated)
}

#[tauri::command]
fn remove_pin(app: AppHandle, state: State<AppState>, space: String, item_id: String) -> Result<TestBarConfig, String> {
    let updated = {
        let mut cfg = state.config.lock().unwrap();
        config::remove_pin_from_space(&mut cfg, &space, &item_id);
        config::save_config(&cfg)?;
        cfg.clone()
    };
    let _ = app.emit("config-updated", &updated);
    Ok(updated)
}

#[tauri::command]
fn reorder_pins(app: AppHandle, state: State<AppState>, space: String, from_idx: usize, to_idx: usize) -> Result<TestBarConfig, String> {
    let updated = {
        let mut cfg = state.config.lock().unwrap();
        config::reorder_pins_in_space(&mut cfg, &space, from_idx, to_idx);
        config::save_config(&cfg)?;
        cfg.clone()
    };
    let _ = app.emit("config-updated", &updated);
    Ok(updated)
}

#[tauri::command]
fn add_custom_space(app: AppHandle, state: State<AppState>, name: String) -> Result<TestBarConfig, String> {
    let clean = name.trim().to_string();
    if clean.is_empty() {
        return Err("Пустое название".to_string());
    }
    let id = format!("custom-{}", now_ms());
    let updated = {
        let mut cfg = state.config.lock().unwrap();
        cfg.custom_spaces.push(CustomSpace {
            id,
            name: clean,
            accent_hex: "#A78BFA".to_string(),
            accent_rgb: "167,139,250".to_string(),
            items: vec![],
        });
        config::save_config(&cfg)?;
        cfg.clone()
    };
    let _ = app.emit("config-updated", &updated);
    Ok(updated)
}

#[tauri::command]
fn pick_folder() -> Option<String> {
    apps_indexer::pick_folder_dialog()
}

#[tauri::command]
fn pick_file() -> Option<String> {
    apps_indexer::pick_file_dialog()
}

#[tauri::command]
fn get_audio_state() -> AudioState {
    audio::get_audio_state()
}

#[tauri::command]
fn set_master_volume(app: AppHandle, volume: u32, muted: Option<bool>) -> Result<AudioState, String> {
    audio::set_master_volume(volume, muted)?;
    let st = audio::get_audio_state();
    let _ = app.emit("audio-updated", &st);
    Ok(st)
}

#[tauri::command]
fn set_mic_muted(app: AppHandle, muted: bool) -> Result<AudioState, String> {
    audio::set_mic_muted(muted)?;
    let st = audio::get_audio_state();
    let _ = app.emit("audio-updated", &st);
    Ok(st)
}

#[tauri::command]
fn set_session_volume(app: AppHandle, pid: u32, volume: u32, muted: Option<bool>) -> Result<AudioState, String> {
    audio::set_session_volume(pid, volume, muted)?;
    let st = audio::get_audio_state();
    let _ = app.emit("audio-updated", &st);
    Ok(st)
}

#[tauri::command]
fn set_audio_device(app: AppHandle, device_id: String) -> Result<AudioState, String> {
    audio::set_default_audio_device(&device_id)?;
    let st = audio::get_audio_state();
    let _ = app.emit("audio-updated", &st);
    Ok(st)
}

#[tauri::command]
fn get_media_state() -> MediaState {
    media::get_media_state()
}

#[tauri::command]
fn media_action(app: AppHandle, action: String) -> Result<MediaState, String> {
    media::media_command(&action)?;
    std::thread::sleep(std::time::Duration::from_millis(80));
    let st = media::get_media_state();
    let _ = app.emit("media-updated", &st);
    Ok(st)
}

#[tauri::command]
fn get_telemetry() -> TelemetryState {
    telemetry::get_telemetry()
}

#[tauri::command]
fn get_clipboard_history() -> Vec<ClipboardEntry> {
    system_widgets::get_clipboard_history()
}

#[tauri::command]
fn copy_to_clipboard(text: String) -> Result<(), String> {
    system_widgets::write_clipboard_text(&text)
}

#[tauri::command]
fn toggle_clip_pin(app: AppHandle, id: String) -> Vec<ClipboardEntry> {
    let list = system_widgets::toggle_clipboard_pin(&id);
    let _ = app.emit("clipboard-updated", &list);
    list
}

#[tauri::command]
fn clear_unpinned_clips(app: AppHandle) -> Vec<ClipboardEntry> {
    let list = system_widgets::clear_unpinned_clipboard();
    let _ = app.emit("clipboard-updated", &list);
    list
}

#[tauri::command]
fn get_keyboard_layout() -> String {
    system_widgets::get_keyboard_layout_code()
}

#[tauri::command]
fn switch_keyboard_layout(app: AppHandle) -> String {
    let code = system_widgets::switch_keyboard_layout();
    let _ = app.emit("layout-updated", &code);
    code
}

#[tauri::command]
fn get_peripherals() -> Vec<PeripheralBattery> {
    system_widgets::get_peripheral_batteries()
}

#[tauri::command]
fn get_tray_apps() -> Vec<TrayAppItem> {
    system_widgets::list_tray_background_apps()
}

#[tauri::command]
fn get_network_info() -> NetworkInfo {
    system_widgets::get_network_info()
}

#[tauri::command]
fn power_action(action: String) -> Result<(), String> {
    system_widgets::system_power_action(&action)
}

#[tauri::command]
fn restore_windows_taskbar_and_exit(app: AppHandle) {
    win_shell::restore_native_taskbar();
    app.exit(0);
}

#[tauri::command]
fn get_icon_for_path(path: String) -> Option<String> {
    windows_tracker::extract_icon_for_path(&path)
}

pub fn run_watchdog_mode(parent_pid: u32) {
    win_shell::run_watchdog(parent_pid);
}

pub fn run(hide_native: bool) {
    // Panic safety net: always restore native Windows taskbar before unwinding
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        win_shell::restore_native_taskbar();
        prev_hook(info);
    }));

    if hide_native {
        win_shell::spawn_watchdog();
    }

    let initial_cfg = config::load_config();

    let app_state = AppState {
        config: Mutex::new(initial_cfg),
        installed_apps: Mutex::new(Vec::new()),
        steam_games: Mutex::new(Vec::new()),
        hide_native,
        start_open: AtomicBool::new(false),
        flyout_open: AtomicBool::new(false),
        flyout_kind: Mutex::new(String::new()),
        last_start_toggle_ms: AtomicU64::new(0),
        taskbar_content_width: Mutex::new(620),
    };

    let app = tauri::Builder::default()
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            get_config,
            set_bar_mode,
            set_autostart,
            resize_taskbar_dock,
            toggle_start,
            open_flyout,
            close_flyout,
            close_preview_if_outside,
            get_open_windows,
            get_window_thumbnail,
            focus_window,
            close_window_hwnd,
            kill_process_pid,
            launch_item,
            launch_combo,
            open_location,
            open_terminal_at,
            get_installed_apps,
            get_steam_games,
            add_pin,
            remove_pin,
            reorder_pins,
            add_custom_space,
            pick_folder,
            pick_file,
            get_audio_state,
            set_master_volume,
            set_mic_muted,
            set_session_volume,
            set_audio_device,
            get_media_state,
            media_action,
            get_telemetry,
            get_clipboard_history,
            copy_to_clipboard,
            toggle_clip_pin,
            clear_unpinned_clips,
            get_keyboard_layout,
            switch_keyboard_layout,
            get_peripherals,
            get_tray_apps,
            get_network_info,
            power_action,
            restore_windows_taskbar_and_exit,
            get_icon_for_path,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();

            // Style & position all 3 pre-warmed transparent borderless windows
            if let Some(tb) = app.get_webview_window("taskbar") {
                if let Some(hwnd) = win_hwnd(&tb) {
                    win_shell::style_overlay_window(hwnd, true);
                    let sf = tb.scale_factor().unwrap_or(1.0);
                    if hide_native {
                        win_shell::hide_native_taskbar();
                        win_shell::register_appbar(hwnd, (58.0 * sf).round() as i32);
                    }
                    let mode = app.state::<AppState>().config.lock().unwrap().bar_mode.clone();
                    position_taskbar_window(&handle, &mode, 620);
                }
            }

            if let Some(start_win) = app.get_webview_window("start") {
                if let Some(hwnd) = win_hwnd(&start_win) {
                    win_shell::style_overlay_window(hwnd, false);
                }
                let h_blur = handle.clone();
                start_win.on_window_event(move |ev| {
                    if let tauri::WindowEvent::Focused(false) = ev {
                        let st = h_blur.state::<AppState>();
                        let elapsed = now_ms().saturating_sub(st.last_start_toggle_ms.load(Ordering::SeqCst));
                        if elapsed > 220 && st.start_open.load(Ordering::SeqCst) {
                            show_start_window(&h_blur, false);
                        }
                    }
                });
            }

            if let Some(fly_win) = app.get_webview_window("flyout") {
                if let Some(hwnd) = win_hwnd(&fly_win) {
                    win_shell::style_overlay_window(hwnd, false);
                }
                let h_blur = handle.clone();
                fly_win.on_window_event(move |ev| {
                    if let tauri::WindowEvent::Focused(false) = ev {
                        let st = h_blur.state::<AppState>();
                        let kind = st.flyout_kind.lock().unwrap().clone();
                        if kind != "preview" && st.flyout_open.load(Ordering::SeqCst) {
                            show_flyout_hidden(&h_blur);
                        }
                    }
                });
            }

            // Global lone-Win key hook & Ctrl+Alt+Shift+Esc emergency restore
            let h_hook = handle.clone();
            win_shell::start_shell_hook_thread(move |ev| match ev {
                ShellHookEvent::ToggleStart => {
                    let st = h_hook.state::<AppState>();
                    let cur = st.start_open.load(Ordering::SeqCst);
                    st.last_start_toggle_ms.store(now_ms(), Ordering::SeqCst);
                    show_start_window(&h_hook, !cur);
                }
                ShellHookEvent::EmergencyRestore => {
                    win_shell::restore_native_taskbar();
                    h_hook.exit(0);
                }
            });

            // Live Clipboard History listener
            let h_clip = handle.clone();
            system_widgets::start_clipboard_listener(move |entries| {
                let _ = h_clip.emit("clipboard-updated", &entries);
            });

            // Async background app & Steam library indexer (non-blocking startup)
            let h_idx = handle.clone();
            std::thread::spawn(move || {
                let apps = apps_indexer::scan_installed_apps();
                let games = apps_indexer::scan_steam_games();
                {
                    let st = h_idx.state::<AppState>();
                    *st.installed_apps.lock().unwrap() = apps.clone();
                    *st.steam_games.lock().unwrap() = games.clone();
                }
                let _ = h_idx.emit("apps-indexed", &apps);
                let _ = h_idx.emit("games-indexed", &games);
            });

            // Fast lightweight poller for open windows, keyboard layout, and fullscreen auto-hide
            let h_fast = handle.clone();
            std::thread::spawn(move || {
                let mut was_fullscreen = false;
                let mut last_layout = String::new();
                loop {
                    let groups = windows_tracker::list_open_window_groups();
                    let _ = h_fast.emit("windows-updated", &groups);

                    let layout = system_widgets::get_keyboard_layout_code();
                    if layout != last_layout {
                        last_layout = layout.clone();
                        let _ = h_fast.emit("layout-updated", &layout);
                    }

                    let fs = win_shell::is_fullscreen_active();
                    if fs != was_fullscreen {
                        was_fullscreen = fs;
                        if let Some(tb) = h_fast.get_webview_window("taskbar") {
                            if fs {
                                let _ = tb.hide();
                                show_start_window(&h_fast, false);
                                show_flyout_hidden(&h_fast);
                            } else {
                                let _ = tb.show();
                            }
                        }
                    }

                    // Ensure native taskbar stays hidden if explorer.exe restarted
                    if hide_native && !was_fullscreen {
                        // Lightweight check every 10 ticks handled below
                    }

                    std::thread::sleep(std::time::Duration::from_millis(650));
                }
            });

            // Slower poller for audio, media, telemetry, and Bluetooth peripherals
            let h_slow = handle.clone();
            std::thread::spawn(move || {
                let mut tick = 0u64;
                loop {
                    let audio_st = audio::get_audio_state();
                    let _ = h_slow.emit("audio-updated", &audio_st);

                    let media_st = media::get_media_state();
                    let _ = h_slow.emit("media-updated", &media_st);

                    if tick % 2 == 0 {
                        let telem = telemetry::get_telemetry();
                        let _ = h_slow.emit("telemetry-updated", &telem);
                    }

                    if tick % 10 == 0 {
                        let periphs = system_widgets::get_peripheral_batteries();
                        let _ = h_slow.emit("peripherals-updated", &periphs);
                        if hide_native {
                            win_shell::hide_native_taskbar();
                        }
                    }

                    tick += 1;
                    std::thread::sleep(std::time::Duration::from_millis(1600));
                }
            });

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error building TestBar");

    app.run(move |_handle, event| {
        if let tauri::RunEvent::Exit = event {
            if hide_native {
                win_shell::restore_native_taskbar();
            }
        }
    });
}

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn real_windows_subsystems_probe_cleanly() {
        let telem = telemetry::get_telemetry();
        assert!(telem.ram_total_gb > 0.0);
        println!(
            "Telemetry OK: CPU={} ({}%), RAM={}/{} GB, GPU={} ({}C, VRAM={}/{} GB)",
            telem.cpu_name, telem.cpu_pct, telem.ram_used_gb, telem.ram_total_gb,
            telem.gpu_name, telem.gpu_temp_c, telem.vram_used_gb, telem.vram_total_gb
        );

        let aud = audio::get_audio_state();
        println!("Audio OK: vol={}%, muted={}, devices={}, sessions={}", aud.master_volume, aud.master_muted, aud.devices.len(), aud.sessions.len());

        let wins = windows_tracker::list_open_window_groups();
        println!("Windows OK: {} groups", wins.len());

        let games = apps_indexer::scan_steam_games();
        println!("Steam OK: {} games found", games.len());

        let layout = system_widgets::get_keyboard_layout_code();
        assert!(!layout.is_empty());
        println!("Layout OK: {}", layout);
    }
}
