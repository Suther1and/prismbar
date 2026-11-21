use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::macos::bridge_ffi;

pub const MAX_UNPINNED_CLIPBOARD_ENTRIES: usize = 25;
pub const MAX_CLIPBOARD_CHARS: usize = 65_536;
pub const CLIPBOARD_PREVIEW_MAX_CHARS: usize = 180;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardEntry {
    pub id: String,
    pub tag: String,
    pub text: String,
    pub color: Option<String>,
    pub pinned: bool,
    #[serde(default)]
    pub preview: String,
    #[serde(default)]
    pub line_count: usize,
    #[serde(default)]
    pub char_count: usize,
    #[serde(default)]
    pub kind: String,
}

static CLIPBOARD_HISTORY: OnceLock<Mutex<Vec<ClipboardEntry>>> = OnceLock::new();
static CLIP_SEQ: AtomicU64 = AtomicU64::new(1);
static LAST_CHANGE_COUNT: AtomicI64 = AtomicI64::new(-1);
static CLIPBOARD_PAUSED: AtomicBool = AtomicBool::new(false);

fn clipboard_store() -> &'static Mutex<Vec<ClipboardEntry>> {
    CLIPBOARD_HISTORY.get_or_init(|| Mutex::new(Vec::new()))
}

fn now_epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn next_clip_id() -> String {
    let seq = CLIP_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("clip-{}-{seq}", now_epoch_ms())
}

fn is_hex_color(t: &str) -> bool {
    if !t.starts_with('#') {
        return false;
    }
    let rest = &t[1..];
    matches!(rest.len(), 3 | 4 | 6 | 8) && rest.bytes().all(|b| b.is_ascii_hexdigit())
}

fn is_url_clip(t: &str) -> bool {
    if t.contains(char::is_whitespace) {
        return false;
    }
    let low = t.to_ascii_lowercase();
    (low.starts_with("http://") || low.starts_with("https://")) && t.len() > 8
}

fn is_path_clip(t: &str) -> bool {
    if t.contains('\n') || t.contains('\r') {
        return false;
    }
    let unquoted = t.trim_matches('"').trim_matches('\'').trim();
    if unquoted.is_empty() {
        return false;
    }
    if (unquoted.starts_with('/') && unquoted.len() > 1 && !unquoted.starts_with("//"))
        || unquoted.starts_with("~/")
    {
        return true;
    }
    if unquoted.starts_with("\\\\") && unquoted.len() > 3 {
        return true;
    }
    let bytes = unquoted.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
}

fn looks_like_code_clip(t: &str) -> bool {
    let trimmed = t.trim();
    if trimmed.is_empty() {
        return false;
    }
    if (trimmed.starts_with('{') && trimmed.ends_with('}') && trimmed.contains(':'))
        || (trimmed.starts_with('[')
            && trimmed.ends_with(']')
            && (trimmed.contains('{') || trimmed.contains('"')))
    {
        return true;
    }
    if trimmed.starts_with("#!/") || trimmed.starts_with("<?") {
        return true;
    }
    let low = trimmed.to_ascii_lowercase();
    if (low.starts_with("select ") && low.contains(" from "))
        || low.starts_with("insert into ")
        || low.starts_with("create table ")
        || (low.starts_with("update ") && low.contains(" set "))
    {
        return true;
    }
    let first_line = trimmed.lines().next().unwrap_or("").trim();
    let cli_prefixes = [
        "git ", "cargo ", "pnpm ", "npm ", "npx ", "yarn ", "docker ", "kubectl ", "curl ",
        "uv ", "python ", "pytest ", "rustc ", "brew ", "xcodebuild ", "swift ",
    ];
    if cli_prefixes.iter().any(|p| first_line.starts_with(p)) {
        return true;
    }
    let code_keywords = [
        "fn ", "pub fn ", "pub struct ", "struct ", "impl ", "enum ", "use ", "const ", "let ",
        "var ", "function ", "export ", "import ", "class ", "interface ", "type ", "def ",
        "async ", "await ", "return ", "#include ", "package ", "func ",
    ];
    if trimmed.lines().any(|line| {
        let l = line.trim_start();
        code_keywords.iter().any(|kw| l.starts_with(kw))
    }) {
        return true;
    }
    if trimmed.contains("</") && trimmed.contains('>') {
        return true;
    }
    if trimmed.contains("=>")
        || trimmed.contains("::")
        || trimmed.contains("===")
        || trimmed.contains("!==")
        || trimmed.contains("&&")
        || trimmed.contains("||")
        || (trimmed.contains('{')
            && trimmed.contains('}')
            && (trimmed.contains(';') || trimmed.contains("()")))
    {
        return true;
    }
    trimmed.lines().filter(|l| l.trim_end().ends_with(';')).count() >= 2
}

