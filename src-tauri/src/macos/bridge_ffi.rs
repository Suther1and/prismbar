use base64::Engine;
use std::ffi::{c_char, c_void, CStr, CString};

/// Callback signature invoked on the dedicated `CGEventTap` thread in `bridge.m`.
pub type TbMacKeyEventCallback = extern "C" fn(i32, u16, u64, u64, *mut c_void);

/// Callback signature invoked for AppKit notifications (e.g. `NSApplicationDidResignActiveNotification`).
pub type TbMacVoidCallback = extern "C" fn(*mut c_void);

extern "C" {
    pub fn tb_mac_configure_start_window(ns_window_ptr: *mut c_void);
    pub fn tb_mac_set_blur_enabled(ns_window_ptr: *mut c_void, enabled: bool);
    pub fn tb_mac_show_and_focus_window(ns_window_ptr: *mut c_void, blur_enabled: bool);
    pub fn tb_mac_hide_window(ns_window_ptr: *mut c_void, yield_app_focus: bool);
    pub fn tb_mac_is_window_visible(ns_window_ptr: *mut c_void) -> bool;
    pub fn tb_mac_register_resign_active_observer(cb: TbMacVoidCallback, ctx: *mut c_void);
    pub fn tb_mac_position_start_window(
        ns_window_ptr: *mut c_void,
        width: f64,
        height: f64,
        position_mode: *const c_char,
    );
    pub fn tb_mac_icon_png_for_path(
        path_utf8: *const c_char,
        size: i32,
        out_len: *mut usize,
    ) -> *mut u8;
    pub fn tb_mac_thumbnail_png_for_path(
        path_utf8: *const c_char,
        size: i32,
        out_len: *mut usize,
    ) -> *mut u8;
    pub fn tb_mac_clipboard_change_count() -> i64;
    pub fn tb_mac_clipboard_get_text() -> *mut c_char;
    pub fn tb_mac_clipboard_set_text(text_utf8: *const c_char) -> bool;
    pub fn tb_mac_clipboard_set_files(paths_utf8: *const *const c_char, count: usize) -> bool;
    pub fn tb_mac_frontmost_app_pid() -> i32;
    pub fn tb_mac_activate_pid(pid: i32) -> bool;
    pub fn tb_mac_is_accessibility_trusted(prompt: bool) -> bool;
    pub fn tb_mac_open_accessibility_settings();
    pub fn tb_mac_start_command_tap(cb: TbMacKeyEventCallback, ctx: *mut c_void) -> bool;
    pub fn tb_mac_send_cmd_v();
    pub fn tb_mac_trash_item(
        path_utf8: *const c_char,
        err_buf: *mut c_char,
        err_buf_len: usize,
    ) -> bool;
    pub fn tb_mac_free(ptr: *mut c_void);
}

fn c_err_buf_to_string(buf: &[u8]) -> String {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..end]).trim().to_string()
}

fn encode_png_data_uri(bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() {
        return None;
    }
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    Some(format!("data:image/png;base64,{b64}"))
}

unsafe fn extract_png_via(
    path: &str,
    size: i32,
    ffi_fn: unsafe extern "C" fn(*const c_char, i32, *mut usize) -> *mut u8,
) -> Option<String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return None;
    }
    let c_path = CString::new(trimmed).ok()?;
    let mut out_len: usize = 0;
    let ptr = ffi_fn(c_path.as_ptr(), size.clamp(16, 512), &mut out_len);
    if ptr.is_null() {
        return None;
    }
    let uri = if out_len > 0 {
        let bytes = std::slice::from_raw_parts(ptr, out_len);
        encode_png_data_uri(bytes)
    } else {
        None
    };
    tb_mac_free(ptr as *mut c_void);
    uri
}

/// Configures the transparent dark floating `NSWindow` attributes on macOS.
pub fn configure_start_window(ns_window_ptr: *mut c_void) {
    if ns_window_ptr.is_null() {
        return;
    }
    unsafe {
        tb_mac_configure_start_window(ns_window_ptr);
    }
}

