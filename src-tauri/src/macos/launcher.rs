use std::path::{Path, PathBuf};
use std::process::Command;

use crate::macos::bridge_ffi;

fn is_url_like(target: &str) -> bool {
    let low = target.trim().to_ascii_lowercase();
    low.starts_with("http://")
        || low.starts_with("https://")
        || low.starts_with("x-apple.systempreferences:")
        || low.starts_with("mailto:")
        || low.starts_with("file://")
}

fn expand_tilde(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed == "~" {
        return dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("/"))
            .to_string_lossy()
            .to_string();
    }
    if let Some(rest) = trimmed.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest).to_string_lossy().to_string();
        }
    }
    trimmed.to_string()
}

fn is_app_bundle(target: &str) -> bool {
    let cleaned = target.trim().trim_end_matches('/');
    cleaned.to_ascii_lowercase().ends_with(".app")
}

/// Splits shell-like argument strings preserving single and double quoted segments.
pub fn split_cli_args(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut chars = raw.chars().peekable();

    while let Some(ch) = chars.next() {
        match quote {
            Some(q) => {
                if ch == q {
                    quote = None;
                } else if ch == '\\' && q == '"' {
                    if let Some(&next) = chars.peek() {
                        if next == '"' || next == '\\' {
                            cur.push(chars.next().unwrap());
                            continue;
                        }
                    }
                    cur.push(ch);
                } else {
                    cur.push(ch);
                }
            }
            None => {
                if ch == '"' || ch == '\'' {
                    quote = Some(ch);
                } else if ch.is_whitespace() {
                    if !cur.is_empty() {
                        out.push(std::mem::take(&mut cur));
                    }
                } else {
                    cur.push(ch);
                }
            }
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Resolves macOS terminal app name (`"Terminal"`, `"iTerm"`, `"Warp"`, `"Ghostty"`).
pub fn resolve_terminal_app_name<F>(terminal_pref: &str, exists_fn: F) -> &'static str
where
    F: Fn(&str) -> bool,
{
    match terminal_pref.trim().to_ascii_lowercase().as_str() {
        "terminal" => "Terminal",
        "iterm" | "iterm2" => "iTerm",
        "warp" => "Warp",
        "ghostty" => "Ghostty",
        _ => {
            if exists_fn("Warp") {
                "Warp"
            } else if exists_fn("Ghostty") {
                "Ghostty"
            } else if exists_fn("iTerm") {
                "iTerm"
            } else {
                "Terminal"
            }
        }
    }
}

fn terminal_bundle_installed(app_name: &str) -> bool {
    let sys_path = format!("/Applications/{app_name}.app");
    if Path::new(&sys_path).exists() {
        return true;
    }
    if let Some(home) = dirs::home_dir() {
        if home.join("Applications").join(format!("{app_name}.app")).exists() {
            return true;
        }
    }
    false
}

/// Launches a URL, macOS `.app` bundle, folder, document, or executable.
pub fn launch_item(target: &str, args: Option<&str>) -> Result<(), String> {
    let trimmed = target.trim();
    if trimmed.is_empty() {
        return Err("Пустая цель запуска".to_string());
    }

    if is_url_like(trimmed) {
        Command::new("open")
            .arg(trimmed)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Не удалось открыть ссылку «{trimmed}»: {e}"))?;
        return Ok(());
    }

    let resolved = expand_tilde(trimmed);
    let extra_args = args
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(split_cli_args)
        .unwrap_or_default();

    let mut cmd = Command::new("open");
    if is_app_bundle(&resolved) && !extra_args.is_empty() {
        cmd.arg("-a").arg(&resolved).arg("--args").args(&extra_args);
    } else if !extra_args.is_empty() && Path::new(&resolved).is_file() {
        let mut bin_cmd = Command::new(&resolved);
        if let Some(parent) = Path::new(&resolved).parent() {
            if parent.is_dir() {
                bin_cmd.current_dir(parent);
            }
        }
        bin_cmd.args(&extra_args);
        bin_cmd
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Не удалось запустить «{resolved}»: {e}"))?;
        return Ok(());
    } else {
        cmd.arg(&resolved);
    }

    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("Не удалось открыть «{resolved}»: {e}"))
}

/// Launches a new independent instance/window (`open -n`).
pub fn launch_new_window(target: &str, args: Option<&str>) -> Result<(), String> {
    let trimmed = target.trim();
    if trimmed.is_empty() {
        return Err("Пустая цель запуска".to_string());
    }

    if is_url_like(trimmed) {
        return launch_item(trimmed, None);
    }

    let resolved = expand_tilde(trimmed);
    let extra_args = args
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(split_cli_args)
        .unwrap_or_default();

    let mut cmd = Command::new("open");
    cmd.arg("-n");
    if is_app_bundle(&resolved) {
        cmd.arg("-a").arg(&resolved);
        if !extra_args.is_empty() {
            cmd.arg("--args").args(&extra_args);
        }
    } else {
        cmd.arg(&resolved);
    }

    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("Не удалось открыть новое окно «{resolved}»: {e}"))
}

/// Launches every non-empty target in sequence.
pub fn launch_combo(targets: &[String]) -> Result<(), String> {
    let mut launched = 0usize;
    let mut first_err: Option<String> = None;

    for raw in targets {
        let t = raw.trim();
        if t.is_empty() {
            continue;
        }
        match launch_item(t, None) {
            Ok(()) => launched += 1,
            Err(err) => {
                if first_err.is_none() {
                    first_err = Some(err);
                }
            }
        }
    }

    if launched == 0 {
        if let Some(err) = first_err {
            return Err(err);
        }
    }
    Ok(())
}

/// Reveals a file, folder, or `.app` bundle in Finder (`open -R`).
pub fn open_location(path: &str) -> Result<(), String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("Пустой путь".to_string());
    }
    if is_url_like(trimmed) {
        return Err("Ссылки нельзя показать в Finder".to_string());
    }
    let resolved = expand_tilde(trimmed);
    if !Path::new(&resolved).exists() {
        return Err(format!("Элемент не найден: {resolved}"));
    }
    Command::new("open")
        .arg("-R")
        .arg(&resolved)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Не удалось открыть Finder: {e}"))
}

