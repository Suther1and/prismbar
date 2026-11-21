use base64::Engine;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use windows::core::{Interface, HRESULT, PCWSTR, PWSTR};
use windows::Foundation::{AsyncOperationCompletedHandler, AsyncStatus, IAsyncOperation};
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession, GlobalSystemMediaTransportControlsSessionManager,
    GlobalSystemMediaTransportControlsSessionMediaProperties,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus,
};
use windows::Storage::Streams::{DataReader, IInputStream};
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER,
    KEY_READ, REG_SZ, REG_VALUE_TYPE,
};

use crate::win_shell::wide;
use crate::windows_tracker::ComGuard;

const MEDIA_CHANNEL_CAPACITY: usize = 32;
const MEDIA_DISPATCH_TIMEOUT: Duration = Duration::from_millis(650);
const WINRT_OP_TIMEOUT: Duration = Duration::from_millis(450);
const MEDIA_POLL_MIN_INTERVAL: Duration = Duration::from_millis(750);
const MAX_THUMBNAIL_BYTES: usize = 4 * 1024 * 1024;
const THUMBNAIL_DOWNSCALE_THRESHOLD_BYTES: usize = 160 * 1024;
const THUMBNAIL_MAX_DOWNSCALE_DIM: u32 = 192;
const THUMBNAIL_READ_CHUNK_BYTES: u32 = 64 * 1024;
const MAX_THUMBNAIL_CACHE_ENTRIES: usize = 24;
const MAX_THUMBNAIL_MISS_RETRIES: u8 = 6;
const THUMBNAIL_MISS_RETRY_COOLDOWN: Duration = Duration::from_secs(2);
const MAX_ENUMERATED_SESSIONS: usize = 8;

