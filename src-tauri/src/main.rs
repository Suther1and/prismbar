#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(target_os = "windows")]
mod windows_entry {
    use testbar_lib::win_shell::{parse_watchdog_args, WatchdogCli};

    fn configure_webview2_browser_args() {
        // Merge all 4 same-origin `http://tauri.localhost` WebViews into a single Chromium renderer
        // process (`--process-per-site`), keep hidden overlay compositors warm across SW_HIDE/SW_SHOW
        // (`CalculateNativeWinOcclusion` disabled), and strip unneeded Edge/Chromium background bloat.
        const LEAN_ARGS: &str = "--process-per-site \
            --disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,Translate,OptimizationHints,MediaRouter,DialMediaRouteProvider,InterestFeedContentSuggestions,CalculateNativeWinOcclusion \
            --disable-background-networking \
            --disable-component-update \
            --disable-extensions \
            --disable-sync";
        let merged = match std::env::var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS") {
            Ok(existing) if !existing.trim().is_empty() => format!("{} {LEAN_ARGS}", existing.trim()),
            _ => LEAN_ARGS.to_string(),
        };
        std::env::set_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", merged);
    }

    pub fn main() {
        let args: Vec<String> = std::env::args().collect();

        // Companion watchdog mode: waits for main process exit and restores native Windows taskbar
        match parse_watchdog_args(&args) {
            WatchdogCli::Run(watchdog_args) => {
                testbar_lib::run_watchdog_mode(watchdog_args);
                return;
            }
            WatchdogCli::Invalid(reason) => {
                eprintln!("Invalid watchdog arguments: {reason}");
                std::process::exit(2);
            }
            WatchdogCli::NotWatchdog => {}
        }

        configure_webview2_browser_args();
        let hide_native = !args.iter().any(|a| a == "--no-hide-native");
        testbar_lib::run(hide_native);
    }
}

#[cfg(target_os = "windows")]
fn main() {
    windows_entry::main();
}

#[cfg(not(target_os = "windows"))]
fn main() {
    testbar_lib::run(false);
}
