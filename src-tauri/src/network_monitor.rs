use serde::{Deserialize, Serialize};
use std::net::{IpAddr, SocketAddr, TcpStream, UdpSocket};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use windows::Win32::NetworkManagement::IpHelper::{
    FreeMibTable, GetIfTable2, MIB_IF_ROW2, MIB_IF_TABLE2,
};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;

const IF_TYPE_ETHERNET: u32 = 6;
const IF_TYPE_PPP: u32 = 23;
const IF_TYPE_LOOPBACK: u32 = 24;
const IF_TYPE_PROP_VIRTUAL: u32 = 53;
const IF_TYPE_WIFI: u32 = 71;
const IF_TYPE_TUNNEL: u32 = 131;
const IF_TYPE_WWANPP: u32 = 243;
const IF_TYPE_WWANPP2: u32 = 244;

const PING_COOLDOWN: Duration = Duration::from_secs(3);
const PING_TIMEOUT: Duration = Duration::from_millis(1000);

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

#[derive(Debug, Clone)]
pub struct AdapterSample {
    pub is_up: bool,
    pub is_hardware: bool,
    pub is_filter: bool,
    pub if_type: u32,
    pub link_speed: u64,
    pub in_octets: u64,
    pub out_octets: u64,
    pub name: String,
}

#[derive(Debug, Default)]
struct MonitorState {
    last_counters: Option<(Instant, u64, u64)>,
    last_ping_at: Option<Instant>,
    cached_ping_ms: Option<u32>,
    cached_ping_host: String,
}

static MONITOR_STATE: OnceLock<Mutex<MonitorState>> = OnceLock::new();

fn state_lock() -> &'static Mutex<MonitorState> {
    MONITOR_STATE.get_or_init(|| Mutex::new(MonitorState::default()))
}

struct MibTableGuard(*mut MIB_IF_TABLE2);

impl Drop for MibTableGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                FreeMibTable(self.0 as *const _);
            }
        }
    }
}

fn utf16_array_to_string(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end]).trim().to_string()
}

fn known_finite_speed(speed: u64) -> u64 {
    if speed == u64::MAX || speed > 1_000_000_000_000 {
        0
    } else {
        speed
    }
}

/// Computes `(rx_bytes_per_sec, tx_bytes_per_sec)` safely across counter resets or ultra-short ticks.
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

/// Aggregates active non-loopback network adapters into `(online, kind, adapter_name, total_in, total_out)`.
pub fn summarize_adapter_samples(samples: &[AdapterSample]) -> (bool, &'static str, String, u64, u64) {
    let mut best_hw: Option<&AdapterSample> = None;
    let mut hw_in = 0u64;
    let mut hw_out = 0u64;

    for s in samples {
        if !s.is_up || !s.is_hardware || s.is_filter {
            continue;
        }
        if s.if_type != IF_TYPE_ETHERNET && s.if_type != IF_TYPE_WIFI {
            continue;
        }
        hw_in = hw_in.saturating_add(s.in_octets);
        hw_out = hw_out.saturating_add(s.out_octets);
        let take = match best_hw {
            None => true,
            Some(prev) => {
                (s.in_octets + s.out_octets > prev.in_octets + prev.out_octets
                    && known_finite_speed(s.link_speed) >= known_finite_speed(prev.link_speed))
                    || known_finite_speed(s.link_speed) > known_finite_speed(prev.link_speed)
            }
        };
        if take {
            best_hw = Some(s);
        }
    }

    if let Some(primary) = best_hw {
        let kind = if primary.if_type == IF_TYPE_WIFI {
            "wifi"
        } else {
            "ethernet"
        };
        return (true, kind, primary.name.clone(), hw_in, hw_out);
    }

    let mut best_fb: Option<&AdapterSample> = None;
    let mut fb_in = 0u64;
    let mut fb_out = 0u64;
    for s in samples {
        if !s.is_up || s.is_filter || s.if_type == IF_TYPE_LOOPBACK {
            continue;
        }
        let supported = matches!(
            s.if_type,
            IF_TYPE_ETHERNET
                | IF_TYPE_WIFI
                | IF_TYPE_PPP
                | IF_TYPE_PROP_VIRTUAL
                | IF_TYPE_TUNNEL
                | IF_TYPE_WWANPP
                | IF_TYPE_WWANPP2
        );
        if !supported {
            continue;
        }
        // Skip idle synthetic/virtual interfaces with zero transferred octets when picking traffic
        if !s.is_hardware && s.in_octets == 0 && s.out_octets == 0 {
            continue;
        }
        fb_in = fb_in.saturating_add(s.in_octets);
        fb_out = fb_out.saturating_add(s.out_octets);
        let take = match best_fb {
            None => true,
            Some(prev) => {
                let s_nic = s.if_type == IF_TYPE_ETHERNET || s.if_type == IF_TYPE_WIFI;
                let p_nic = prev.if_type == IF_TYPE_ETHERNET || prev.if_type == IF_TYPE_WIFI;
                (s_nic && !p_nic)
                    || (s_nic == p_nic
                        && (s.in_octets + s.out_octets) > (prev.in_octets + prev.out_octets))
            }
        };
        if take {
            best_fb = Some(s);
        }
    }

    if let Some(fb) = best_fb {
        let kind = if fb.if_type == IF_TYPE_WIFI {
            "wifi"
        } else {
            "ethernet"
        };
        return (true, kind, fb.name.clone(), fb_in, fb_out);
    }

    (false, "offline", "Нет подключения".to_string(), 0, 0)
}