/// Number of 100-nanosecond ticks per second (`TimeSpan.Duration` / `DateTime.UniversalTime`).
const TICKS_PER_SEC: i64 = 10_000_000;
const TICKS_PER_MS: i64 = 10_000;
/// Offset between Windows epoch (1601-01-01 UTC) and Unix epoch (1970-01-01 UTC) in 100ns ticks.
pub const WINDOWS_TO_UNIX_EPOCH_TICKS: i64 = 116_444_736_000_000_000;
/// Maximum plausible elapsed extrapolation window (24h) to ignore bogus/uninitialized timestamps.
const MAX_EXTRAPOLATION_TICKS: i64 = 86_400 * TICKS_PER_SEC;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MediaSessionSummary {
    pub id: String,
    pub app_name: String,
    pub title: String,
    pub artist: String,
    pub is_playing: bool,
    pub is_current: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MediaState {
    pub has_session: bool,
    pub title: String,
    pub artist: String,
    pub source: String,
    pub is_playing: bool,
    pub position_sec: f64,
    pub duration_sec: f64,
    #[serde(default)]
    pub can_seek: bool,
    #[serde(default)]
    pub last_updated_ms: u64,
    #[serde(default)]
    pub thumbnail: Option<String>,
    #[serde(default)]
    pub sessions: Vec<MediaSessionSummary>,
    #[serde(default)]
    pub active_session_id: String,
}

/// Alias matching Wave 1B / UI naming while preserving `MediaState` compatibility.
pub type MediaInfo = MediaState;

impl Default for MediaState {
    fn default() -> Self {
        Self {
            has_session: false,
            title: "Ничего не воспроизводится".to_string(),
            artist: "Запусти музыку или видео".to_string(),
            source: String::new(),
            is_playing: false,
            position_sec: 0.0,
            duration_sec: 0.0,
            can_seek: false,
            last_updated_ms: 0,
            thumbnail: None,
            sessions: Vec::new(),
            active_session_id: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum MediaCommand {
    Poll,
    PlayPause,
    Next,
    Prev,
    Seek(f64),
    SelectSession(String),
}

pub fn parse_media_action(action: &str) -> Option<MediaCommand> {
    match action.trim().to_ascii_lowercase().as_str() {
        "toggle" | "play_pause" | "playpause" => Some(MediaCommand::PlayPause),
        "next" => Some(MediaCommand::Next),
        "prev" | "previous" => Some(MediaCommand::Prev),
        "poll" => Some(MediaCommand::Poll),
        _ => None,
    }
}

pub fn clean_source_name(raw: &str) -> String {
    let low = raw.to_ascii_lowercase();
    if low.contains("yandex") || low.contains("яндекс") {
        "Яндекс Музыка".to_string()
    } else if low.contains("spotify") {
        "Spotify".to_string()
    } else if low.contains("zen") {
        "Zen Browser".to_string()
    } else if low.contains("chrome") {
        "Chrome".to_string()
    } else if low.contains("edge") {
        "Edge".to_string()
    } else if low.contains("telegram") {
        "Telegram".to_string()
    } else {
        let tail = raw.rsplit(['\\', '!']).next().unwrap_or(raw).trim();
        let stripped = if tail.to_ascii_lowercase().ends_with(".exe") {
            &tail[..tail.len() - 4]
        } else {
            tail
        };
        stripped.to_string()
    }
}

/// Returns `true` for WinRT/COM RPC disconnect/server-death HRESULTs where the cached
/// `GlobalSystemMediaTransportControlsSessionManager` should be discarded and re-requested.
pub fn is_smtc_manager_error(hr: HRESULT) -> bool {
    let code = hr.0 as u32;
    // RPC_E_DISCONNECTED (0x80010108), RPC_E_WRONG_THREAD (0x8001010E),
    // RPC_E_SERVER_DIED (0x80010007), RPC_E_SERVER_DIED_DNE (0x80010012),
    // RPC_S_UNKNOWN_IF (0x800706B5), RPC_S_SERVER_UNAVAILABLE (0x800706BA),
    // RPC_S_CALL_FAILED (0x800706BE), RPC_S_CALL_FAILED_DNE (0x800706BF),
    // E_UNEXPECTED (0x8000FFFF), RO_E_CLOSED (0x80000013)
    matches!(
        code,
        0x8001_0108
            | 0x8001_010E
            | 0x8001_0007
            | 0x8001_0012
            | 0x8007_06B5
            | 0x8007_06BA
            | 0x8007_06BE
            | 0x8007_06BF
            | 0x8000_FFFF
            | 0x8000_0013
    )
}

/// Detects safe image MIME type from raw thumbnail magic bytes (JPEG, PNG, WebP, GIF, BMP).
pub fn detect_image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF {
        return Some("image/jpeg");
    }
    if bytes.len() >= 8 && &bytes[0..8] == b"\x89PNG\r\n\x1A\n" {
        return Some("image/png");
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    if bytes.len() >= 6 && (&bytes[0..6] == b"GIF87a" || &bytes[0..6] == b"GIF89a") {
        return Some("image/gif");
    }
    if bytes.len() >= 2 && &bytes[0..2] == b"BM" {
        return Some("image/bmp");
    }
    None
}

/// Downscales oversized PNG thumbnails (e.g. Firefox/Zen decoding YouTube WebP/JPEG into
/// 500KB-1.2MB 1280x720 PNGs in SMTC) to <= 256px max dimension so IPC & WebView2 memory stay lean.
fn downscale_large_png_if_needed(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let info = reader.info();
    let src_w = info.width;
    let src_h = info.height;
    if src_w == 0 || src_h == 0 || src_w > 4096 || src_h > 4096 {
        return None;
    }
    if src_w <= THUMBNAIL_MAX_DOWNSCALE_DIM && src_h <= THUMBNAIL_MAX_DOWNSCALE_DIM {
        return None;
    }

    let mut raw = vec![0u8; reader.output_buffer_size()];
    let frame = reader.next_frame(&mut raw).ok()?;
    let pixels = &raw[..frame.buffer_size()];

    let mut rgba = Vec::with_capacity((src_w as usize) * (src_h as usize) * 4);
    match frame.color_type {
        png::ColorType::Rgba => rgba.extend_from_slice(pixels),
        png::ColorType::Rgb => {
            for px in pixels.chunks_exact(3) {
                rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
            }
        }
        png::ColorType::GrayscaleAlpha => {
            for px in pixels.chunks_exact(2) {
                rgba.extend_from_slice(&[px[0], px[0], px[0], px[1]]);
            }
        }
        png::ColorType::Grayscale => {
            for &g in pixels {
                rgba.extend_from_slice(&[g, g, g, 255]);
            }
        }
        _ => return None,
    }
    if rgba.len() < (src_w as usize) * (src_h as usize) * 4 {
        return None;
    }

    let scale = (THUMBNAIL_MAX_DOWNSCALE_DIM as f64) / (src_w.max(src_h) as f64);
    let dst_w = ((src_w as f64 * scale).round() as u32).clamp(1, THUMBNAIL_MAX_DOWNSCALE_DIM);
    let dst_h = ((src_h as f64 * scale).round() as u32).clamp(1, THUMBNAIL_MAX_DOWNSCALE_DIM);

    let mut out_rgba = vec![0u8; (dst_w as usize) * (dst_h as usize) * 4];
    for dy in 0..dst_h {
        let sy0 = ((dy as u64) * (src_h as u64) / (dst_h as u64)) as u32;
        let sy1 = ((((dy + 1) as u64) * (src_h as u64) / (dst_h as u64)) as u32)
            .max(sy0 + 1)
            .min(src_h);
        for dx in 0..dst_w {
            let sx0 = ((dx as u64) * (src_w as u64) / (dst_w as u64)) as u32;
            let sx1 = ((((dx + 1) as u64) * (src_w as u64) / (dst_w as u64)) as u32)
                .max(sx0 + 1)
                .min(src_w);
            let mut sum = [0u64; 4];
            let mut count = 0u64;
            for sy in sy0..sy1 {
                let row_off = (sy as usize) * (src_w as usize) * 4;
                for sx in sx0..sx1 {
                    let off = row_off + (sx as usize) * 4;
                    sum[0] += u64::from(rgba[off]);
                    sum[1] += u64::from(rgba[off + 1]);
                    sum[2] += u64::from(rgba[off + 2]);
                    sum[3] += u64::from(rgba[off + 3]);
                    count += 1;
                }
            }
            let dst_off = ((dy as usize) * (dst_w as usize) + (dx as usize)) * 4;
            let denom = count.max(1);
            out_rgba[dst_off] = (sum[0] / denom) as u8;
            out_rgba[dst_off + 1] = (sum[1] / denom) as u8;
            out_rgba[dst_off + 2] = (sum[2] / denom) as u8;
            out_rgba[dst_off + 3] = (sum[3] / denom) as u8;
        }
    }

    let all_opaque = out_rgba.chunks_exact(4).all(|px| px[3] == 255);
    let mut encoded = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut encoded, dst_w, dst_h);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Default);
        if all_opaque {
            encoder.set_color(png::ColorType::Rgb);
            let mut rgb = Vec::with_capacity((dst_w as usize) * (dst_h as usize) * 3);
            for px in out_rgba.chunks_exact(4) {
                rgb.extend_from_slice(&px[..3]);
            }
            let mut writer = encoder.write_header().ok()?;
            writer.write_image_data(&rgb).ok()?;
        } else {
            encoder.set_color(png::ColorType::Rgba);
            let mut writer = encoder.write_header().ok()?;
            writer.write_image_data(&out_rgba).ok()?;
        }
    }
    (!encoded.is_empty()).then_some(encoded)
}

fn encode_thumbnail_data_uri(bytes: &[u8], fallback_content_type: &str) -> Option<String> {
    if bytes.is_empty() || bytes.len() > MAX_THUMBNAIL_BYTES {
        return None;
    }
    let mime = detect_image_mime(bytes).or_else(|| {
        let ct = fallback_content_type.trim().to_ascii_lowercase();
        let base_ct = ct.split(';').next().unwrap_or(&ct).trim();
        match base_ct {
            "image/jpeg" | "image/jpg" => Some("image/jpeg"),
            "image/png" => Some("image/png"),
            "image/webp" => Some("image/webp"),
            "image/gif" => Some("image/gif"),
            "image/bmp" => Some("image/bmp"),
            _ => None,
        }
    })?;
    if mime == "image/png" && bytes.len() > THUMBNAIL_DOWNSCALE_THRESHOLD_BYTES {
        if let Some(compact) = downscale_large_png_if_needed(bytes) {
            let b64 = base64::engine::general_purpose::STANDARD.encode(&compact);
            return Some(format!("data:image/png;base64,{b64}"));
        }
    }
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    Some(format!("data:{mime};base64,{b64}"))
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn now_windows_universal_ticks() -> i64 {
    let unix_100ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| {
            (d.as_secs() as i64)
                .saturating_mul(TICKS_PER_SEC)
                .saturating_add((d.subsec_nanos() / 100) as i64)
        })
        .unwrap_or(0);
    unix_100ns.saturating_add(WINDOWS_TO_UNIX_EPOCH_TICKS)
}

fn universal_ticks_to_unix_ms(universal_ticks: i64) -> u64 {
    if universal_ticks <= WINDOWS_TO_UNIX_EPOCH_TICKS {
        return 0;
    }
    ((universal_ticks - WINDOWS_TO_UNIX_EPOCH_TICKS) / TICKS_PER_MS).max(0) as u64
}

/// Pure, testable timeline normalizer & live extrapolator:
/// - Subtracts `start_ticks` so live/segmented streams normalize to `0..=duration_sec`.
/// - When `is_playing == true` and `last_updated_universal_ticks` is valid, extrapolates
///   `position_sec = ((pos - start) + (now - last_updated)).clamp(0.0, duration_sec)`.
/// - Returns `(position_sec, duration_sec, last_updated_ms)`.
pub fn compute_extrapolated_timeline(
    start_ticks: i64,
    end_ticks: i64,
    pos_ticks: i64,
    last_updated_universal_ticks: i64,
    now_universal_ticks: i64,
    is_playing: bool,
) -> (f64, f64, u64) {
    let duration_ticks = end_ticks.saturating_sub(start_ticks).max(0);
    let duration_sec = duration_ticks as f64 / TICKS_PER_SEC as f64;
    let base_pos_ticks = pos_ticks.saturating_sub(start_ticks).max(0);
    let base_pos_sec = base_pos_ticks as f64 / TICKS_PER_SEC as f64;

    let valid_last_updated = last_updated_universal_ticks > WINDOWS_TO_UNIX_EPOCH_TICKS;
    let (raw_pos_sec, updated_ms) = if is_playing {
        let elapsed_ticks = if valid_last_updated {
            let delta = now_universal_ticks.saturating_sub(last_updated_universal_ticks);
            if (0..=MAX_EXTRAPOLATION_TICKS).contains(&delta) {
                delta
            } else {
                0
            }
        } else {
            0
        };
        let extrapolated = base_pos_sec + (elapsed_ticks as f64 / TICKS_PER_SEC as f64);
        (extrapolated, universal_ticks_to_unix_ms(now_universal_ticks))
    } else {
        (base_pos_sec, universal_ticks_to_unix_ms(last_updated_universal_ticks))
    };

    let position_sec = if duration_sec > 0.0 {
        raw_pos_sec.clamp(0.0, duration_sec)
    } else {
        raw_pos_sec.max(0.0)
    };

    (position_sec, duration_sec, updated_ms)
}

/// Pure seek position calculator converting a relative `target_sec` into absolute WinRT
/// 100-nanosecond playback ticks (`start_ticks + clamped_offset_ticks`) and clamped seconds.
pub fn compute_seek_ticks(
    start_ticks: i64,
    end_ticks: i64,
    target_sec: f64,
) -> Result<(i64, f64), String> {
    if !target_sec.is_finite() || target_sec < 0.0 {
        return Err("Некорректная позиция перемотки".to_string());
    }
    let dur_ticks = end_ticks.saturating_sub(start_ticks).max(0);
    let raw_offset = (target_sec * TICKS_PER_SEC as f64).round();
    let offset_ticks = if raw_offset >= i64::MAX as f64 {
        i64::MAX
    } else {
        (raw_offset as i64).max(0)
    };
    let clamped_offset = if dur_ticks > 0 {
        offset_ticks.clamp(0, dur_ticks)
    } else {
        offset_ticks
    };
    let abs_ticks = start_ticks.saturating_add(clamped_offset);
    let clamped_sec = clamped_offset as f64 / TICKS_PER_SEC as f64;
    Ok((abs_ticks, clamped_sec))
}

/// Assigns deterministic unique session IDs from raw `SourceAppUserModelId` strings
/// (disambiguating multiple sessions from the same app as `id`, `id#2`, `id#3`, ...).
pub fn assign_session_ids(raw_app_ids: &[String]) -> Vec<String> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut out = Vec::with_capacity(raw_app_ids.len());
    for (idx, raw) in raw_app_ids.iter().enumerate() {
        let norm = raw.trim().to_ascii_lowercase();
        let base = if norm.is_empty() {
            format!("session-{idx}")
        } else {
            norm
        };
        let seen = counts.entry(base.clone()).or_insert(0);
        *seen += 1;
        if *seen == 1 {
            out.push(base);
        } else {
            out.push(format!("{base}#{}", *seen));
        }
    }
    out
}

