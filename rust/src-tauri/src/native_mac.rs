// macOS layer: the Swift helper (CoreAudio, CoreWLAN, Bluetooth, window list) plus netstat, ping and the Trash.
#![cfg(target_os = "macos")]
use crate::util::run;
use once_cell::sync::OnceCell;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

static HELPER: OnceCell<PathBuf> = OnceCell::new();

/// Finds the helper: inside the app bundle's Resources, else next to the crate (dev builds).
pub fn init_helper(resource_dir: Option<PathBuf>) {
    let mut candidates = Vec::new();
    if let Some(r) = resource_dir {
        candidates.push(r.join("MissionControlHelper.app/Contents/MacOS/helper"));
    }
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("helper/MissionControlHelper.app/Contents/MacOS/helper"));
    if let Some(p) = candidates.into_iter().find(|p| p.exists()) {
        let _ = HELPER.set(p);
    }
}

pub async fn helper(cmd: &str, args: &Value, scan: bool) -> Result<Value, String> {
    let exe = HELPER.get().ok_or("The macOS helper is missing. Rebuild with the Xcode command line tools installed.")?;
    let a = args.to_string();
    let mut argv = vec![cmd, a.as_str()];
    if scan {
        argv.push("scan");
    }
    let r = run(&exe.to_string_lossy(), &argv, if scan { 25000 } else { 15000 }).await;
    if !r.ok {
        return Err(r.err.trim().to_string());
    }
    serde_json::from_str(r.out.trim()).map_err(|e| e.to_string())
}

pub async fn apple(script: &str) -> Result<String, String> {
    let r = run("/usr/bin/osascript", &["-e", script], 15000).await;
    if r.ok { Ok(r.out.trim().to_string()) } else { Err(r.err.trim().to_string()) }
}

#[derive(Clone, serde::Serialize)]
pub struct Adapter {
    pub name: String,
    #[serde(rename = "linkBps")]
    pub link_bps: f64,
    pub wifi: bool,
}

#[derive(Clone)]
pub struct NetCounters {
    pub rx: f64,
    pub tx: f64,
    pub adapter: Option<Adapter>,
}

struct Cache {
    counters: Option<NetCounters>,
    fullscreen: bool,
    apps: HashMap<u32, String>,
    wifi_if: String,
}

static CACHE: std::sync::LazyLock<Mutex<Cache>> = std::sync::LazyLock::new(|| Mutex::new(Cache { counters: None, fullscreen: false, apps: HashMap::new(), wifi_if: String::new() }));

/// Refreshes the cached netstat / window snapshot (called once a second from a background task).
pub async fn refresh() {
    if CACHE.lock().unwrap().wifi_if.is_empty() {
        if let Ok(w) = helper("wifi", &Value::Object(Default::default()), false).await {
            CACHE.lock().unwrap().wifi_if = w.get("interface").and_then(|v| v.as_str()).unwrap_or("en0").to_string();
        }
    }
    let wifi_if = CACHE.lock().unwrap().wifi_if.clone();
    let r = run("/usr/sbin/netstat", &["-ibn"], 5000).await;
    if r.ok {
        let mut seen = std::collections::HashSet::new();
        let (mut rx, mut tx) = (0f64, 0f64);
        let mut best: Option<(f64, Adapter)> = None;
        for line in r.out.lines().skip(1) {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() < 10 {
                continue;
            }
            let name = f[0];
            let en = name.starts_with("en") && name[2..].chars().all(|c| c.is_ascii_digit()) && name.len() > 2;
            if !en || seen.contains(name) || !f[2].starts_with("<Link") {
                continue;
            }
            seen.insert(name.to_string());
            let (i, o) = (f[6].parse::<f64>().unwrap_or(0.0), f[9].parse::<f64>().unwrap_or(0.0));
            rx += i;
            tx += o;
            if best.as_ref().map(|b| i + o > b.0).unwrap_or(true) {
                best = Some((i + o, Adapter { name: name.into(), link_bps: 0.0, wifi: name == wifi_if }));
            }
        }
        CACHE.lock().unwrap().counters = Some(NetCounters { rx, tx, adapter: best.map(|b| b.1) });
    }
    if let Ok(w) = helper("windows", &Value::Object(Default::default()), false).await {
        let mut c = CACHE.lock().unwrap();
        c.fullscreen = w.get("fullscreen").and_then(|v| v.as_bool()).unwrap_or(false);
        c.apps = w
            .get("apps")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| Some((x.get("pid")?.as_u64()? as u32, x.get("name").and_then(|n| n.as_str()).unwrap_or("App").to_string())))
                    .collect()
            })
            .unwrap_or_default();
    }
}

