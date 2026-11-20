use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSessionManager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus,
};

use crate::windows_tracker::ComGuard;

const MEDIA_CHANNEL_CAPACITY: usize = 32;
const MEDIA_DISPATCH_TIMEOUT: Duration = Duration::from_millis(650);

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
}

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
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaCommand {
    Poll,
    PlayPause,
    Next,
    Prev,
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
        raw.rsplit(['\\', '!'])
            .next()
            .unwrap_or(raw)
            .trim_end_matches(".exe")
            .to_string()
    }
}

fn lock_unpoisoned<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

struct WorkerMsg {
    cmd: MediaCommand,
    coalesced_poll: bool,
    reply: Option<SyncSender<Result<MediaState, String>>>,
}

/// Dedicated single-thread WinRT SMTC worker (AD-5).
/// Owns an `mpsc` command channel (`Poll`, `PlayPause`, `Next`, `Prev`) and updates an
/// `Arc<Mutex<MediaState>>` snapshot so callers never invoke WinRT `.get()` or `thread::sleep`
/// on Tauri IPC threads.
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

    /// Dispatches a transport or poll command to the MTA worker with zero `thread::sleep`.
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

struct WinrtSmtcBackend {
    com_usable: bool,
    manager: Option<GlobalSystemMediaTransportControlsSessionManager>,
}

impl WinrtSmtcBackend {
    fn new(com_usable: bool) -> Self {
        Self {
            com_usable,
            manager: None,
        }
    }

    fn ensure_manager(
        &mut self,
    ) -> Result<&GlobalSystemMediaTransportControlsSessionManager, String> {
        if !self.com_usable {
            return Err("COM MTA недоступен для WinRT SMTC".to_string());
        }
        if self.manager.is_none() {
            let op = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
                .map_err(|e| format!("SMTC недоступен: {e}"))?;
            let mgr = op
                .get()
                .map_err(|e| format!("Не удалось подключиться к SMTC: {e}"))?;
            self.manager = Some(mgr);
        }
        self.manager
            .as_ref()
            .ok_or_else(|| "SMTC менеджер не инициализирован".to_string())
    }

    fn read_state(&mut self) -> MediaState {
        let Ok(mgr) = self.ensure_manager() else {
            return MediaState::default();
        };
        let Ok(session) = mgr.GetCurrentSession() else {
            return MediaState::default();
        };

        let source = session
            .SourceAppUserModelId()
            .map(|s| clean_source_name(&s.to_string()))
            .unwrap_or_default();

        let is_playing = session
            .GetPlaybackInfo()
            .and_then(|i| i.PlaybackStatus())
            .map(|st| st == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing)
            .unwrap_or(false);

        let (title, artist) = session
            .TryGetMediaPropertiesAsync()
            .ok()
            .and_then(|op| op.get().ok())
            .map(|props| {
                let t = props.Title().map(|s| s.to_string()).unwrap_or_default();
                let a = props.Artist().map(|s| s.to_string()).unwrap_or_default();
                (t, a)
            })
            .unwrap_or_default();

        let (position_sec, duration_sec) = session
            .GetTimelineProperties()
            .ok()
            .map(|tl| {
                let pos = tl
                    .Position()
                    .map(|ts| ts.Duration as f64 / 10_000_000.0)
                    .unwrap_or(0.0);
                let dur = tl
                    .EndTime()
                    .map(|ts| ts.Duration as f64 / 10_000_000.0)
                    .unwrap_or(0.0);
                (pos.max(0.0), dur.max(0.0))
            })
            .unwrap_or((0.0, 0.0));

        if title.is_empty() && artist.is_empty() {
            return MediaState::default();
        }

        MediaState {
            has_session: true,
            title,
            artist,
            source,
            is_playing,
            position_sec,
            duration_sec,
        }
    }

    fn execute(&mut self, cmd: MediaCommand, prev: &MediaState) -> Result<MediaState, String> {
        if cmd == MediaCommand::Poll {
            return Ok(self.read_state());
        }

        let session = {
            let mgr = self.ensure_manager()?;
            mgr.GetCurrentSession()
                .map_err(|_| "Нет активного медиа-плеера".to_string())?
        };

        let toggled_ok = match cmd {
            MediaCommand::PlayPause => session
                .TryTogglePlayPauseAsync()
                .and_then(|op| op.get())
                .map_err(|e| format!("Не удалось переключить воспроизведение: {e}"))?,
            MediaCommand::Next => session
                .TrySkipNextAsync()
                .and_then(|op| op.get())
                .map_err(|e| format!("Не удалось переключить трек вперёд: {e}"))?,
            MediaCommand::Prev => session
                .TrySkipPreviousAsync()
                .and_then(|op| op.get())
                .map_err(|e| format!("Не удалось переключить трек назад: {e}"))?,
            MediaCommand::Poll => true,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::Instant;

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
    }

    #[test]
    fn media_worker_dispatches_commands_and_coalesces_non_blocking_polls() {
        let track_idx = Arc::new(AtomicUsize::new(0));
        let poll_count = Arc::new(AtomicUsize::new(0));
        let worker_tid = Arc::new(Mutex::new(None::<std::thread::ThreadId>));

        let tracks = [
            ("Midnight City", "M83"),
            ("Resonance", "HOME"),
            ("Nightcall", "Kavinsky"),
        ];

        let t_idx = Arc::clone(&track_idx);
        let p_cnt = Arc::clone(&poll_count);
        let w_tid = Arc::clone(&worker_tid);

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
            }
            let idx = t_idx.load(Ordering::SeqCst) % tracks.len();
            Ok(MediaState {
                has_session: true,
                title: tracks[idx].0.to_string(),
                artist: tracks[idx].1.to_string(),
                source: "Spotify".to_string(),
                is_playing,
                position_sec: 12.0,
                duration_sec: 180.0,
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
}