/// Resolves which session index is active, honoring a persisted `selected_id` until that
/// session disappears, then falling back cleanly to OS current -> first playing -> first session.
pub fn resolve_active_session_index(
    session_ids: &[String],
    selected_id: &mut Option<String>,
    os_current_idx: Option<usize>,
    playing_flags: &[bool],
) -> Option<usize> {
    if session_ids.is_empty() {
        *selected_id = None;
        return None;
    }

    if let Some(ref wanted) = *selected_id {
        if let Some(pos) = session_ids.iter().position(|id| id == wanted) {
            return Some(pos);
        }
        // Preferred session disappeared -> clear preference so OS default takes over cleanly.
        *selected_id = None;
    }

    if let Some(idx) = os_current_idx.filter(|&i| i < session_ids.len()) {
        return Some(idx);
    }
    if let Some(idx) = playing_flags.iter().position(|&p| p) {
        return Some(idx);
    }
    Some(0)
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ThumbnailCacheKey {
    app_id: String,
    title: String,
    artist: String,
}

#[derive(Debug, Clone)]
struct ThumbnailCacheEntry {
    data_uri: Option<String>,
    miss_count: u8,
    last_miss_at: Option<Instant>,
}

#[derive(Debug, Default)]
struct BoundedThumbnailCache {
    map: HashMap<ThumbnailCacheKey, ThumbnailCacheEntry>,
    order: VecDeque<ThumbnailCacheKey>,
}

impl BoundedThumbnailCache {
    fn get_cached_at(&self, key: &ThumbnailCacheKey, now: Instant) -> Option<Option<String>> {
        let entry = self.map.get(key)?;
        if entry.data_uri.is_some() {
            return Some(entry.data_uri.clone());
        }
        if entry.miss_count >= MAX_THUMBNAIL_MISS_RETRIES {
            return Some(None);
        }
        // Throttle retries on transient misses (e.g. YouTube still downloading cover art in Zen/Chrome)
        // for 2s instead of permanently caching None after 2 quick polls.
        if entry
            .last_miss_at
            .is_some_and(|t| now.saturating_duration_since(t) < THUMBNAIL_MISS_RETRY_COOLDOWN)
        {
            return Some(None);
        }
        None
    }

    fn record_at(
        &mut self,
        key: ThumbnailCacheKey,
        data_uri: Option<String>,
        now: Instant,
        count_failure: bool,
    ) {
        if let Some(existing) = self.map.get_mut(&key) {
            if data_uri.is_some() {
                existing.data_uri = data_uri;
                existing.miss_count = 0;
                existing.last_miss_at = None;
            } else {
                if count_failure {
                    existing.miss_count = existing.miss_count.saturating_add(1);
                }
                existing.last_miss_at = Some(now);
            }
            return;
        }

        while self.map.len() >= MAX_THUMBNAIL_CACHE_ENTRIES {
            if let Some(oldest) = self.order.pop_front() {
                self.map.remove(&oldest);
            } else {
                break;
            }
        }

        let (miss_count, last_miss_at) = if data_uri.is_some() {
            (0, None)
        } else {
            (u8::from(count_failure), Some(now))
        };
        self.order.push_back(key.clone());
        self.map.insert(
            key,
            ThumbnailCacheEntry {
                data_uri,
                miss_count,
                last_miss_at,
            },
        );
    }
}

fn lock_unpoisoned<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Awaits a WinRT `IAsyncOperation<T>` with a strict bounded timeout via `SetCompleted` +
/// `mpsc::sync_channel` so a hung browser/player tab can NEVER wedge the MTA SMTC worker thread.
fn await_async_op<T>(
    op: &IAsyncOperation<T>,
    timeout: Duration,
) -> Result<T, windows::core::Error>
where
    T: windows::core::RuntimeType + 'static,
{
    if let Ok(status) = op.Status() {
        if status == AsyncStatus::Completed
            || status == AsyncStatus::Error
            || status == AsyncStatus::Canceled
        {
            return op.GetResults();
        }
    }

    let (tx, rx) = mpsc::sync_channel::<()>(1);
    let handler = AsyncOperationCompletedHandler::new(move |_async_op, _status| {
        let _ = tx.try_send(());
        Ok(())
    });
    op.SetCompleted(&handler)?;

    match rx.recv_timeout(timeout) {
        Ok(()) => op.GetResults(),
        Err(RecvTimeoutError::Timeout) => {
            let _ = op.Cancel();
            Err(windows::core::Error::new(
                HRESULT(0x8007_05B4_u32 as i32), // ERROR_TIMEOUT
                "WinRT SMTC operation timed out",
            ))
        }
        Err(RecvTimeoutError::Disconnected) => op.GetResults(),
    }
}

struct WorkerMsg {
    cmd: MediaCommand,
    coalesced_poll: bool,
    reply: Option<SyncSender<Result<MediaState, String>>>,
}

/// Dedicated single-thread WinRT SMTC worker (AD-5).
/// Owns an `mpsc` command channel (`Poll`, `PlayPause`, `Next`, `Prev`, `Seek`, `SelectSession`)
/// and updates an `Arc<Mutex<MediaState>>` snapshot so callers never invoke WinRT `.get()` or
/// `thread::sleep` on Tauri IPC threads.
pub struct MediaWorker {
    tx: SyncSender<WorkerMsg>,
    snapshot: Arc<Mutex<MediaState>>,
    poll_pending: Arc<AtomicBool>,
}

impl MediaWorker {
    pub fn spawn_with_handler<F>(mut handler: F) -> Self
    where
        F: FnMut(MediaCommand, &MediaState) -> Result<MediaState, String> + Send + 'static,
    {
        let (tx, rx) = mpsc::sync_channel::<WorkerMsg>(MEDIA_CHANNEL_CAPACITY);
        let snapshot = Arc::new(Mutex::new(MediaState::default()));
        let poll_pending = Arc::new(AtomicBool::new(false));

        let snap_worker = Arc::clone(&snapshot);
        let pending_worker = Arc::clone(&poll_pending);

        let _ = std::thread::Builder::new()
            .name("testbar-smtc-worker".to_string())
            .spawn(move || {
                while let Ok(msg) = rx.recv() {
                    if msg.coalesced_poll {
                        pending_worker.store(false, Ordering::Release);
                    }
                    let prev = lock_unpoisoned(&snap_worker).clone();
                    let result = handler(msg.cmd, &prev);
                    if let Ok(ref updated) = result {
                        *lock_unpoisoned(&snap_worker) = updated.clone();
                    }
                    if let Some(reply_tx) = msg.reply {
                        let _ = reply_tx.send(result);
                    }
                }
            });

        Self {
            tx,
            snapshot,
            poll_pending,
        }
    }

    pub fn spawn_winrt() -> Self {
        let (tx, rx) = mpsc::sync_channel::<WorkerMsg>(MEDIA_CHANNEL_CAPACITY);
        let snapshot = Arc::new(Mutex::new(MediaState::default()));
        let poll_pending = Arc::new(AtomicBool::new(false));

        let snap_worker = Arc::clone(&snapshot);
        let pending_worker = Arc::clone(&poll_pending);

        let _ = std::thread::Builder::new()
            .name("testbar-smtc-mta".to_string())
            .spawn(move || {
                // Dedicated MTA COM apartment held for the entire worker thread lifetime (AD-5)
                let com = ComGuard::init_mta();
                let mut backend = WinrtSmtcBackend::new(com.is_usable());

                while let Ok(msg) = rx.recv() {
                    if msg.coalesced_poll {
                        pending_worker.store(false, Ordering::Release);
                    }
                    let prev = lock_unpoisoned(&snap_worker).clone();
                    let result = backend.execute(msg.cmd, &prev);
                    if let Ok(ref updated) = result {
                        *lock_unpoisoned(&snap_worker) = updated.clone();
                    }
                    if let Some(reply_tx) = msg.reply {
                        let _ = reply_tx.send(result);
                    }
                }
            });

        Self {
            tx,
            snapshot,
            poll_pending,
        }
    }

    /// Reads the latest cached `MediaState` snapshot immediately without blocking on WinRT.
    pub fn snapshot(&self) -> MediaState {
        lock_unpoisoned(&self.snapshot).clone()
    }

    /// Enqueues a coalesced non-blocking `Poll` command onto the MTA worker thread.
    /// Returns `true` if a poll was scheduled or already pending.
    pub fn request_poll(&self) -> bool {
        if self
            .poll_pending
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return true;
        }
        match self.tx.try_send(WorkerMsg {
            cmd: MediaCommand::Poll,
            coalesced_poll: true,
            reply: None,
        }) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                self.poll_pending.store(false, Ordering::Release);
                false
            }
        }
    }

    /// Requests a fresh poll on the MTA worker and waits up to `timeout` for the refreshed snapshot.
    /// Safe for background threads (`h_slow`, show-time hydration); falls back to cached snapshot on timeout.
    pub fn poll_and_get(&self, timeout: Duration) -> MediaState {
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        let msg = WorkerMsg {
            cmd: MediaCommand::Poll,
            coalesced_poll: false,
            reply: Some(reply_tx),
        };
        if self.tx.try_send(msg).is_err() {
            return self.snapshot();
        }
        match reply_rx.recv_timeout(timeout) {
            Ok(Ok(state)) => state,
            Ok(Err(_)) | Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => {
                self.snapshot()
            }
        }
    }

    /// Dispatches a transport, seek, session-select, or poll command to the MTA worker with zero `thread::sleep`.
    pub fn dispatch_command(
        &self,
        cmd: MediaCommand,
        timeout: Duration,
    ) -> Result<MediaState, String> {
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        let msg = WorkerMsg {
            cmd,
            coalesced_poll: false,
            reply: Some(reply_tx),
        };
        self.tx.try_send(msg).map_err(|err| match err {
            TrySendError::Full(_) => "Очередь управления медиа переполнена".to_string(),
            TrySendError::Disconnected(_) => "Медиа-сервис недоступен".to_string(),
        })?;

        match reply_rx.recv_timeout(timeout) {
            Ok(res) => res,
            Err(RecvTimeoutError::Timeout) => Ok(self.snapshot()),
            Err(RecvTimeoutError::Disconnected) => Err("Медиа-сервис остановлен".to_string()),
        }
    }
}

