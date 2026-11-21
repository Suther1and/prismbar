use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr, TcpStream, UdpSocket};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use sysinfo::Networks;

const PING_COOLDOWN: Duration = Duration::from_secs(3);
const PING_TIMEOUT: Duration = Duration::from_millis(1000);
const PING_HOST: &str = "1.1.1.1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NetworkSnapshot {
    pub online: bool,
    pub kind: String,
    pub adapter_name: String,
    pub local_ip: String,
    pub rx_bytes_per_sec: u64,
    pub tx_bytes_per_sec: u64,
    pub ping_ms: Option<u32>,
    pub ping_host: String,
}

#[derive(Debug, Default)]
struct MonitorState {
    last_counters: Option<(Instant, u64, u64)>,
    last_ping_at: Option<Instant>,
    cached_ping_ms: Option<u32>,
    cached_ping_host: String,
}

static MONITOR_STATE: OnceLock<Mutex<MonitorState>> = OnceLock::new();
static HARDWARE_PORTS: OnceLock<HashMap<String, (&'static str, String)>> = OnceLock::new();

fn state_lock() -> &'static Mutex<MonitorState> {
    MONITOR_STATE.get_or_init(|| Mutex::new(MonitorState::default()))
}

/// Parses `networksetup -listallhardwareports` into `device -> (kind, port_label)`.
pub fn parse_hardware_ports(output: &str) -> HashMap<String, (&'static str, String)> {
    let mut map = HashMap::new();
    let mut current_port: Option<String> = None;

    for raw_line in output.lines() {
        let line = raw_line.trim();
        if let Some(port) = line.strip_prefix("Hardware Port:") {
            current_port = Some(port.trim().to_string());
        } else if let Some(dev) = line.strip_prefix("Device:") {
            let dev_name = dev.trim().to_string();
            if let Some(port_name) = current_port.take() {
                if !dev_name.is_empty() {
                    let low = port_name.to_ascii_lowercase();
                    let kind = if low.contains("wi-fi") || low.contains("wifi") || low.contains("airport") {
                        "wifi"
                    } else {
                        "ethernet"
                    };
                    map.insert(dev_name, (kind, port_name));
                }
            }
        }
    }
    map
}

fn hardware_ports_map() -> &'static HashMap<String, (&'static str, String)> {
    HARDWARE_PORTS.get_or_init(|| {
        Command::new("networksetup")
            .arg("-listallhardwareports")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| parse_hardware_ports(&String::from_utf8_lossy(&o.stdout)))
            .unwrap_or_default()
    })
}

/// Returns `true` for virtual/loopback/tunnel macOS interfaces excluded from primary NIC selection.
pub fn is_ignored_mac_interface(name: &str) -> bool {
    let low = name.trim().to_ascii_lowercase();
    low.is_empty()
        || low.starts_with("lo")
        || low.starts_with("utun")
        || low.starts_with("awdl")
        || low.starts_with("llw")
        || low.starts_with("anpi")
        || low.starts_with("gif")
        || low.starts_with("stf")
        || low.starts_with("ap")
        || low.starts_with("bridge")
        || low.starts_with("p2p")
        || low.starts_with("xhc")
}

fn interface_en_index(name: &str) -> usize {
    name.strip_prefix("en")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(usize::MAX)
}

/// Computes `(rx_bytes_per_sec, tx_bytes_per_sec)` safely across counter resets or short ticks.
pub fn compute_throughput_rates(
    prev: Option<(Instant, u64, u64)>,
    now: Instant,
    cur_in: u64,
    cur_out: u64,
) -> (u64, u64) {
    let Some((prev_at, prev_in, prev_out)) = prev else {
        return (0, 0);
    };
    let Some(elapsed) = now.checked_duration_since(prev_at) else {
        return (0, 0);
    };
    let secs = elapsed.as_secs_f64();
    if secs < 0.05 || cur_in < prev_in || cur_out < prev_out {
        return (0, 0);
    }
    let rx = ((cur_in - prev_in) as f64 / secs).round().max(0.0) as u64;
    let tx = ((cur_out - prev_out) as f64 / secs).round().max(0.0) as u64;
    (rx, tx)
}

