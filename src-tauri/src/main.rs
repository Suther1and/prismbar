#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // Companion watchdog mode: waits for main process exit and restores native Windows taskbar
    if args.len() >= 3 && args[1] == "--watchdog" {
        if let Ok(pid) = args[2].parse::<u32>() {
            testbar_lib::run_watchdog_mode(pid);
        }
        return;
    }

    let hide_native = !args.iter().any(|a| a == "--no-hide-native");
    testbar_lib::run(hide_native);
}
