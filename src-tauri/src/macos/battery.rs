use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const BATTERY_CACHE_TTL: Duration = Duration::from_secs(45);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BatterySubLevel {
    pub label: String,
    pub percent: u8,
    pub charging: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PeripheralBattery {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub percent: u8,
    pub charging: bool,
    pub source: String,
    pub connected: bool,
    pub sub_levels: Option<Vec<BatterySubLevel>>,
}

static BATTERY_CACHE: OnceLock<Mutex<Option<(Instant, Vec<PeripheralBattery>)>>> = OnceLock::new();
static BATTERY_SCAN_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn cache_lock() -> &'static Mutex<Option<(Instant, Vec<PeripheralBattery>)>> {
    BATTERY_CACHE.get_or_init(|| Mutex::new(None))
}

fn scan_lock() -> &'static Mutex<()> {
    BATTERY_SCAN_LOCK.get_or_init(|| Mutex::new(()))
}

/// Classifies peripheral device kind (`"mouse" | "keyboard" | "headphones" | "gamepad" | "audio" | "other"`).
pub fn classify_peripheral_kind(name: &str, minor_type_hint: Option<&str>) -> &'static str {
    let low = format!(
        "{} {}",
        name.to_ascii_lowercase(),
        minor_type_hint.unwrap_or("").to_ascii_lowercase()
    );
    if low.contains("gamepad")
        || low.contains("controller")
        || low.contains("xbox")
        || low.contains("dualsense")
        || low.contains("dualshock")
        || low.contains("joy-con")
    {
        "gamepad"
    } else if low.contains("keyboard") || low.contains("keychron") || low.contains("mx keys") {
        "keyboard"
    } else if low.contains("airpods")
        || low.contains("head")
        || low.contains("beats")
        || low.contains("buds")
        || low.contains("pods")
        || low.contains("bose")
        || low.contains("sony")
        || low.contains("wh-1000")
        || low.contains("wf-1000")
        || low.contains("sennheiser")
        || low.contains("jabra")
    {
        "headphones"
    } else if low.contains("speaker") || low.contains("audio") {
        "audio"
    } else if low.contains("mouse")
        || low.contains("trackpad")
        || low.contains("mx master")
        || low.contains("mx anywhere")
        || low.contains("superlight")
        || low.contains("viper")
        || low.contains("basilisk")
    {
        "mouse"
    } else {
        "other"
    }
}