/// Selects the primary macOS network interface and totals traffic across physical `en*` NICs.
pub fn summarize_mac_interfaces(
    samples: &[(&str, u64, u64)],
    hw_ports: &HashMap<String, (&'static str, String)>,
) -> (&'static str, String, u64, u64) {
    let mut en_candidates: Vec<(&str, u64, u64)> = Vec::new();
    let mut total_rx = 0u64;
    let mut total_tx = 0u64;

    for &(name, rx, tx) in samples {
        if is_ignored_mac_interface(name) {
            continue;
        }
        if name.starts_with("en") {
            total_rx = total_rx.saturating_add(rx);
            total_tx = total_tx.saturating_add(tx);
            en_candidates.push((name, rx, tx));
        }
    }

    en_candidates.sort_by(|a, b| {
        let a_active = (a.1 + a.2) > 0;
        let b_active = (b.1 + b.2) > 0;
        b_active
            .cmp(&a_active)
            .then_with(|| (a.0 != "en0").cmp(&(b.0 != "en0")))
            .then_with(|| (b.1 + b.2).cmp(&(a.1 + a.2)))
            .then_with(|| interface_en_index(a.0).cmp(&interface_en_index(b.0)))
    });

    if let Some(&(primary_name, _, _)) = en_candidates.first() {
        let (kind, label) = hw_ports
            .get(primary_name)
            .map(|(k, port)| (*k, format!("{port} ({primary_name})")))
            .unwrap_or_else(|| {
                let kind = if primary_name == "en0" {
                    "wifi"
                } else {
                    "ethernet"
                };
                (kind, primary_name.to_string())
            });
        return (kind, label, total_rx, total_tx);
    }

    ("ethernet", "Сеть".to_string(), 0, 0)
}

fn detect_local_ip() -> Option<String> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("1.1.1.1:80").ok()?;
    let addr = socket.local_addr().ok()?;
    let ip: IpAddr = addr.ip();
    if ip.is_loopback() || ip.is_unspecified() {
        None
    } else {
        Some(ip.to_string())
    }
}

fn measure_tcp_ping_ms() -> Option<u32> {
    let target: SocketAddr = "1.1.1.1:443".parse().ok()?;
    let start = Instant::now();
    match TcpStream::connect_timeout(&target, PING_TIMEOUT) {
        Ok(stream) => {
            let _ = stream.shutdown(std::net::Shutdown::Both);
            Some(start.elapsed().as_millis().max(1) as u32)
        }
        Err(_) => None,
    }
}