fn collect_adapter_samples() -> Vec<AdapterSample> {
    unsafe {
        let mut table_ptr: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
        if GetIfTable2(&mut table_ptr).is_err() || table_ptr.is_null() {
            return Vec::new();
        }
        let _guard = MibTableGuard(table_ptr);
        let table = &*table_ptr;
        let rows: &[MIB_IF_ROW2] =
            std::slice::from_raw_parts(table.Table.as_ptr(), table.NumEntries as usize);

        rows.iter()
            .map(|row| {
                let bits = row.InterfaceAndOperStatusFlags._bitfield;
                let alias = utf16_array_to_string(&row.Alias);
                let desc = utf16_array_to_string(&row.Description);
                let name = if !alias.is_empty() { alias } else { desc };
                AdapterSample {
                    is_up: row.OperStatus == IfOperStatusUp,
                    is_hardware: (bits & 0x01) != 0,
                    is_filter: (bits & 0x02) != 0,
                    if_type: row.Type,
                    link_speed: row.TransmitLinkSpeed,
                    in_octets: row.InOctets,
                    out_octets: row.OutOctets,
                    name,
                }
            })
            .collect()
    }
}

fn detect_local_ipv4() -> String {
    let Ok(sock) = UdpSocket::bind("0.0.0.0:0") else {
        return String::new();
    };
    if sock.connect("1.1.1.1:80").is_err() {
        return String::new();
    }
    match sock.local_addr() {
        Ok(SocketAddr::V4(addr)) if !addr.ip().is_loopback() => addr.ip().to_string(),
        Ok(SocketAddr::V6(addr)) => match IpAddr::V6(*addr.ip()) {
            IpAddr::V6(v6) => v6.to_string(),
            _ => String::new(),
        },
        _ => String::new(),
    }
}

fn measure_tcp_ping() -> Option<(u32, &'static str)> {
    let targets: [(&str, SocketAddr); 2] = [
        ("1.1.1.1", SocketAddr::from(([1, 1, 1, 1], 443))),
        ("8.8.8.8", SocketAddr::from(([8, 8, 8, 8], 53))),
    ];
    for (host, addr) in targets {
        let start = Instant::now();
        if TcpStream::connect_timeout(&addr, PING_TIMEOUT).is_ok() {
            let ms = (start.elapsed().as_secs_f64() * 1000.0).round().max(1.0) as u32;
            return Some((ms, host));
        }
    }
    None
}