struct EnumeratedSession {
    id: String,
    raw_app_id: String,
    app_name: String,
    session: GlobalSystemMediaTransportControlsSession,
    is_playing: bool,
    controls_can_seek: bool,
}

struct MediaRegKeyGuard(HKEY);

impl Drop for MediaRegKeyGuard {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = RegCloseKey(self.0);
            }
        }
    }
}

fn is_gecko_hex_aumid(raw: &str) -> bool {
    let trimmed = raw.trim();
    trimmed.len() == 16 && trimmed.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Resolves Firefox/Zen 16-hex-char SMTC AUMID hashes (e.g. `F0DC299D809B9700`) via
/// `HKCU\Software\Classes\AppUserModelId\*Toast-<HASH>` `DisplayName` (runs once per unique hash).
fn lookup_hkcu_aumid_display_name(raw_aumid: &str) -> Option<String> {
    let target = raw_aumid.trim().to_ascii_uppercase();
    if target.is_empty() {
        return None;
    }
    let suffix = format!("-{target}");
    unsafe {
        let root_w = wide(r"Software\Classes\AppUserModelId");
        let mut root_key = HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(root_w.as_ptr()),
            0,
            KEY_READ,
            &mut root_key,
        )
        .is_err()
        {
            return None;
        }
        let _root_guard = MediaRegKeyGuard(root_key);

        let val_w = wide("DisplayName");
        let mut idx = 0u32;
        let mut name_buf = [0u16; 256];
        while idx < 512 {
            let mut len = name_buf.len() as u32;
            let status = RegEnumKeyExW(
                root_key,
                idx,
                PWSTR(name_buf.as_mut_ptr()),
                &mut len,
                None,
                PWSTR::null(),
                None,
                None,
            );
            idx += 1;
            if status.is_err() {
                break;
            }
            let subkey = String::from_utf16_lossy(&name_buf[..len as usize]);
            let sub_upper = subkey.trim().to_ascii_uppercase();
            if sub_upper != target && !sub_upper.ends_with(&suffix) {
                continue;
            }
            let sub_w = wide(&subkey);
            let mut sub_hkey = HKEY::default();
            if RegOpenKeyExW(root_key, PCWSTR(sub_w.as_ptr()), 0, KEY_READ, &mut sub_hkey).is_err()
            {
                continue;
            }
            let _sub_guard = MediaRegKeyGuard(sub_hkey);
            let mut value_type = REG_VALUE_TYPE(0);
            let mut byte_len = 0u32;
            if RegQueryValueExW(
                sub_hkey,
                PCWSTR(val_w.as_ptr()),
                None,
                Some(&mut value_type),
                None,
                Some(&mut byte_len),
            )
            .is_err()
                || value_type != REG_SZ
                || byte_len < 2
                || byte_len > 1024
            {
                continue;
            }
            let mut buf = vec![0u16; (byte_len as usize).div_ceil(2)];
            let mut read_bytes = byte_len;
            if RegQueryValueExW(
                sub_hkey,
                PCWSTR(val_w.as_ptr()),
                None,
                Some(&mut value_type),
                Some(buf.as_mut_ptr() as *mut u8),
                Some(&mut read_bytes),
            )
            .is_ok()
            {
                let nul_pos = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
                let display = String::from_utf16_lossy(&buf[..nul_pos]).trim().to_string();
                if !display.is_empty() && !display.starts_with('@') {
                    return Some(display);
                }
            }
        }
    }
    None
}

struct WinrtSmtcBackend {
    com_usable: bool,
    manager: Option<GlobalSystemMediaTransportControlsSessionManager>,
    selected_session_id: Option<String>,
    thumbnails: BoundedThumbnailCache,
    source_name_cache: HashMap<String, String>,
    last_poll_at: Option<Instant>,
    last_state: Option<MediaState>,
}

impl WinrtSmtcBackend {
    fn new(com_usable: bool) -> Self {
        Self {
            com_usable,
            manager: None,
            selected_session_id: None,
            thumbnails: BoundedThumbnailCache::default(),
            source_name_cache: HashMap::new(),
            last_poll_at: None,
            last_state: None,
        }
    }

    fn resolve_source_display_name(&mut self, raw_app_id: &str) -> String {
        let key = raw_app_id.trim().to_ascii_lowercase();
        if let Some(cached) = self.source_name_cache.get(&key) {
            return cached.clone();
        }
        let mut resolved = clean_source_name(raw_app_id);
        if is_gecko_hex_aumid(&resolved) {
            if let Some(display) = lookup_hkcu_aumid_display_name(raw_app_id) {
                resolved = clean_source_name(&display);
            }
        }
        if self.source_name_cache.len() >= 32 {
            self.source_name_cache.clear();
        }
        self.source_name_cache.insert(key, resolved.clone());
        resolved
    }

    fn invalidate_manager_if_needed(&mut self, err: &windows::core::Error) {
        if is_smtc_manager_error(err.code()) {
            self.manager = None;
        }
    }

    fn ensure_manager(
        &mut self,
    ) -> Result<GlobalSystemMediaTransportControlsSessionManager, String> {
        if !self.com_usable {
            return Err("COM MTA недоступен для WinRT SMTC".to_string());
        }
        if self.manager.is_none() {
            let op = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
                .map_err(|e| format!("SMTC недоступен: {e}"))?;
            let mgr = await_async_op(&op, WINRT_OP_TIMEOUT)
                .map_err(|e| format!("Не удалось подключиться к SMTC: {e}"))?;
            self.manager = Some(mgr);
        }
        self.manager
            .clone()
            .ok_or_else(|| "SMTC менеджер не инициализирован".to_string())
    }

    fn collect_sessions(
        &mut self,
    ) -> Result<(Vec<EnumeratedSession>, Option<usize>), String> {
        let mgr = self.ensure_manager()?;
        let list = match mgr.GetSessions() {
            Ok(v) => v,
            Err(_) => {
                // Always self-heal manager if GetSessions fails (e.g. RPC disconnect / shell restart)
                self.manager = None;
                let mgr2 = self.ensure_manager()?;
                mgr2.GetSessions().map_err(|e2| {
                    self.manager = None;
                    format!("Не удалось получить список медиа-сессий: {e2}")
                })?
            }
        };

        let count = (list.Size().unwrap_or(0) as usize).min(MAX_ENUMERATED_SESSIONS);
        if count == 0 {
            return Ok((Vec::new(), None));
        }

        let current_app_id = mgr
            .GetCurrentSession()
            .ok()
            .and_then(|s| s.SourceAppUserModelId().ok())
            .map(|s| s.to_string().trim().to_ascii_lowercase());

        let mut raw_sessions = Vec::with_capacity(count);
        let mut raw_app_ids = Vec::with_capacity(count);
        for i in 0..count {
            let Ok(sess) = list.GetAt(i as u32) else {
                continue;
            };
            let raw_id = sess
                .SourceAppUserModelId()
                .map(|s| s.to_string())
                .unwrap_or_default();
            raw_app_ids.push(raw_id.clone());
            raw_sessions.push((raw_id, sess));
        }

        let ids = assign_session_ids(&raw_app_ids);
        let mut out = Vec::with_capacity(raw_sessions.len());
        let mut os_current_idx: Option<usize> = None;

        for ((raw_app_id, session), id) in raw_sessions.into_iter().zip(ids.into_iter()) {
            let playback_info = session.GetPlaybackInfo().ok();
            let is_playing = playback_info
                .as_ref()
                .and_then(|i| i.PlaybackStatus().ok())
                .map(|st| st == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing)
                .unwrap_or(false);
            let controls_can_seek = playback_info
                .as_ref()
                .and_then(|i| i.Controls().ok())
                .and_then(|c| c.IsPlaybackPositionEnabled().ok())
                .unwrap_or(false);
            let app_name = self.resolve_source_display_name(&raw_app_id);

            if os_current_idx.is_none() {
                if let Some(ref cur_id) = current_app_id {
                    if raw_app_id.trim().to_ascii_lowercase() == *cur_id {
                        os_current_idx = Some(out.len());
                    }
                }
            }

            out.push(EnumeratedSession {
                id,
                raw_app_id,
                app_name,
                session,
                is_playing,
                controls_can_seek,
            });
        }

        Ok((out, os_current_idx))
    }