/// Samples macOS network interface status, local IP, live throughput, and optional TCP ping.
pub fn get_network_snapshot(probe_ping: bool) -> NetworkSnapshot {
    let now = Instant::now();
    let local_ip_opt = detect_local_ip();
    let online = local_ip_opt.is_some();

    let networks = Networks::new_with_refreshed_list();
    let raw_samples: Vec<(&str, u64, u64)> = networks
        .list()
        .iter()
        .map(|(name, data)| (name.as_str(), data.total_received(), data.total_transmitted()))
        .collect();

    let (kind_raw, adapter_raw, total_in, total_out) =
        summarize_mac_interfaces(&raw_samples, hardware_ports_map());

    let (rx_bytes_per_sec, tx_bytes_per_sec) = {
        let mut guard = state_lock().lock().unwrap_or_else(|e| e.into_inner());
        let rates = if online {
            compute_throughput_rates(guard.last_counters, now, total_in, total_out)
        } else {
            (0, 0)
        };
        guard.last_counters = if online {
            Some((now, total_in, total_out))
        } else {
            None
        };
        rates
    };

    if !online {
        let mut guard = state_lock().lock().unwrap_or_else(|e| e.into_inner());
        guard.cached_ping_ms = None;
        guard.last_ping_at = None;
        return NetworkSnapshot {
            online: false,
            kind: "offline".to_string(),
            adapter_name: "Нет подключения".to_string(),
            local_ip: String::new(),
            rx_bytes_per_sec: 0,
            tx_bytes_per_sec: 0,
            ping_ms: None,
            ping_host: String::new(),
        };
    }

    let should_probe = if probe_ping {
        let guard = state_lock().lock().unwrap_or_else(|e| e.into_inner());
        guard
            .last_ping_at
            .map(|t| now.saturating_duration_since(t) >= PING_COOLDOWN)
            .unwrap_or(true)
    } else {
        false
    };

    let (ping_ms, ping_host) = if should_probe {
        let measured = measure_tcp_ping_ms();
        let mut guard = state_lock().lock().unwrap_or_else(|e| e.into_inner());
        guard.last_ping_at = Some(Instant::now());
        guard.cached_ping_ms = measured;
        guard.cached_ping_host = PING_HOST.to_string();
        (measured, PING_HOST.to_string())
    } else {
        let guard = state_lock().lock().unwrap_or_else(|e| e.into_inner());
        let host = if guard.cached_ping_host.is_empty() {
            PING_HOST.to_string()
        } else {
            guard.cached_ping_host.clone()
        };
        (guard.cached_ping_ms, host)
    };

    NetworkSnapshot {
        online: true,
        kind: kind_raw.to_string(),
        adapter_name: adapter_raw,
        local_ip: local_ip_opt.unwrap_or_default(),
        rx_bytes_per_sec,
        tx_bytes_per_sec,
        ping_ms,
        ping_host,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_networksetup_hardware_ports() {
        let sample = "\
Hardware Port: Wi-Fi
Device: en0
Ethernet Address: aa:bb:cc:dd:ee:ff

Hardware Port: USB 10/100/1000 LAN
Device: en5
Ethernet Address: 11:22:33:44:55:66
";
        let ports = parse_hardware_ports(sample);
        assert_eq!(ports.get("en0"), Some(&("wifi", "Wi-Fi".to_string())));
        assert_eq!(
            ports.get("en5"),
            Some(&("ethernet", "USB 10/100/1000 LAN".to_string()))
        );
    }

    #[test]
    fn ignores_virtual_interfaces_and_prefers_active_en0_or_physical_nic() {
        let mut ports = HashMap::new();
        ports.insert("en0".to_string(), ("wifi", "Wi-Fi".to_string()));
        ports.insert("en5".to_string(), ("ethernet", "Ethernet".to_string()));

        let samples = vec![
            ("lo0", 999_999, 999_999),
            ("utun4", 500_000, 100_000),
            ("awdl0", 10_000, 10_000),
            ("en0", 120_000, 30_000),
            ("en5", 40_000, 10_000),
        ];
        let (kind, name, rx, tx) = summarize_mac_interfaces(&samples, &ports);
        assert_eq!(kind, "wifi");
        assert_eq!(name, "Wi-Fi (en0)");
        assert_eq!(rx, 160_000);
        assert_eq!(tx, 40_000);
    }

    #[test]
    fn computes_throughput_rates_safely() {
        let t0 = Instant::now();
        let t1 = t0 + Duration::from_secs(2);
        assert_eq!(compute_throughput_rates(None, t1, 1000, 500), (0, 0));
        assert_eq!(
            compute_throughput_rates(Some((t0, 1000, 500)), t1, 3000, 1500),
            (1000, 500)
        );
        // Counter reset must return zero instead of underflowing
        assert_eq!(
            compute_throughput_rates(Some((t0, 5000, 500)), t1, 100, 500),
            (0, 0)
        );
    }
}