/// Classifies clipboard text into `(kind, tag, color)` matching the frontend schema.
pub fn classify_clipboard_content(text: &str) -> (String, String, Option<String>) {
    let t = text.trim();
    if is_url_clip(t) {
        return ("url".to_string(), "URL".to_string(), None);
    }
    if is_hex_color(t) {
        return ("hex".to_string(), "HEX".to_string(), Some(t.to_string()));
    }
    if is_path_clip(t) {
        return ("path".to_string(), "PATH".to_string(), None);
    }
    if looks_like_code_clip(t) {
        return ("code".to_string(), "CODE".to_string(), None);
    }
    ("text".to_string(), "TXT".to_string(), None)
}

/// Normalizes line endings and bounds clipboard text at `MAX_CLIPBOARD_CHARS`.
pub fn normalize_clipboard_text(text: &str) -> String {
    let normalized_nl = text.replace("\r\n", "\n").replace('\r', "\n");
    let trimmed = normalized_nl.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.chars().count() <= MAX_CLIPBOARD_CHARS {
        trimmed.to_string()
    } else {
        trimmed.chars().take(MAX_CLIPBOARD_CHARS).collect()
    }
}

/// Builds a concise 1–2 line preview capped at `CLIPBOARD_PREVIEW_MAX_CHARS`.
pub fn build_clipboard_preview(full_text: &str) -> String {
    let mut lines: Vec<String> = Vec::with_capacity(2);
    let mut has_more_lines = false;
    for raw_line in full_text.lines() {
        let collapsed: String = raw_line
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if collapsed.is_empty() {
            continue;
        }
        if lines.len() < 2 {
            lines.push(collapsed);
        } else {
            has_more_lines = true;
            break;
        }
    }
    let joined = lines.join("\n");
    let count = joined.chars().count();
    if count > CLIPBOARD_PREVIEW_MAX_CHARS {
        let mut out: String = joined
            .chars()
            .take(CLIPBOARD_PREVIEW_MAX_CHARS - 1)
            .collect();
        out.push('…');
        out
    } else if has_more_lines && count < CLIPBOARD_PREVIEW_MAX_CHARS {
        format!("{joined}…")
    } else {
        joined
    }
}

fn build_clipboard_entry(id: String, text: String, pinned: bool) -> ClipboardEntry {
    let (kind, tag, color) = classify_clipboard_content(&text);
    let preview = build_clipboard_preview(&text);
    let line_count = if text.is_empty() {
        0
    } else {
        text.lines().count().max(1)
    };
    let char_count = text.chars().count();
    ClipboardEntry {
        id,
        tag,
        text,
        color,
        pinned,
        preview,
        line_count,
        char_count,
        kind,
    }
}

fn trim_unpinned_overflow(list: &mut Vec<ClipboardEntry>) {
    list.sort_by_key(|c| !c.pinned);
    let mut unpinned_seen = 0usize;
    list.retain(|c| {
        if c.pinned {
            true
        } else if unpinned_seen < MAX_UNPINNED_CLIPBOARD_ENTRIES {
            unpinned_seen += 1;
            true
        } else {
            false
        }
    });
}

pub fn hydrate_pinned_clipboard_into(list: &mut Vec<ClipboardEntry>, pinned_texts: &[String]) {
    for raw in pinned_texts {
        let normalized = normalize_clipboard_text(raw);
        if normalized.is_empty() {
            continue;
        }
        if let Some(existing) = list.iter_mut().find(|c| c.text == normalized) {
            *existing = build_clipboard_entry(existing.id.clone(), normalized, true);
        } else {
            list.push(build_clipboard_entry(next_clip_id(), normalized, true));
        }
    }
    trim_unpinned_overflow(list);
}

pub fn push_clipboard_text_into(list: &mut Vec<ClipboardEntry>, text: &str) -> bool {
    let normalized = normalize_clipboard_text(text);
    if normalized.is_empty() {
        return false;
    }
    if let Some(pos) = list.iter().position(|c| c.text == normalized) {
        let old = list.remove(pos);
        list.insert(0, build_clipboard_entry(old.id, normalized, old.pinned));
    } else {
        list.insert(0, build_clipboard_entry(next_clip_id(), normalized, false));
    }
    trim_unpinned_overflow(list);
    true
}

