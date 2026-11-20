use serde::{Deserialize, Serialize};
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSessionManager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
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

fn clean_source_name(raw: &str) -> String {
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

pub fn get_media_state() -> MediaState {
    let Ok(op) = GlobalSystemMediaTransportControlsSessionManager::RequestAsync() else {
        return MediaState::default();
    };
    let Ok(mgr) = op.get() else {
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
            let pos = tl.Position().map(|ts| ts.Duration as f64 / 10_000_000.0).unwrap_or(0.0);
            let dur = tl.EndTime().map(|ts| ts.Duration as f64 / 10_000_000.0).unwrap_or(0.0);
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

pub fn media_command(action: &str) -> Result<(), String> {
    let mgr = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
        .map_err(|e| e.to_string())?
        .get()
        .map_err(|e| e.to_string())?;
    let session = mgr.GetCurrentSession().map_err(|e| e.to_string())?;
    match action {
        "toggle" => {
            let _ = session.TryTogglePlayPauseAsync().and_then(|op| op.get());
        }
        "next" => {
            let _ = session.TrySkipNextAsync().and_then(|op| op.get());
        }
        "prev" => {
            let _ = session.TrySkipPreviousAsync().and_then(|op| op.get());
        }
        _ => {}
    }
    Ok(())
}
