#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use testbar_lib::win_shell::{parse_watchdog_args, WatchdogCli};

fn main() {
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

    let hide_native = !args.iter().any(|a| a == "--no-hide-native");
    testbar_lib::run(hide_native);
}