pub fn get_network_snapshot(probe_ping: bool) -> NetworkSnapshot {
    let samples = collect_adapter_samples();
    let (online, kind, adapter_name, total_in, total_out) = summarize_adapter_samples(&samples);
    let now = Instant::now();

    let (rx_bytes_per_sec, tx_bytes_per_sec, should_probe_ping, cached_ping_ms, cached_ping_host) = {
        let mut st = state_lock().lock().unwrap_or_else(|e| e.into_inner());
        let (rx, tx) = if online {
            compute_throughput_rates(st.last_counters, now, total_in, total_out)
        } else {
            (0, 0)
        };
        st.last_counters = if online {
            Some((now, total_in, total_out))
        } else {
            None
        };
        let cooldown_ok = st
            .last_ping_at
            .is_none_or(|t| now.saturating_duration_since(t) >= PING_COOLDOWN);
        (
            rx,
            tx,
            online && probe_ping && cooldown_ok,
            if online { st.cached_ping_ms } else { None },
            if online && !st.cached_ping_host.is_empty() {
                st.cached_ping_host.clone()
            } else {
                "1.1.1.1".to_string()
            },
        )
    };

    let (ping_ms, ping_host) = if should_probe_ping {
        let probed = measure_tcp_ping();
        let mut st = state_lock().lock().unwrap_or_else(|e| e.into_inner());
        st.last_ping_at = Some(Instant::now());
        if let Some((ms, host)) = probed {
            st.cached_ping_ms = Some(ms);
            st.cached_ping_host = host.to_string();
            (Some(ms), host.to_string())
        } else {
            (st.cached_ping_ms, cached_ping_host)
        }
    } else {
        (cached_ping_ms, cached_ping_host)
    };

    let local_ip = if online {
        detect_local_ipv4()
    } else {
        String::new()
    };

    NetworkSnapshot {
        online,
        kind: kind.to_string(),
        adapter_name: if adapter_name.is_empty() {
            if online {
                "Сетевой адаптер".to_string()
            } else {
                "Нет подключения".to_string()
            }
        } else {
            adapter_name
        },
        local_ip,
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
    fn compute_throughput_handles_first_sample_short_tick_and_counter_reset() {
        let t0 = Instant::now();
        assert_eq!(compute_throughput_rates(None, t0, 1000, 500), (0, 0));

        // Too-short elapsed window (< 50ms) yields zero instead of noisy spike
        let t_short = t0 + Duration::from_millis(20);
        assert_eq!(
            compute_throughput_rates(Some((t0, 1000, 500)), t_short, 2000, 1000),
            (0, 0)
        );

        // Counter reset / rollback yields zero without underflow
        let t2 = t0 + Duration::from_secs(2);
        assert_eq!(
            compute_throughput_rates(Some((t0, 5000, 5000)), t2, 1000, 6000),
            (0, 0)
        );

        // Exact 2s delta -> 2048 B / 2s = 1024 B/s, 1024 B / 2s = 512 B/s
        assert_eq!(
            compute_throughput_rates(Some((t0, 1000, 500)), t2, 3048, 1524),
            (1024, 512)
        );
    }

    #[test]
    fn summarize_adapter_samples_prefers_hardware_nic_then_fallback() {
        let offline = summarize_adapter_samples(&[AdapterSample {
            is_up: true,
            is_hardware: false,
            is_filter: false,
            if_type: IF_TYPE_LOOPBACK,
            link_speed: 10_000_000_000,
            in_octets: 100,
            out_octets: 100,
            name: "Loopback".into(),
        }]);
        assert!(!offline.0);
        assert_eq!(offline.1, "offline");

        let wifi_and_eth = summarize_adapter_samples(&[
            AdapterSample {
                is_up: true,
                is_hardware: true,
                is_filter: false,
                if_type: IF_TYPE_WIFI,
                link_speed: 600_000_000,
                in_octets: 400,
                out_octets: 200,
                name: "Wi-Fi".into(),
            },
            AdapterSample {
                is_up: true,
                is_hardware: true,
                is_filter: false,
                if_type: IF_TYPE_ETHERNET,
                link_speed: 1_000_000_000,
                in_octets: 1000,
                out_octets: 500,
                name: "Ethernet".into(),
            },
        ]);
        assert_eq!(
            wifi_and_eth,
            (true, "ethernet", "Ethernet".into(), 1400, 700)
        );
    }

    #[test]
    fn live_network_snapshot_non_probe_does_not_panic() {
        let snap = get_network_snapshot(false);
        assert!(matches!(snap.kind.as_str(), "ethernet" | "wifi" | "offline"));
        assert!(!snap.ping_host.is_empty());
    }
}