pub fn net_counters() -> Option<NetCounters> {
    CACHE.lock().unwrap().counters.clone()
}
pub fn is_foreground_fullscreen() -> bool {
    CACHE.lock().unwrap().fullscreen
}
pub fn list_app_windows() -> HashMap<u32, String> {
    CACHE.lock().unwrap().apps.clone()
}

pub async fn ping(ip: &str, timeout_ms: u32) -> Option<f64> {
    if !ip.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return None;
    }
    let r = run("/sbin/ping", &["-n", "-c", "1", "-W", &timeout_ms.to_string(), ip], timeout_ms as u64 + 1000).await;
    let re = regex::Regex::new(r"time[=<]([\d.]+)").unwrap();
    re.captures(&r.out).and_then(|c| c[1].parse().ok())
}

fn trash() -> PathBuf {
    crate::util::home().join(".Trash")
}

/// Only this user's Trash; no shell commands, symlink traversal, or privileged deletion.
pub fn recycle_bin_info() -> (f64, f64) {
    let (mut size, mut items) = (0f64, 0f64);
    let mut stack = vec![trash()];
    while let Some(p) = stack.pop() {
        if let Ok(rd) = std::fs::read_dir(&p) {
            for e in rd.flatten() {
                if let Ok(m) = std::fs::symlink_metadata(e.path()) {
                    items += 1.0;
                    if m.is_dir() {
                        stack.push(e.path());
                    } else {
                        size += m.len() as f64;
                    }
                }
            }
        }
    }
    (size, items)
}

pub fn empty_recycle_bin() -> bool {
    match std::fs::read_dir(trash()) {
        Ok(rd) => {
            for e in rd.flatten() {
                let p = e.path();
                let _ = if p.is_dir() && !p.is_symlink() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
            }
            true
        }
        Err(_) => false,
    }
}

/// Seconds since the last keyboard / mouse input.
pub fn user_idle_secs() -> u64 {
    let out = crate::util::std_cmd("/usr/sbin/ioreg").args(["-c", "IOHIDSystem", "-d", "4"]).output();
    if let Ok(o) = out {
        let t = String::from_utf8_lossy(&o.stdout);
        if let Some(c) = regex::Regex::new(r#""HIDIdleTime"\s*=\s*(\d+)"#).unwrap().captures(&t) {
            return c[1].parse::<u64>().unwrap_or(0) / 1_000_000_000;
        }
    }
    0
}

/// Mounted volumes: "/" plus real entries under /Volumes.
pub fn disks() -> Vec<(String, f64, f64)> {
    let mut roots = vec![PathBuf::from("/")];
    if let Ok(rd) = std::fs::read_dir("/Volumes") {
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if p.is_symlink() || ["Recovery", "Preboot", "VM", "Update"].contains(&name.as_str()) {
                continue;
            }
            roots.push(p);
        }
    }
    let mut out = Vec::new();
    for r in roots {
        let c = std::ffi::CString::new(r.to_string_lossy().as_bytes()).unwrap();
        let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::statvfs(c.as_ptr(), &mut s) } == 0 && s.f_blocks > 0 {
            let bs = s.f_frsize as f64;
            out.push((r.to_string_lossy().to_string(), s.f_blocks as f64 * bs, s.f_bavail as f64 * bs));
        }
    }
    out
}