/// Opens the configured macOS terminal application at `path` (or its parent if `path` is a file).
pub fn open_terminal_at(path: &str, terminal_pref: &str) -> Result<(), String> {
    let resolved = if path.trim().is_empty() {
        expand_tilde("~")
    } else {
        expand_tilde(path)
    };
    let target_path = Path::new(&resolved);
    let dir = if target_path.is_file() {
        target_path.parent().unwrap_or_else(|| Path::new("/"))
    } else if target_path.is_dir() {
        target_path
    } else {
        return Err(format!("Папка не найдена: {resolved}"));
    };

    let app_name = resolve_terminal_app_name(terminal_pref, terminal_bundle_installed);
    let status = Command::new("open")
        .arg("-a")
        .arg(app_name)
        .arg(dir)
        .status()
        .map_err(|e| format!("Не удалось запустить терминал {app_name}: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        // Fallback to system Terminal if a configured third-party terminal failed to open
        if app_name != "Terminal" {
            let fb = Command::new("open")
                .arg("-a")
                .arg("Terminal")
                .arg(dir)
                .status()
                .map_err(|e| format!("Не удалось запустить Terminal: {e}"))?;
            if fb.success() {
                return Ok(());
            }
        }
        Err(format!("Не удалось открыть {app_name} в папке {}", dir.display()))
    }
}

/// Executes macOS session power actions (`"lock"`, `"sleep"`, `"reboot"`, `"shutdown"`).
pub fn power_action(action: &str) -> Result<(), String> {
    match action.trim().to_ascii_lowercase().as_str() {
        "lock" => {
            if bridge_ffi::is_accessibility_trusted(false) {
                let ok = Command::new("osascript")
                    .args([
                        "-e",
                        r#"tell application "System Events" to keystroke "q" using {control down, command down}"#,
                    ])
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false);
                if ok {
                    return Ok(());
                }
            }
            let status = Command::new("pmset")
                .arg("displaysleepnow")
                .status()
                .map_err(|e| format!("Не удалось заблокировать экран: {e}"))?;
            if status.success() {
                Ok(())
            } else {
                Err("Не удалось заблокировать экран через pmset".to_string())
            }
        }
        "sleep" => {
            let status = Command::new("pmset")
                .arg("sleepnow")
                .status()
                .map_err(|e| format!("Не удалось перевести Mac в режим сна: {e}"))?;
            if status.success() {
                Ok(())
            } else {
                Err("Не удалось перевести Mac в режим сна".to_string())
            }
        }
        "reboot" => {
            let status = Command::new("osascript")
                .args(["-e", r#"tell application "System Events" to restart"#])
                .status()
                .map_err(|e| format!("Не удалось перезагрузить Mac: {e}"))?;
            if status.success() {
                Ok(())
            } else {
                Err("Команда перезагрузки отклонена системой".to_string())
            }
        }
        "shutdown" => {
            let status = Command::new("osascript")
                .args(["-e", r#"tell application "System Events" to shut down"#])
                .status()
                .map_err(|e| format!("Не удалось выключить Mac: {e}"))?;
            if status.success() {
                Ok(())
            } else {
                Err("Команда выключения отклонена системой".to_string())
            }
        }
        other => Err(format!("Неизвестное действие питания: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_quoted_arguments_cleanly() {
        assert_eq!(
            split_cli_args(r#"--profile "Work Profile" 'foo bar' --flag"#),
            vec!["--profile", "Work Profile", "foo bar", "--flag"]
        );
        assert!(split_cli_args("   ").is_empty());
    }

    #[test]
    fn terminal_preference_resolution_and_auto_fallback() {
        assert_eq!(resolve_terminal_app_name("terminal", |_| true), "Terminal");
        assert_eq!(resolve_terminal_app_name("iterm", |_| false), "iTerm");
        assert_eq!(resolve_terminal_app_name("warp", |_| false), "Warp");
        assert_eq!(resolve_terminal_app_name("ghostty", |_| false), "Ghostty");
        assert_eq!(
            resolve_terminal_app_name("auto", |name| name == "iTerm"),
            "iTerm"
        );
        assert_eq!(
            resolve_terminal_app_name("auto", |name| name == "Warp" || name == "iTerm"),
            "Warp"
        );
        assert_eq!(resolve_terminal_app_name("auto", |_| false), "Terminal");
    }

    #[test]
    fn url_and_app_bundle_detection() {
        assert!(is_url_like("https://apple.com"));
        assert!(is_url_like("x-apple.systempreferences:com.apple.Sound-Settings.extension"));
        assert!(!is_url_like("/Applications/Safari.app"));
        assert!(is_app_bundle("/Applications/Safari.app"));
        assert!(is_app_bundle("/Applications/Safari.app/"));
        assert!(!is_app_bundle("/Users/alex/file.txt"));
    }
}