/// Enables or disables the native `NSVisualEffectView` backdrop behind the Start chassis.
pub fn set_blur_enabled(ns_window_ptr: *mut c_void, enabled: bool) {
    if ns_window_ptr.is_null() {
        return;
    }
    unsafe {
        tb_mac_set_blur_enabled(ns_window_ptr, enabled);
    }
}

/// Activates the accessory app and focuses the Start window on top of other apps.
pub fn show_and_focus_window(ns_window_ptr: *mut c_void, blur_enabled: bool) {
    if ns_window_ptr.is_null() {
        return;
    }
    unsafe {
        tb_mac_show_and_focus_window(ns_window_ptr, blur_enabled);
    }
}

/// Immediately orders out and hides the native `NSWindow` (optionally yielding app focus).
pub fn hide_window(ns_window_ptr: *mut c_void, yield_app_focus: bool) {
    if ns_window_ptr.is_null() {
        return;
    }
    unsafe {
        tb_mac_hide_window(ns_window_ptr, yield_app_focus);
    }
}

/// Returns whether the native `NSWindow` is currently ordered on screen with non-zero alpha.
pub fn is_window_visible(ns_window_ptr: *mut c_void) -> bool {
    if ns_window_ptr.is_null() {
        return false;
    }
    unsafe { tb_mac_is_window_visible(ns_window_ptr) }
}

/// Registers an observer for `NSApplicationDidResignActiveNotification`.
pub fn register_resign_active_observer(cb: TbMacVoidCallback, ctx: *mut c_void) {
    unsafe {
        tb_mac_register_resign_active_observer(cb, ctx);
    }
}

/// Positions the Start window on the screen under the mouse cursor (`"center"` or `"bottom"`).
pub fn position_start_window(
    ns_window_ptr: *mut c_void,
    width: f64,
    height: f64,
    position_mode: &str,
) {
    if ns_window_ptr.is_null() {
        return;
    }
    let mode_cstr = CString::new(position_mode.trim())
        .unwrap_or_else(|_| CString::new("center").expect("static cstr"));
    unsafe {
        tb_mac_position_start_window(ns_window_ptr, width, height, mode_cstr.as_ptr());
    }
}

/// Extracts an app/file icon via `NSWorkspace` as a `"data:image/png;base64,..."` URI.
pub fn icon_png_for_path(path: &str, size: i32) -> Option<String> {
    unsafe { extract_png_via(path, size, tb_mac_icon_png_for_path) }
}

/// Extracts a QuickLook/image thumbnail (falling back to file icon) as a PNG data URI.
pub fn thumbnail_png_for_path(path: &str, size: i32) -> Option<String> {
    unsafe { extract_png_via(path, size, tb_mac_thumbnail_png_for_path) }
}

/// Returns the monotonically increasing `NSPasteboard.generalPasteboard.changeCount`.
pub fn clipboard_change_count() -> i64 {
    unsafe { tb_mac_clipboard_change_count() }
}

/// Reads UTF-8 plain text from the macOS general pasteboard, if available.
pub fn clipboard_get_text() -> Option<String> {
    unsafe {
        let ptr = tb_mac_clipboard_get_text();
        if ptr.is_null() {
            return None;
        }
        let text = CStr::from_ptr(ptr).to_string_lossy().into_owned();
        tb_mac_free(ptr as *mut c_void);
        Some(text)
    }
}

/// Writes UTF-8 plain text to the macOS general pasteboard.
pub fn clipboard_set_text(text: &str) -> Result<(), String> {
    let sanitized = if text.contains('\0') {
        text.replace('\0', "")
    } else {
        text.to_string()
    };
    let c_text = CString::new(sanitized)
        .map_err(|e| format!("Не удалось подготовить текст для буфера обмена: {e}"))?;
    let ok = unsafe { tb_mac_clipboard_set_text(c_text.as_ptr()) };
    if ok {
        Ok(())
    } else {
        Err("Не удалось записать текст в буфер обмена macOS".to_string())
    }
}

