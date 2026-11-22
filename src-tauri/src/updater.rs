use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::UpdaterExt;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheckResult {
    pub available: bool,
    pub current_version: String,
    pub version: Option<String>,
    pub date: Option<String>,
    pub body: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProgressEvent {
    pub stage: String,
    pub downloaded: u64,
    pub total: Option<u64>,
    pub percent: Option<u8>,
}

pub fn compute_progress_percent(downloaded: u64, total: Option<u64>) -> Option<u8> {
    total
        .filter(|&t| t > 0)
        .map(|t| (downloaded.saturating_mul(100) / t).min(100) as u8)
}

pub fn current_app_version(app: &AppHandle) -> String {
    app.package_info().version.to_string()
}

pub async fn check_for_updates(app: &AppHandle) -> Result<UpdateCheckResult, String> {
    let updater = app
        .updater()
        .map_err(|e| format!("Не удалось инициализировать модуль обновлений: {e}"))?;
    let current_version = current_app_version(app);
    let found = updater
        .check()
        .await
        .map_err(|e| format!("Не удалось проверить обновления: {e}"))?;

    match found {
        Some(update) => Ok(UpdateCheckResult {
            available: true,
            current_version,
            version: Some(update.version),
            date: update.date.map(|d| d.to_string()),
            body: update.body,
        }),
        None => Ok(UpdateCheckResult {
            available: false,
            current_version,
            version: None,
            date: None,
            body: None,
        }),
    }
}

pub async fn download_and_install_update<F>(
    app: &AppHandle,
    before_install: F,
) -> Result<(), String>
where
    F: FnOnce() + Send + 'static,
{
    let updater = app
        .updater()
        .map_err(|e| format!("Не удалось инициализировать модуль обновлений: {e}"))?;
    let Some(update) = updater
        .check()
        .await
        .map_err(|e| format!("Не удалось проверить обновления: {e}"))?
    else {
        return Err("Установлена последняя версия".to_string());
    };

    let _ = app.emit(
        "update-progress",
        UpdateProgressEvent {
            stage: "downloading".to_string(),
            downloaded: 0,
            total: None,
            percent: Some(0),
        },
    );

    let downloaded_acc = Arc::new(AtomicU64::new(0));
    let acc_chunk = Arc::clone(&downloaded_acc);
    let app_chunk = app.clone();
    let app_finish = app.clone();
    let acc_finish = Arc::clone(&downloaded_acc);

    let bytes = update
        .download(
            move |chunk_len, content_len| {
                let done = acc_chunk.fetch_add(chunk_len as u64, Ordering::Relaxed)
                    + chunk_len as u64;
                let _ = app_chunk.emit(
                    "update-progress",
                    UpdateProgressEvent {
                        stage: "downloading".to_string(),
                        downloaded: done,
                        total: content_len,
                        percent: compute_progress_percent(done, content_len),
                    },
                );
            },
            move || {
                let total_done = acc_finish.load(Ordering::Relaxed);
                let _ = app_finish.emit(
                    "update-progress",
                    UpdateProgressEvent {
                        stage: "installing".to_string(),
                        downloaded: total_done,
                        total: (total_done > 0).then_some(total_done),
                        percent: Some(100),
                    },
                );
            },
        )
        .await
        .map_err(|e| format!("Не удалось скачать обновление: {e}"))?;

    before_install();
    update
        .install(bytes)
        .map_err(|e| format!("Не удалось установить обновление: {e}"))?;

    #[allow(unreachable_code)]
    {
        app.restart();
    }
    #[allow(unreachable_code)]
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_bounded_download_progress_percentages() {
        assert_eq!(compute_progress_percent(0, None), None);
        assert_eq!(compute_progress_percent(500, Some(0)), None);
        assert_eq!(compute_progress_percent(0, Some(1000)), Some(0));
        assert_eq!(compute_progress_percent(250, Some(1000)), Some(25));
        assert_eq!(compute_progress_percent(1000, Some(1000)), Some(100));
        assert_eq!(compute_progress_percent(1500, Some(1000)), Some(100));
    }
}