    fn read_media_props(
        &mut self,
        session: &GlobalSystemMediaTransportControlsSession,
    ) -> Option<GlobalSystemMediaTransportControlsSessionMediaProperties> {
        let op = match session.TryGetMediaPropertiesAsync() {
            Ok(op) => op,
            Err(err) => {
                self.invalidate_manager_if_needed(&err);
                return None;
            }
        };
        match await_async_op(&op, WINRT_OP_TIMEOUT) {
            Ok(props) => Some(props),
            Err(err) => {
                self.invalidate_manager_if_needed(&err);
                None
            }
        }
    }

    fn read_thumbnail_cached(
        &mut self,
        app_id: &str,
        title: &str,
        artist: &str,
        props: &GlobalSystemMediaTransportControlsSessionMediaProperties,
    ) -> Option<String> {
        let key = ThumbnailCacheKey {
            app_id: app_id.trim().to_ascii_lowercase(),
            title: title.to_string(),
            artist: artist.to_string(),
        };
        let now = Instant::now();
        if let Some(cached) = self.thumbnails.get_cached_at(&key, now) {
            return cached;
        }

        let Ok(thumb_ref) = props.Thumbnail() else {
            // Browsers (Zen/Chrome on YouTube) often publish title/artist 0.5-2s BEFORE the
            // thumbnail download completes. Throttle retry by 2s without exhausting retry budget.
            self.thumbnails.record_at(key, None, now, false);
            return None;
        };

        let extracted = Self::extract_thumbnail_data_uri_from_ref(&thumb_ref);
        self.thumbnails.record_at(key, extracted.clone(), now, true);
        extracted
    }

    fn extract_thumbnail_data_uri_from_ref(
        thumb_ref: &windows::Storage::Streams::IRandomAccessStreamReference,
    ) -> Option<String> {
        let open_op = thumb_ref.OpenReadAsync().ok()?;
        let stream = await_async_op(&open_op, WINRT_OP_TIMEOUT).ok()?;
        let _ = stream.Seek(0);
        let declared_size = stream.Size().unwrap_or(0);
        if declared_size > MAX_THUMBNAIL_BYTES as u64 {
            let _ = stream.Close();
            return None;
        }
        let content_type = stream
            .ContentType()
            .ok()
            .map(|s| s.to_string())
            .unwrap_or_default();
        let Ok(input_stream) = stream.cast::<IInputStream>() else {
            let _ = stream.Close();
            return None;
        };
        let Ok(reader) = DataReader::CreateDataReader(&input_stream) else {
            let _ = stream.Close();
            return None;
        };

        let mut buf = Vec::with_capacity(
            (declared_size as usize).min(MAX_THUMBNAIL_BYTES).min(256 * 1024),
        );
        while buf.len() < MAX_THUMBNAIL_BYTES {
            let remaining = MAX_THUMBNAIL_BYTES - buf.len();
            let want = if declared_size > 0 && (buf.len() as u64) < declared_size {
                ((declared_size - buf.len() as u64) as usize)
                    .min(remaining)
                    .min(THUMBNAIL_READ_CHUNK_BYTES as usize) as u32
            } else {
                remaining.min(THUMBNAIL_READ_CHUNK_BYTES as usize) as u32
            };
            if want == 0 {
                break;
            }
            let Ok(load_raw) = reader.LoadAsync(want) else {
                break;
            };
            let Ok(load_op) = load_raw.cast::<IAsyncOperation<u32>>() else {
                break;
            };
            let Ok(loaded) = await_async_op(&load_op, WINRT_OP_TIMEOUT) else {
                break;
            };
            let readable = (loaded as usize)
                .min(reader.UnconsumedBufferLength().unwrap_or(0) as usize)
                .min(remaining);
            if readable == 0 {
                break;
            }
            let start = buf.len();
            buf.resize(start + readable, 0);
            if reader.ReadBytes(&mut buf[start..]).is_err() {
                buf.truncate(start);
                break;
            }
            if declared_size > 0 && (buf.len() as u64) >= declared_size {
                break;
            }
        }
        let _ = reader.DetachStream();
        let _ = stream.Close();
        if buf.is_empty() {
            return None;
        }
        encode_thumbnail_data_uri(&buf, &content_type)
    }

    fn read_state(&mut self) -> MediaState {
        let Ok((enumerated, os_current_idx)) = self.collect_sessions() else {
            return MediaState::default();
        };
        if enumerated.is_empty() {
            self.selected_session_id = None;
            return MediaState::default();
        }

        let session_ids: Vec<String> = enumerated.iter().map(|e| e.id.clone()).collect();
        let playing_flags: Vec<bool> = enumerated.iter().map(|e| e.is_playing).collect();
        let Some(active_idx) = resolve_active_session_index(
            &session_ids,
            &mut self.selected_session_id,
            os_current_idx,
            &playing_flags,
        ) else {
            return MediaState::default();
        };

        let mut summaries: Vec<MediaSessionSummary> = Vec::with_capacity(enumerated.len());
        let mut active_props: Option<GlobalSystemMediaTransportControlsSessionMediaProperties> =
            None;
        let mut active_title = String::new();
        let mut active_artist = String::new();

        for (idx, item) in enumerated.iter().enumerate() {
            let props_opt = self.read_media_props(&item.session);
            let (title, artist) = props_opt
                .as_ref()
                .map(|p| {
                    let t = p.Title().map(|s| s.to_string()).unwrap_or_default();
                    let a = p.Artist().map(|s| s.to_string()).unwrap_or_default();
                    (t, a)
                })
                .unwrap_or_default();

            let is_current = idx == active_idx;
            if is_current {
                active_title = title.clone();
                active_artist = artist.clone();
                active_props = props_opt;
            }

            if is_current || item.is_playing || !title.is_empty() || !artist.is_empty() {
                summaries.push(MediaSessionSummary {
                    id: item.id.clone(),
                    app_name: item.app_name.clone(),
                    title,
                    artist,
                    is_playing: item.is_playing,
                    is_current,
                });
            }
        }

        let active = &enumerated[active_idx];
        let source = active.app_name.clone();
        let is_playing = active.is_playing;

        let now_ticks = now_windows_universal_ticks();
        let (position_sec, duration_sec, last_updated_ms) = active
            .session
            .GetTimelineProperties()
            .ok()
            .map(|tl| {
                let start = tl.StartTime().map(|ts| ts.Duration).unwrap_or(0);
                let end = tl.EndTime().map(|ts| ts.Duration).unwrap_or(0);
                let pos = tl.Position().map(|ts| ts.Duration).unwrap_or(0);
                let last_upd = tl.LastUpdatedTime().map(|dt| dt.UniversalTime).unwrap_or(0);
                compute_extrapolated_timeline(start, end, pos, last_upd, now_ticks, is_playing)
            })
            .unwrap_or((0.0, 0.0, if is_playing { now_unix_ms() } else { 0 }));

        let can_seek = active.controls_can_seek && duration_sec > 0.0;

        if active_title.is_empty() && active_artist.is_empty() && !is_playing {
            return MediaState {
                sessions: summaries,
                ..MediaState::default()
            };
        }

        let display_title = if active_title.is_empty() && active_artist.is_empty() {
            if !source.is_empty() {
                source.clone()
            } else {
                "Воспроизведение".to_string()
            }
        } else {
            active_title
        };

        let thumbnail = active_props.as_ref().and_then(|props| {
            self.read_thumbnail_cached(&active.raw_app_id, &display_title, &active_artist, props)
        });

        MediaState {
            has_session: true,
            title: display_title,
            artist: active_artist,
            source,
            is_playing,
            position_sec,
            duration_sec,
            can_seek,
            last_updated_ms,
            thumbnail,
            sessions: summaries,
            active_session_id: active.id.clone(),
        }
    }

    fn resolve_target_session(
        &mut self,
    ) -> Result<GlobalSystemMediaTransportControlsSession, String> {
        let (enumerated, os_current_idx) = self.collect_sessions()?;
        let session_ids: Vec<String> = enumerated.iter().map(|e| e.id.clone()).collect();
        let playing_flags: Vec<bool> = enumerated.iter().map(|e| e.is_playing).collect();
        let active_idx = resolve_active_session_index(
            &session_ids,
            &mut self.selected_session_id,
            os_current_idx,
            &playing_flags,
        )
        .ok_or_else(|| "Нет активного медиа-плеера".to_string())?;
        Ok(enumerated[active_idx].session.clone())
    }