/// Writes file URLs to the macOS general pasteboard for Finder paste operations.
pub fn clipboard_set_files(paths: &[String]) -> Result<(), String> {
    if paths.is_empty() {
        return Err("Список файлов пуст".to_string());
    }
    let mut c_paths = Vec::with_capacity(paths.len());
    for p in paths {
        let c = CString::new(p.as_str())
            .map_err(|_| format!("Путь содержит недопустимый NUL-символ: {p}"))?;
        c_paths.push(c);
    }
    let ptrs: Vec<*const c_char> = c_paths.iter().map(|c| c.as_ptr()).collect();
    let ok = unsafe { tb_mac_clipboard_set_files(ptrs.as_ptr(), ptrs.len()) };
    if ok {
        Ok(())
    } else {
        Err("Не удалось скопировать файлы в буфер обмена macOS".to_string())
    }
}

/// Returns the PID of the frontmost application (`None` if unavailable or <= 0).
pub fn frontmost_app_pid() -> Option<i32> {
    let pid = unsafe { tb_mac_frontmost_app_pid() };
    (pid > 0).then_some(pid)
}

/// Activates the application owning `pid`.
pub fn activate_pid(pid: i32) -> bool {
    if pid <= 0 {
        return false;
    }
    unsafe { tb_mac_activate_pid(pid) }
}

/// Checks macOS Accessibility permission, optionally prompting the system dialog.
pub fn is_accessibility_trusted(prompt: bool) -> bool {
    unsafe { tb_mac_is_accessibility_trusted(prompt) }
}

/// Opens System Settings -> Privacy & Security -> Accessibility.
pub fn open_accessibility_settings() {
    unsafe {
        tb_mac_open_accessibility_settings();
    }
}

/// Starts the background `CGEventTap` listener thread for lone-Command tap detection.
pub fn start_command_tap(cb: TbMacKeyEventCallback, ctx: *mut c_void) -> bool {
    unsafe { tb_mac_start_command_tap(cb, ctx) }
}

/// Synthesizes a `Cmd+V` paste keystroke via `CGEvent`.
pub fn send_cmd_v() {
    unsafe {
        tb_mac_send_cmd_v();
    }
}

/// Moves a file, folder, or `.app` bundle to Trash via `NSFileManager`.
pub fn trash_item(path: &str) -> Result<(), String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("Пустой путь для удаления в корзину".to_string());
    }
    let c_path = CString::new(trimmed)
        .map_err(|_| format!("Путь содержит недопустимый NUL-символ: {trimmed}"))?;
    let mut err_buf = [0u8; 512];
    let ok = unsafe {
        tb_mac_trash_item(
            c_path.as_ptr(),
            err_buf.as_mut_ptr() as *mut c_char,
            err_buf.len(),
        )
    };
    if ok {
        Ok(())
    } else {
        let msg = c_err_buf_to_string(&err_buf);
        if msg.is_empty() {
            Err(format!("Не удалось переместить в корзину: {trimmed}"))
        } else {
            Err(msg)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_buffer_decodes_until_nul() {
        let buf = b"Operation not permitted\0garbage";
        assert_eq!(c_err_buf_to_string(buf), "Operation not permitted");
        assert_eq!(c_err_buf_to_string(b"\0\0\0"), "");
    }

    #[test]
    fn encode_png_data_uri_skips_empty_and_prefixes_non_empty() {
        assert_eq!(encode_png_data_uri(&[]), None);
        assert_eq!(
            encode_png_data_uri(b"PNG").as_deref(),
            Some("data:image/png;base64,UE5H")
        );
    }

    #[test]
    fn empty_inputs_fail_safely_before_ffi() {
        assert!(clipboard_set_files(&[]).is_err());
        assert!(trash_item("   ").is_err());
        assert!(!activate_pid(0));
        assert!(!activate_pid(-5));
    }
}
