// SPDX-License-Identifier: MIT
//! Network and battery state read from /sys and /proc (port of device_status.cpp).

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceStatus {
    pub wifi_present: bool,
    pub wifi_connected: bool,
    pub wifi_strength_percent: i32,
    pub ethernet_present: bool,
    pub ethernet_connected: bool,
    pub battery_present: bool,
    pub battery_charging: bool,
    pub battery_percent: i32,
}

impl Default for DeviceStatus {
    fn default() -> Self {
        Self {
            wifi_present: false,
            wifi_connected: false,
            wifi_strength_percent: -1,
            ethernet_present: false,
            ethernet_connected: false,
            battery_present: false,
            battery_charging: false,
            battery_percent: -1,
        }
    }
}

/// Where the kernel exposes the information (overridable for tests).
#[derive(Debug, Clone)]
pub struct Roots {
    pub power_supply: PathBuf,
    pub net_class: PathBuf,
    pub wireless: PathBuf,
}

impl Default for Roots {
    fn default() -> Self {
        Self {
            power_supply: "/sys/class/power_supply".into(),
            net_class: "/sys/class/net".into(),
            wireless: "/proc/net/wireless".into(),
        }
    }
}

fn read_text(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.lines().next().unwrap_or("").trim().to_owned())
}

fn read_int(path: &Path, fallback: i32) -> i32 {
    read_text(path).and_then(|s| s.split_whitespace().next().and_then(|w| w.parse().ok())).unwrap_or(fallback)
}

fn read_battery(roots: &Roots, st: &mut DeviceStatus) {
    let Ok(rd) = fs::read_dir(&roots.power_supply) else { return };
    for entry in rd.flatten() {
        let p = entry.path();
        if read_text(&p.join("type")).as_deref() != Some("Battery") {
            continue;
        }
        st.battery_percent = read_int(&p.join("capacity"), -1);
        st.battery_present = st.battery_percent >= 0;
        st.battery_charging = read_text(&p.join("status")).as_deref() == Some("Charging");
        return;
    }
}

fn read_wifi_strength(roots: &Roots, st: &mut DeviceStatus, interface: &str) {
    let Ok(text) = fs::read_to_string(&roots.wireless) else { return };
    for line in text.lines() {
        let Some((name, rest)) = line.split_once(':') else { continue };
        if name.trim() != interface {
            continue;
        }
        let mut values = rest.split_whitespace();
        if let (Some(_state), Some(q)) = (values.next(), values.next().and_then(|q| q.parse::<f64>().ok())) {
            st.wifi_strength_percent = ((q * 100.0 / 70.0) as i32).clamp(0, 100);
            return;
        }
    }
}

fn read_network(roots: &Roots, st: &mut DeviceStatus) {
    let Ok(rd) = fs::read_dir(&roots.net_class) else { return };
    let mut wifi_interface = String::new();
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == "lo" {
            continue;
        }
        let p = entry.path();
        let connected = read_int(&p.join("carrier"), 0) == 1;
        if p.join("wireless").exists() {
            st.wifi_present = true;
            st.wifi_connected |= connected;
            if connected && wifi_interface.is_empty() {
                wifi_interface = name;
            }
        } else if read_int(&p.join("type"), -1) == 1 {
            st.ethernet_present = true;
            st.ethernet_connected |= connected;
        }
    }
    if !wifi_interface.is_empty() {
        read_wifi_strength(roots, st, &wifi_interface);
    }
}

pub fn read_device_status() -> DeviceStatus {
    read_device_status_from(&Roots::default())
}

pub fn read_device_status_from(roots: &Roots) -> DeviceStatus {
    let mut st = DeviceStatus::default();
    read_network(roots, &mut st);
    read_battery(roots, &mut st);
    st
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(root: &Path, rel: &str, text: &str) {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
    }

    #[test]
    fn empty_roots_give_the_defaults() {
        let d = tempfile::tempdir().unwrap();
        let roots = Roots { power_supply: d.path().join("a"), net_class: d.path().join("b"), wireless: d.path().join("c") };
        assert_eq!(read_device_status_from(&roots), DeviceStatus::default());
    }

    #[test]
    fn wifi_ethernet_and_battery() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        put(r, "net/lo/carrier", "1\n");
        put(r, "net/wlan0/carrier", "1\n");
        put(r, "net/wlan0/wireless/x", "");
        put(r, "net/eth0/carrier", "0\n");
        put(r, "net/eth0/type", "1\n");
        put(r, "wireless", "Inter-| sta-|   Quality\n face | tus | link level noise\n wlan0: 0000   35.  -60.  -256\n");
        put(r, "ps/AC/type", "Mains\n");
        put(r, "ps/BAT0/type", "Battery\n");
        put(r, "ps/BAT0/capacity", "87\n");
        put(r, "ps/BAT0/status", "Charging\n");
        let roots = Roots { power_supply: r.join("ps"), net_class: r.join("net"), wireless: r.join("wireless") };
        let st = read_device_status_from(&roots);
        assert!(st.wifi_present && st.wifi_connected);
        assert_eq!(st.wifi_strength_percent, 50); // 35 / 70
        assert!(st.ethernet_present && !st.ethernet_connected);
        assert_eq!((st.battery_present, st.battery_charging, st.battery_percent), (true, true, 87));
    }

    #[test]
    fn strength_is_clamped_and_unreadable_battery_is_absent() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        put(r, "net/wlan0/carrier", "1\n");
        put(r, "net/wlan0/wireless/x", "");
        put(r, "wireless", "h\nh\n wlan0: 0000   99.  -1.  -1\n");
        put(r, "ps/BAT0/type", "Battery\n");
        put(r, "ps/BAT0/status", "Discharging\n");
        let roots = Roots { power_supply: r.join("ps"), net_class: r.join("net"), wireless: r.join("wireless") };
        let st = read_device_status_from(&roots);
        assert_eq!(st.wifi_strength_percent, 100);
        assert!(!st.battery_present && !st.battery_charging && st.battery_percent == -1);
    }

    /// The real files of this machine must at least be readable without panicking.
    #[test]
    fn real_system_does_not_panic() {
        let _ = read_device_status();
    }
}