    fn execute(&mut self, cmd: MediaCommand, prev: &MediaState) -> Result<MediaState, String> {
        match cmd {
            MediaCommand::Poll => {
                if let (Some(ts), Some(ref cached)) = (self.last_poll_at, &self.last_state) {
                    if ts.elapsed() < MEDIA_POLL_MIN_INTERVAL {
                        return Ok(cached.clone());
                    }
                }
                let state = self.read_state();
                self.last_poll_at = Some(Instant::now());
                self.last_state = Some(state.clone());
                return Ok(state);
            }
            MediaCommand::SelectSession(session_id) => {
                let trimmed = session_id.trim().to_ascii_lowercase();
                if trimmed.is_empty() {
                    self.selected_session_id = None;
                } else {
                    self.selected_session_id = Some(trimmed);
                }
                let state = self.read_state();
                self.last_poll_at = Some(Instant::now());
                self.last_state = Some(state.clone());
                return Ok(state);
            }
            _ => {
                self.last_poll_at = None;
            }
        }

        let session = self.resolve_target_session()?;

        let mut optimistic_seek_sec: Option<f64> = None;
        let toggled_ok = match cmd {
            MediaCommand::PlayPause => {
                let op = session.TryTogglePlayPauseAsync().map_err(|e| {
                    self.invalidate_manager_if_needed(&e);
                    format!("Не удалось переключить воспроизведение: {e}")
                })?;
                await_async_op(&op, WINRT_OP_TIMEOUT).map_err(|e| {
                    self.invalidate_manager_if_needed(&e);
                    format!("Не удалось переключить воспроизведение: {e}")
                })?
            }
            MediaCommand::Next => {
                let op = session.TrySkipNextAsync().map_err(|e| {
                    self.invalidate_manager_if_needed(&e);
                    format!("Не удалось переключить трек вперёд: {e}")
                })?;
                await_async_op(&op, WINRT_OP_TIMEOUT).map_err(|e| {
                    self.invalidate_manager_if_needed(&e);
                    format!("Не удалось переключить трек вперёд: {e}")
                })?
            }
            MediaCommand::Prev => {
                let op = session.TrySkipPreviousAsync().map_err(|e| {
                    self.invalidate_manager_if_needed(&e);
                    format!("Не удалось переключить трек назад: {e}")
                })?;
                await_async_op(&op, WINRT_OP_TIMEOUT).map_err(|e| {
                    self.invalidate_manager_if_needed(&e);
                    format!("Не удалось переключить трек назад: {e}")
                })?
            }
            MediaCommand::Seek(target_sec) => {
                let (start_ticks, end_ticks) = session
                    .GetTimelineProperties()
                    .ok()
                    .map(|tl| {
                        (
                            tl.StartTime().map(|t| t.Duration).unwrap_or(0),
                            tl.EndTime().map(|t| t.Duration).unwrap_or(0),
                        )
                    })
                    .unwrap_or((0, 0));
                let (abs_ticks, clamped_sec) = compute_seek_ticks(start_ticks, end_ticks, target_sec)?;
                let op = session
                    .TryChangePlaybackPositionAsync(abs_ticks)
                    .map_err(|e| {
                        self.invalidate_manager_if_needed(&e);
                        format!("Не удалось перемотать трек: {e}")
                    })?;
                let ok = await_async_op(&op, WINRT_OP_TIMEOUT).map_err(|e| {
                    self.invalidate_manager_if_needed(&e);
                    format!("Не удалось перемотать трек: {e}")
                })?;
                if ok {
                    optimistic_seek_sec = Some(clamped_sec);
                }
                ok
            }
            MediaCommand::Poll | MediaCommand::SelectSession(_) => true,
        };

        let mut updated = self.read_state();
        if cmd == MediaCommand::PlayPause
            && toggled_ok
            && updated.has_session
            && prev.has_session
            && updated.is_playing == prev.is_playing
        {
            // Reflect toggle immediately without sleeping on the IPC thread; next poll reconciles.
            updated.is_playing = !prev.is_playing;
        }
        if let Some(seek_sec) = optimistic_seek_sec {
            if updated.has_session {
                updated.position_sec = if updated.duration_sec > 0.0 {
                    seek_sec.clamp(0.0, updated.duration_sec)
                } else {
                    seek_sec
                };
                updated.last_updated_ms = now_unix_ms();
            }
        }
        Ok(updated)
    }
}

static MEDIA_WORKER: OnceLock<MediaWorker> = OnceLock::new();

fn worker() -> &'static MediaWorker {
    MEDIA_WORKER.get_or_init(MediaWorker::spawn_winrt)
}

/// Ensures the single dedicated MTA SMTC worker thread is started.
pub fn start_media_worker() {
    let _ = worker();
}

/// Non-blocking snapshot read; schedules a coalesced background poll on the MTA worker.
pub fn get_media_state() -> MediaState {
    let w = worker();
    w.request_poll();
    w.snapshot()
}

/// Pure instant snapshot read without scheduling a poll.
pub fn get_media_snapshot() -> MediaState {
    worker().snapshot()
}

/// Background-thread poll helper that waits briefly for the MTA worker to refresh the snapshot.
pub fn poll_media_state() -> MediaState {
    worker().poll_and_get(MEDIA_DISPATCH_TIMEOUT)
}

/// Dispatches `"toggle"` / `"next"` / `"prev"` to the dedicated MTA worker with zero `thread::sleep`.
pub fn media_action(action: &str) -> Result<MediaState, String> {
    let cmd = parse_media_action(action)
        .ok_or_else(|| format!("Неизвестная медиа-команда: {action}"))?;
    worker().dispatch_command(cmd, MEDIA_DISPATCH_TIMEOUT)
}

/// Seeks the active SMTC media session to `position_sec` (normalized relative to track start).
pub fn seek_media(position_sec: f64) -> Result<MediaState, String> {
    if !position_sec.is_finite() || position_sec < 0.0 {
        return Err("Некорректная позиция перемотки".to_string());
    }
    worker().dispatch_command(MediaCommand::Seek(position_sec), MEDIA_DISPATCH_TIMEOUT)
}