fn slugify_id(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

/// Parses `pmset -g batt` output for the MacBook internal battery (`InternalBattery-0`).
pub fn parse_pmset_batt(output: &str) -> Option<PeripheralBattery> {
    for line in output.lines() {
        if !line.contains("InternalBattery") {
            continue;
        }
        let pct_idx = line.find('%')?;
        let prefix = &line[..pct_idx];
        let digits: String = prefix
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        let percent = digits.parse::<u8>().ok()?.min(100);
        let low = line.to_ascii_lowercase();
        let charging = (low.contains("; charging") || low.contains("; finishing charge"))
            && !low.contains("discharging")
            && !low.contains("not charging");
        return Some(PeripheralBattery {
            id: "mac-internal".to_string(),
            name: "MacBook".to_string(),
            kind: "other".to_string(),
            percent,
            charging,
            source: "pmset".to_string(),
            connected: true,
            sub_levels: None,
        });
    }
    None
}

fn extract_ioreg_quoted(line: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    if !line.contains(&needle) {
        return None;
    }
    let (_, rhs) = line.split_once('=')?;
    let trimmed = rhs.trim().trim_matches('"').trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn extract_ioreg_u8(line: &str, key: &str) -> Option<u8> {
    let needle = format!("\"{key}\"");
    if !line.contains(&needle) {
        return None;
    }
    let (_, rhs) = line.split_once('=')?;
    rhs.trim().parse::<u16>().ok().map(|v| v.min(100) as u8)
}

/// Parses `ioreg -r -c AppleDeviceManagementHIDEventService` output into Bluetooth/HID batteries.
pub fn parse_ioreg_hid_batteries(output: &str) -> Vec<PeripheralBattery> {
    #[derive(Default)]
    struct Block {
        product: Option<String>,
        percent: Option<u8>,
        addr: Option<String>,
        serial: Option<String>,
    }

    let flush_block = |b: Block, out: &mut Vec<PeripheralBattery>, seen: &mut HashSet<String>| {
        let (Some(name), Some(percent)) = (b.product, b.percent) else {
            return;
        };
        let ident = b
            .addr
            .or(b.serial)
            .unwrap_or_else(|| slugify_id(&name));
        let id = format!("hid-{}", slugify_id(&ident));
        if id == "hid-" || !seen.insert(id.clone()) {
            return;
        }
        out.push(PeripheralBattery {
            id,
            kind: classify_peripheral_kind(&name, None).to_string(),
            name,
            percent,
            charging: false,
            source: "ioreg".to_string(),
            connected: true,
            sub_levels: None,
        });
    };

    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut cur = Block::default();

    for raw_line in output.lines() {
        let line = raw_line.trim();
        if line.starts_with("+-o ") {
            flush_block(std::mem::take(&mut cur), &mut out, &mut seen);
            continue;
        }
        if let Some(p) = extract_ioreg_quoted(line, "Product") {
            cur.product = Some(p);
        } else if let Some(pct) = extract_ioreg_u8(line, "BatteryPercent") {
            cur.percent = Some(pct);
        } else if let Some(addr) = extract_ioreg_quoted(line, "DeviceAddress") {
            cur.addr = Some(addr);
        } else if let Some(ser) = extract_ioreg_quoted(line, "SerialNumber") {
            cur.serial = Some(ser);
        }
    }
    flush_block(cur, &mut out, &mut seen);
    out
}

fn parse_pct_value(val: &serde_json::Value) -> Option<u8> {
    match val {
        serde_json::Value::Number(n) => n.as_u64().map(|v| v.min(100) as u8),
        serde_json::Value::String(s) => {
            let clean = s.trim().trim_end_matches('%').trim();
            clean.parse::<u16>().ok().map(|v| v.min(100) as u8)
        }
        _ => None,
    }
}

/// Parses `system_profiler SPBluetoothDataType -json` (catching AirPods L/R/Case & BT peripherals).
pub fn parse_sp_bluetooth_json(json_str: &str) -> Vec<PeripheralBattery> {
    let Ok(root) = serde_json::from_str::<serde_json::Value>(json_str) else {
        return Vec::new();
    };
    let Some(bt_sections) = root.get("SPBluetoothDataType").and_then(|v| v.as_array()) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    let mut seen = HashSet::new();

    for section in bt_sections {
        let Some(connected) = section.get("device_connected").and_then(|v| v.as_array()) else {
            continue;
        };
        for item in connected {
            let Some(map) = item.as_object() else {
                continue;
            };
            for (dev_name, props) in map {
                let main_pct = props
                    .get("device_batteryLevelMain")
                    .and_then(parse_pct_value);
                let left_pct = props
                    .get("device_batteryLevelLeft")
                    .and_then(parse_pct_value);
                let right_pct = props
                    .get("device_batteryLevelRight")
                    .and_then(parse_pct_value);
                let case_pct = props
                    .get("device_batteryLevelCase")
                    .and_then(parse_pct_value);

                let mut sub_levels = Vec::new();
                if let Some(p) = left_pct {
                    sub_levels.push(BatterySubLevel {
                        label: "Л".to_string(),
                        percent: p,
                        charging: false,
                    });
                }
                if let Some(p) = right_pct {
                    sub_levels.push(BatterySubLevel {
                        label: "П".to_string(),
                        percent: p,
                        charging: false,
                    });
                }
                if let Some(p) = case_pct {
                    sub_levels.push(BatterySubLevel {
                        label: "Кейс".to_string(),
                        percent: p,
                        charging: false,
                    });
                }

                let effective_pct = main_pct.or_else(|| match (left_pct, right_pct) {
                    (Some(l), Some(r)) => Some(l.min(r)),
                    (Some(l), None) => Some(l),
                    (None, Some(r)) => Some(r),
                    _ => None,
                });
                let Some(percent) = effective_pct else {
                    continue;
                };

                let addr = props
                    .get("device_address")
                    .and_then(|v| v.as_str())
                    .unwrap_or(dev_name);
                let id = format!("bt-{}", slugify_id(addr));
                if id == "bt-" || !seen.insert(id.clone()) {
                    continue;
                }
                let minor = props.get("device_minorType").and_then(|v| v.as_str());
                let kind = classify_peripheral_kind(dev_name, minor).to_string();

                out.push(PeripheralBattery {
                    id,
                    name: dev_name.trim().to_string(),
                    kind,
                    percent,
                    charging: false,
                    source: "bluetooth".to_string(),
                    connected: true,
                    sub_levels: (!sub_levels.is_empty()).then_some(sub_levels),
                });
            }
        }
    }
    out
}

/// Merges internal battery, IORegistry HID, and Bluetooth profiler entries without name duplicates.
pub fn merge_peripheral_batteries(
    internal: Option<PeripheralBattery>,
    ioreg: Vec<PeripheralBattery>,
    bt: Vec<PeripheralBattery>,
) -> Vec<PeripheralBattery> {
    let mut out = Vec::new();
    let mut by_name: HashMap<String, usize> = std::collections::HashMap::new();

    if let Some(mac) = internal {
        by_name.insert(mac.name.to_lowercase(), out.len());
        out.push(mac);
    }

    // Prefer Bluetooth profiler entries when sub_levels are present; merge seamlessly with ioreg.
    for dev in bt {
        let key = dev.name.trim().to_lowercase();
        if !by_name.contains_key(&key) {
            by_name.insert(key, out.len());
            out.push(dev);
        }
    }
    for dev in ioreg {
        let key = dev.name.trim().to_lowercase();
        if !by_name.contains_key(&key) {
            by_name.insert(key, out.len());
            out.push(dev);
        }
    }

    out
}

fn run_battery_scan() -> Vec<PeripheralBattery> {
    let internal = Command::new("pmset")
        .args(["-g", "batt"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| parse_pmset_batt(&String::from_utf8_lossy(&o.stdout)));

    let ioreg = Command::new("ioreg")
        .args(["-r", "-c", "AppleDeviceManagementHIDEventService"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| parse_ioreg_hid_batteries(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default();

    let bt = Command::new("system_profiler")
        .args(["SPBluetoothDataType", "-json"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| parse_sp_bluetooth_json(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default();

    merge_peripheral_batteries(internal, ioreg, bt)
}

/// Returns cached MacBook + Bluetooth peripheral battery levels (45s TTL).
pub fn get_peripheral_batteries() -> Vec<PeripheralBattery> {
    {
        let guard = cache_lock().lock().unwrap_or_else(|e| e.into_inner());
        if let Some((fetched_at, ref items)) = *guard {
            if fetched_at.elapsed() < BATTERY_CACHE_TTL {
                return items.clone();
            }
        }
    }
    let _scan = scan_lock().lock().unwrap_or_else(|e| e.into_inner());
    {
        let guard = cache_lock().lock().unwrap_or_else(|e| e.into_inner());
        if let Some((fetched_at, ref items)) = *guard {
            if fetched_at.elapsed() < BATTERY_CACHE_TTL {
                return items.clone();
            }
        }
    }
    let fresh = run_battery_scan();
    let mut guard = cache_lock().lock().unwrap_or_else(|e| e.into_inner());
    *guard = Some((Instant::now(), fresh.clone()));
    fresh
}

/// Bypasses the 45s cache and immediately rescans MacBook + Bluetooth battery levels.
pub fn refresh_peripheral_batteries() -> Vec<PeripheralBattery> {
    let _scan = scan_lock().lock().unwrap_or_else(|e| e.into_inner());
    let fresh = run_battery_scan();
    let mut guard = cache_lock().lock().unwrap_or_else(|e| e.into_inner());
    *guard = Some((Instant::now(), fresh.clone()));
    fresh
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pmset_internal_battery_states() {
        let discharging = "\
Now drawing from 'Battery Power'
 -InternalBattery-0 (id=9961571)	84%; discharging; 5:12 remaining present: true
";
        let batt = parse_pmset_batt(discharging).unwrap();
        assert_eq!(batt.id, "mac-internal");
        assert_eq!(batt.kind, "other");
        assert_eq!(batt.percent, 84);
        assert!(!batt.charging);

        let charging = "\
Now drawing from 'AC Power'
 -InternalBattery-0 (id=9961571)	62%; charging; 0:45 remaining present: true
";
        let batt2 = parse_pmset_batt(charging).unwrap();
        assert_eq!(batt2.percent, 62);
        assert!(batt2.charging);

        let mac_mini = "Now drawing from 'AC Power'\n";
        assert_eq!(parse_pmset_batt(mac_mini), None);
    }

    #[test]
    fn parses_ioreg_and_system_profiler_bluetooth_and_dedupes() {
        let ioreg_sample = r#"
+-o AppleDeviceManagementHIDEventService  <class AppleDeviceManagementHIDEventService, id 0x100001>
    {
      "Product" = "Magic Mouse"
      "DeviceAddress" = "aa-bb-cc-11-22-33"
      "BatteryPercent" = 77
    }
+-o AppleDeviceManagementHIDEventService  <class AppleDeviceManagementHIDEventService, id 0x100002>
    {
      "Product" = "Keychron K2"
      "SerialNumber" = "KC123"
      "BatteryPercent" = 91
    }
"#;
        let hid = parse_ioreg_hid_batteries(ioreg_sample);
        assert_eq!(hid.len(), 2);
        assert_eq!(hid[0].kind, "mouse");
        assert_eq!(hid[0].percent, 77);
        assert_eq!(hid[1].kind, "keyboard");

        let bt_json = r#"{
          "SPBluetoothDataType": [{
            "device_connected": [
              {
                "AirPods Pro": {
                  "device_address": "11:22:33:44:55:66",
                  "device_minorType": "Headphones",
                  "device_batteryLevelLeft": "90%",
                  "device_batteryLevelRight": "80%",
                  "device_batteryLevelCase": "55%"
                }
              },
              {
                "Magic Mouse": {
                  "device_address": "aa:bb:cc:11:22:33",
                  "device_minorType": "Mouse",
                  "device_batteryLevelMain": "77%"
                }
              }
            ]
          }]
        }"#;
        let bt = parse_sp_bluetooth_json(bt_json);
        assert_eq!(bt.len(), 2);
        assert_eq!(bt[0].kind, "headphones");
        assert_eq!(bt[0].percent, 80);
        assert_eq!(bt[0].sub_levels.as_ref().map(|v| v.len()), Some(3));

        let merged = merge_peripheral_batteries(None, hid, bt);
        assert_eq!(merged.len(), 3);
    }
}