/// Polls `NSPasteboard.changeCount` and ingests new UTF-8 text when changed and not paused.
/// Returns `Some(updated_history)` only when the in-memory clipboard history changed.
pub fn poll_clipboard_into_history() -> Option<Vec<ClipboardEntry>> {
    let count = bridge_ffi::clipboard_change_count();
    let prev = LAST_CHANGE_COUNT.swap(count, Ordering::AcqRel);
    if count == prev || CLIPBOARD_PAUSED.load(Ordering::Acquire) {
        return None;
    }
    let text = bridge_ffi::clipboard_get_text()?;
    let mut list = clipboard_store().lock().unwrap_or_else(|e| e.into_inner());
    if push_clipboard_text_into(&mut list, &text) {
        Some(list.clone())
    } else {
        None
    }
}

pub fn get_clipboard_history() -> Vec<ClipboardEntry> {
    clipboard_store()
        .lock()
        .map(|v| v.clone())
        .unwrap_or_default()
}

pub fn hydrate_pinned_clipboard(pinned_texts: &[String]) -> Vec<ClipboardEntry> {
    let mut list = clipboard_store().lock().unwrap_or_else(|e| e.into_inner());
    hydrate_pinned_clipboard_into(&mut list, pinned_texts);
    list.clone()
}

pub fn copy_to_clipboard(text: &str) -> Result<(), String> {
    bridge_ffi::clipboard_set_text(text)
}

pub fn toggle_clipboard_pin(id: &str) -> Vec<ClipboardEntry> {
    let mut list = clipboard_store().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(c) = list.iter_mut().find(|c| c.id == id) {
        c.pinned = !c.pinned;
    }
    trim_unpinned_overflow(&mut list);
    list.clone()
}

/// Alias matching `toggle_clip_pin` Tauri command naming.
pub fn toggle_clip_pin(id: &str) -> Vec<ClipboardEntry> {
    toggle_clipboard_pin(id)
}

pub fn delete_clipboard_entry(id: &str) -> Vec<ClipboardEntry> {
    let mut list = clipboard_store().lock().unwrap_or_else(|e| e.into_inner());
    list.retain(|c| c.id != id);
    list.clone()
}

pub fn set_clipboard_paused(paused: bool) -> bool {
    if !paused {
        LAST_CHANGE_COUNT.store(bridge_ffi::clipboard_change_count(), Ordering::Release);
    }
    CLIPBOARD_PAUSED.store(paused, Ordering::Release);
    paused
}

pub fn get_clipboard_paused() -> bool {
    CLIPBOARD_PAUSED.load(Ordering::Acquire)
}

pub fn clear_unpinned_clips() -> Vec<ClipboardEntry> {
    let mut list = clipboard_store().lock().unwrap_or_else(|e| e.into_inner());
    list.retain(|c| c.pinned);
    list.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_urls_paths_hex_code_and_text() {
        assert_eq!(classify_clipboard_content("https://example.com").0, "url");
        assert_eq!(classify_clipboard_content("#1E293B").0, "hex");
        assert_eq!(
            classify_clipboard_content("/Users/alex/Documents/report.pdf").0,
            "path"
        );
        assert_eq!(classify_clipboard_content("~/Downloads").0, "path");
        assert_eq!(
            classify_clipboard_content("git status && cargo test").0,
            "code"
        );
        assert_eq!(classify_clipboard_content("Привет, как дела?").0, "text");
        assert_ne!(classify_clipboard_content("// just a comment").0, "path");
    }

    #[test]
    fn preserves_all_pinned_and_caps_unpinned_at_25() {
        let mut list = Vec::new();
        hydrate_pinned_clipboard_into(
            &mut list,
            &["pinned-1".to_string(), "pinned-2".to_string()],
        );
        for i in 0..35 {
            push_clipboard_text_into(&mut list, &format!("item-{i}"));
        }
        let pinned_count = list.iter().filter(|e| e.pinned).count();
        let unpinned_count = list.iter().filter(|e| !e.pinned).count();
        assert_eq!(pinned_count, 2);
        assert_eq!(unpinned_count, 25);
        assert!(list.iter().any(|e| e.text == "pinned-1"));
        assert!(list.iter().any(|e| e.text == "item-34"));
        assert!(!list.iter().any(|e| e.text == "item-0"));
    }

    #[test]
    fn deduplicates_and_promotes_repeated_text() {
        let mut list = Vec::new();
        push_clipboard_text_into(&mut list, "alpha");
        push_clipboard_text_into(&mut list, "beta");
        push_clipboard_text_into(&mut list, "alpha");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].text, "alpha");
    }
}