/// Persists the preferred SMTC session ID inside the MTA worker (until that session disappears)
/// and immediately returns the switched `MediaState`. Pass empty string to reset to OS default.
pub fn select_media_session(session_id: String) -> Result<MediaState, String> {
    worker().dispatch_command(MediaCommand::SelectSession(session_id), MEDIA_DISPATCH_TIMEOUT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::Instant;
    use windows::Win32::Foundation::{E_UNEXPECTED, RPC_E_DISCONNECTED, S_OK};

    #[test]
    fn parse_media_action_and_source_name_normalization() {
        assert_eq!(parse_media_action("toggle"), Some(MediaCommand::PlayPause));
        assert_eq!(parse_media_action("PLAY_PAUSE"), Some(MediaCommand::PlayPause));
        assert_eq!(parse_media_action("next"), Some(MediaCommand::Next));
        assert_eq!(parse_media_action("prev"), Some(MediaCommand::Prev));
        assert_eq!(parse_media_action("previous"), Some(MediaCommand::Prev));
        assert_eq!(parse_media_action("poll"), Some(MediaCommand::Poll));
        assert_eq!(parse_media_action("bogus"), None);

        assert_eq!(clean_source_name("Yandex.Music.exe"), "Яндекс Музыка");
        assert_eq!(clean_source_name("SpotifyAB.SpotifyMusic!Spotify"), "Spotify");
        assert_eq!(clean_source_name(r"C:\Apps\Zen\zen.exe"), "Zen Browser");
        assert_eq!(clean_source_name(r"C:\Apps\VLC\vlc.exe"), "vlc");
        // Case-insensitive .EXE / .Exe stripping + UTF-8 Cyrillic safety
        assert_eq!(clean_source_name(r"C:\Apps\VLC\VLC.EXE"), "VLC");
        assert_eq!(clean_source_name(r"C:\Apps\Foo\Player.Exe"), "Player");
        assert_eq!(clean_source_name("Плеер"), "Плеер");
        assert_eq!(clean_source_name("Я"), "Я");
    }

    #[test]
    fn extrapolated_timeline_and_seek_ticks_handle_start_offset_live_advance_and_invalid_timestamps()
    {
        let start = 10 * TICKS_PER_SEC; // 10s start offset
        let end = 190 * TICKS_PER_SEC; // 180s duration
        let pos = 40 * TICKS_PER_SEC; // 30s relative position
        let base_unix_ms: i64 = 1_760_000_000_000;
        let last_upd = WINDOWS_TO_UNIX_EPOCH_TICKS + base_unix_ms * TICKS_PER_MS;
        let now_5s_later = last_upd + 5 * TICKS_PER_SEC;

        // Playing: extrapolates 30s + 5s = 35s, duration = 180s
        let (p, d, ms) =
            compute_extrapolated_timeline(start, end, pos, last_upd, now_5s_later, true);
        assert!((p - 35.0).abs() < 1e-6);
        assert!((d - 180.0).abs() < 1e-6);
        assert_eq!(ms, (base_unix_ms + 5000) as u64);

        // Playing past track end clamps cleanly to duration
        let now_500s_later = last_upd + 500 * TICKS_PER_SEC;
        let (p_clamped, _, _) =
            compute_extrapolated_timeline(start, end, pos, last_upd, now_500s_later, true);
        assert!((p_clamped - 180.0).abs() < 1e-6);

        // Paused: stays at exact 30s without extrapolating and preserves stable last_updated_ms
        let (p_paused, d_paused, ms_paused) =
            compute_extrapolated_timeline(start, end, pos, last_upd, now_5s_later, false);
        assert!((p_paused - 30.0).abs() < 1e-6);
        assert!((d_paused - 180.0).abs() < 1e-6);
        assert_eq!(ms_paused, base_unix_ms as u64);

        // Zero / pre-1970 LastUpdatedTime does not blow up position
        let (p_zero_ts, _, _) =
            compute_extrapolated_timeline(start, end, pos, 0, now_5s_later, true);
        assert!((p_zero_ts - 30.0).abs() < 1e-6);

        // Seek ticks add start_ticks and clamp within 0..=duration
        let (abs, rel) = compute_seek_ticks(start, end, 45.5).expect("valid seek");
        assert_eq!(abs, start + 455_000_000);
        assert!((rel - 45.5).abs() < 1e-6);

        let (abs_max, rel_max) = compute_seek_ticks(start, end, 999.0).expect("clamped seek");
        assert_eq!(abs_max, end);
        assert!((rel_max - 180.0).abs() < 1e-6);

        assert!(compute_seek_ticks(start, end, -1.0).is_err());
        assert!(compute_seek_ticks(start, end, f64::NAN).is_err());
    }

    #[test]
    fn thumbnail_mime_detection_and_bounded_cache_work_cleanly() {
        assert_eq!(detect_image_mime(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
        assert_eq!(
            detect_image_mime(b"\x89PNG\r\n\x1A\n\x00\x00"),
            Some("image/png")
        );
        assert_eq!(
            detect_image_mime(b"RIFF\x10\x00\x00\x00WEBP"),
            Some("image/webp")
        );
        assert_eq!(detect_image_mime(b"GIF89a"), Some("image/gif"));
        assert_eq!(detect_image_mime(b"not-an-image"), None);

        let uri = encode_thumbnail_data_uri(&[0xFF, 0xD8, 0xFF, 0xDB], "").unwrap();
        assert!(uri.starts_with("data:image/jpeg;base64,"));
        // WebP (YouTube in Chrome/Zen) with empty or non-standard content type detected cleanly by magic bytes
        let mut webp = b"RIFF\x24\x00\x00\x00WEBPVP8 ".to_vec();
        webp.resize(300 * 1024, 0xAB); // 300KB (> old 256KB cap!) must now succeed!
        let webp_uri = encode_thumbnail_data_uri(&webp, "application/octet-stream").unwrap();
        assert!(webp_uri.starts_with("data:image/webp;base64,"));

        // Oversized (> 2MB) rejected safely
        let oversized = vec![0xFFu8; MAX_THUMBNAIL_BYTES + 1];
        assert!(encode_thumbnail_data_uri(&oversized, "image/jpeg").is_none());

        // Oversized raw PNG (e.g. Firefox/Zen decoding YouTube thumbnail to large 512x512 PNG)
        // is automatically downscaled to <= 256px compact PNG.
        let mut big_png = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut big_png, 512, 512);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.set_compression(png::Compression::Fast);
            let mut w = enc.write_header().unwrap();
            let mut pixels = vec![0u8; 512 * 512 * 4];
            for (i, b) in pixels.iter_mut().enumerate() {
                *b = ((i.wrapping_mul(31) ^ (i >> 3)) & 0xFF) as u8;
            }
            w.write_image_data(&pixels).unwrap();
        }
        assert!(big_png.len() > THUMBNAIL_DOWNSCALE_THRESHOLD_BYTES);
        let compact_uri = encode_thumbnail_data_uri(&big_png, "").unwrap();
        assert!(compact_uri.starts_with("data:image/png;base64,"));
        let decoded_compact = base64::engine::general_purpose::STANDARD
            .decode(compact_uri.strip_prefix("data:image/png;base64,").unwrap())
            .unwrap();
        assert!(
            decoded_compact.len() < big_png.len(),
            "downscaled PNG ({}) must be smaller than raw 512x512 PNG ({})",
            decoded_compact.len(),
            big_png.len()
        );

        // Cache throttles transient misses by 2s cooldown instead of permanently locking out
        // late-loading YouTube thumbnails, caches Some immediately, and enforces capacity bound.
        let mut cache = BoundedThumbnailCache::default();
        let k1 = ThumbnailCacheKey {
            app_id: "zen.exe".into(),
            title: "YouTube Video".into(),
            artist: "Channel".into(),
        };
        let t0 = Instant::now();
        assert_eq!(cache.get_cached_at(&k1, t0), None);
        // Browser hasn't downloaded thumbnail yet (count_failure = false): throttled for 2s, then retries!
        cache.record_at(k1.clone(), None, t0, false);
        assert_eq!(
            cache.get_cached_at(&k1, t0 + Duration::from_millis(500)),
            Some(None),
            "within 2s cooldown returns throttled None"
        );
        assert_eq!(
            cache.get_cached_at(&k1, t0 + THUMBNAIL_MISS_RETRY_COOLDOWN),
            None,
            "after 2s cooldown allows retry to pick up late-arriving YouTube cover art"
        );
        // Once thumbnail arrives, caches Some immediately
        cache.record_at(k1.clone(), Some("data:image/webp;base64,YT".into()), t0 + Duration::from_secs(3), true);
        assert_eq!(
            cache.get_cached_at(&k1, t0 + Duration::from_secs(4)),
            Some(Some("data:image/webp;base64,YT".into()))
        );

        // Hard extraction failures cap cleanly at MAX_THUMBNAIL_MISS_RETRIES
        let k_fail = ThumbnailCacheKey {
            app_id: "local.exe".into(),
            title: "NoArt".into(),
            artist: "Artist".into(),
        };
        for i in 0..MAX_THUMBNAIL_MISS_RETRIES {
            let ti = t0 + Duration::from_secs((i as u64) * 5);
            cache.record_at(k_fail.clone(), None, ti, true);
        }
        assert_eq!(
            cache.get_cached_at(&k_fail, t0 + Duration::from_secs(120)),
            Some(None),
            "hard misses capped at MAX_THUMBNAIL_MISS_RETRIES"
        );

        let k2 = ThumbnailCacheKey {
            app_id: "chrome.exe".into(),
            title: "Video".into(),
            artist: "Channel".into(),
        };
        cache.record_at(k2.clone(), Some("data:image/png;base64,AAA".into()), t0, true);
        assert_eq!(
            cache.get_cached_at(&k2, t0),
            Some(Some("data:image/png;base64,AAA".into()))
        );

        for i in 0..40 {
            cache.record_at(
                ThumbnailCacheKey {
                    app_id: "app".into(),
                    title: format!("t{i}"),
                    artist: "a".into(),
                },
                Some(format!("data:image/png;base64,{i}")),
                t0,
                true,
            );
        }
        assert!(cache.map.len() <= MAX_THUMBNAIL_CACHE_ENTRIES);
    }

    #[test]
    fn multi_session_id_assignment_persistence_and_fallback_on_disappear() {
        let ids = assign_session_ids(&[
            "Spotify.exe".into(),
            "chrome.exe".into(),
            "CHROME.EXE".into(),
        ]);
        assert_eq!(ids, vec!["spotify.exe", "chrome.exe", "chrome.exe#2"]);

        let mut selected = Some("chrome.exe#2".to_string());
        // Preferred session persists even when OS current is Spotify (index 0)
        assert_eq!(
            resolve_active_session_index(&ids, &mut selected, Some(0), &[true, false, false]),
            Some(2)
        );
        assert_eq!(selected.as_deref(), Some("chrome.exe#2"));

        // When preferred session disappears, selected resets and falls back to OS current (index 0)
        let shorter = vec!["spotify.exe".to_string(), "chrome.exe".to_string()];
        assert_eq!(
            resolve_active_session_index(&shorter, &mut selected, Some(0), &[false, true]),
            Some(0)
        );
        assert_eq!(selected, None);

        // When OS current is absent, falls back to first playing session
        assert_eq!(
            resolve_active_session_index(&shorter, &mut selected, None, &[false, true]),
            Some(1)
        );

        // RPC error classification (timeout / S_OK must NOT trigger manager invalidation)
        assert!(is_smtc_manager_error(RPC_E_DISCONNECTED));
        assert!(is_smtc_manager_error(E_UNEXPECTED));
        assert!(!is_smtc_manager_error(HRESULT(0x8007_05B4_u32 as i32)));
        assert!(!is_smtc_manager_error(S_OK));
    }

    #[test]
    fn media_worker_dispatches_commands_and_coalesces_non_blocking_polls() {
        let track_idx = Arc::new(AtomicUsize::new(0));
        let poll_count = Arc::new(AtomicUsize::new(0));
        let worker_tid = Arc::new(Mutex::new(None::<std::thread::ThreadId>));
        let selected = Arc::new(Mutex::new("spotify.exe".to_string()));
        let position = Arc::new(Mutex::new(12.0f64));

        let tracks = [
            ("Midnight City", "M83"),
            ("Resonance", "HOME"),
            ("Nightcall", "Kavinsky"),
        ];

        let t_idx = Arc::clone(&track_idx);
        let p_cnt = Arc::clone(&poll_count);
        let w_tid = Arc::clone(&worker_tid);
        let sel = Arc::clone(&selected);
        let pos_arc = Arc::clone(&position);

        let worker = MediaWorker::spawn_with_handler(move |cmd, prev| {
            *w_tid.lock().unwrap() = Some(std::thread::current().id());
            let mut is_playing = prev.is_playing;
            match cmd {
                MediaCommand::Poll => {
                    p_cnt.fetch_add(1, Ordering::SeqCst);
                }
                MediaCommand::PlayPause => {
                    is_playing = !prev.is_playing;
                }
                MediaCommand::Next => {
                    t_idx.fetch_add(1, Ordering::SeqCst);
                    is_playing = true;
                }
                MediaCommand::Prev => {
                    let cur = t_idx.load(Ordering::SeqCst);
                    t_idx.store(cur.saturating_sub(1), Ordering::SeqCst);
                    is_playing = true;
                }
                MediaCommand::Seek(sec) => {
                    *pos_arc.lock().unwrap() = sec.clamp(0.0, 180.0);
                }
                MediaCommand::SelectSession(id) => {
                    *sel.lock().unwrap() = id;
                }
            }
            let idx = t_idx.load(Ordering::SeqCst) % tracks.len();
            let active_id = sel.lock().unwrap().clone();
            let pos = *pos_arc.lock().unwrap();
            Ok(MediaState {
                has_session: true,
                title: tracks[idx].0.to_string(),
                artist: tracks[idx].1.to_string(),
                source: "Spotify".to_string(),
                is_playing,
                position_sec: pos,
                duration_sec: 180.0,
                can_seek: true,
                last_updated_ms: 1_760_000_000_000,
                thumbnail: None,
                sessions: vec![MediaSessionSummary {
                    id: active_id.clone(),
                    app_name: "Spotify".to_string(),
                    title: tracks[idx].0.to_string(),
                    artist: tracks[idx].1.to_string(),
                    is_playing,
                    is_current: true,
                }],
                active_session_id: active_id,
            })
        });

        // Initial snapshot is default and reads instantaneously without blocking
        let t0 = Instant::now();
        let initial = worker.snapshot();
        assert!(!initial.has_session);
        assert!(t0.elapsed() < Duration::from_millis(50));

        // Poll refreshes snapshot on a separate worker thread
        let polled = worker.poll_and_get(Duration::from_secs(1));
        assert!(polled.has_session);
        assert_eq!(polled.title, "Midnight City");
        assert!(!polled.is_playing);
        assert!(polled.can_seek);
        assert_ne!(
            *worker_tid.lock().unwrap(),
            Some(std::thread::current().id()),
            "SMTC work must execute on dedicated worker thread, never caller thread"
        );

        // PlayPause toggles cleanly
        let playing = worker
            .dispatch_command(MediaCommand::PlayPause, Duration::from_secs(1))
            .expect("play/pause must succeed");
        assert!(playing.is_playing);
        assert_eq!(worker.snapshot(), playing);

        // Seek updates position cleanly
        let seeked = worker
            .dispatch_command(MediaCommand::Seek(95.5), Duration::from_secs(1))
            .expect("seek must succeed");
        assert!((seeked.position_sec - 95.5).abs() < 1e-6);

        // SelectSession updates active_session_id cleanly
        let switched = worker
            .dispatch_command(
                MediaCommand::SelectSession("chrome.exe".to_string()),
                Duration::from_secs(1),
            )
            .expect("select session must succeed");
        assert_eq!(switched.active_session_id, "chrome.exe");

        // Next & Prev advance/rewind track cleanly
        let next = worker
            .dispatch_command(MediaCommand::Next, Duration::from_secs(1))
            .expect("next must succeed");
        assert_eq!(next.title, "Resonance");

        let prev = worker
            .dispatch_command(MediaCommand::Prev, Duration::from_secs(1))
            .expect("prev must succeed");
        assert_eq!(prev.title, "Midnight City");

        // Non-blocking request_poll returns immediately and updates snapshot
        assert!(worker.request_poll());
        let after = worker.poll_and_get(Duration::from_secs(1));
        assert_eq!(after.title, "Midnight City");
        assert!(poll_count.load(Ordering::SeqCst) >= 2);
    }

    #[test]
    fn media_worker_snapshot_never_blocks_when_worker_is_busy() {
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let started = Arc::new(AtomicBool::new(false));
        let started_clone = Arc::clone(&started);

        let worker = MediaWorker::spawn_with_handler(move |cmd, _prev| {
            if cmd == MediaCommand::Poll {
                started_clone.store(true, Ordering::Release);
                let _ = release_rx.recv_timeout(Duration::from_secs(2));
            }
            Ok(MediaState {
                has_session: true,
                title: "Loaded".to_string(),
                artist: "Artist".to_string(),
                source: "Spotify".to_string(),
                is_playing: true,
                position_sec: 1.0,
                duration_sec: 100.0,
                ..MediaState::default()
            })
        });

        // Kick off slow poll, then verify snapshot() returns immediately (< 50ms)
        assert!(worker.request_poll());
        let t0 = Instant::now();
        let snap = worker.snapshot();
        assert!(t0.elapsed() < Duration::from_millis(50));
        assert!(!snap.has_session);

        // Coalesced second request_poll does not block or duplicate
        assert!(worker.request_poll());

        let _ = release_tx.send(());
        let refreshed = worker.poll_and_get(Duration::from_secs(1));
        assert!(started.load(Ordering::Acquire));
        assert_eq!(refreshed.title, "Loaded");
    }

    #[test]
    fn live_smtc_sessions_probe_cleanly() {
        let com = ComGuard::init_mta();
        assert!(com.is_usable());
        let mut backend = WinrtSmtcBackend::new(true);
        let Ok((sessions, _)) = backend.collect_sessions() else {
            return;
        };
        println!("LIVE SMTC sessions: {}", sessions.len());
        for s in &sessions {
            let props = backend.read_media_props(&s.session);
            let title = props
                .as_ref()
                .and_then(|p| p.Title().ok())
                .map(|v| v.to_string())
                .unwrap_or_default();
            let artist = props
                .as_ref()
                .and_then(|p| p.Artist().ok())
                .map(|v| v.to_string())
                .unwrap_or_default();
            let thumb_diag = if let Some(ref p) = props {
                match p.Thumbnail() {
                    Ok(tref) => {
                        let open = tref.OpenReadAsync().ok().and_then(|op| await_async_op(&op, WINRT_OP_TIMEOUT).ok());
                        let raw_info = open.map(|st| {
                            let sz = st.Size().ok();
                            let pos = st.Position().ok();
                            let ct = st.ContentType().ok().map(|v| v.to_string());
                            let _ = st.Close();
                            format!("size={sz:?} pos={pos:?} ct={ct:?}")
                        });
                        let extracted = WinrtSmtcBackend::extract_thumbnail_data_uri_from_ref(&tref);
                        format!(
                            "raw=({raw_info:?}) extracted={}",
                            extracted.as_ref().map(|u| format!("ok(len={}, prefix={})", u.len(), &u[..u.len().min(32)])).unwrap_or_else(|| "NONE".into())
                        )
                    }
                    Err(e) => format!("props.Thumbnail() Err({e})"),
                }
            } else {
                "no props".into()
            };
            println!(
                "  app={} clean={} playing={} title=[{}] artist=[{}] thumb={}",
                s.raw_app_id, s.app_name, s.is_playing, title, artist, thumb_diag
            );
        }
    }
}
