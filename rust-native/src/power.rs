// Live power draw and electricity cost. Every part of the PC uses the best source it has: a real reading
// where the hardware reports one (Windows' energy meter for the CPU, NVIDIA's driver for its GPUs, the
// battery's own drain rate on a laptop), otherwise an estimate from load and typical power for that kind of
// part. Each figure says which it is. Wall energy is added up per day in power-log.json next to the
// settings, so the Power tab can show what was actually used as well as projections.
//
// It runs on its own thread for as long as the app runs (also while the widget idles during fullscreen
// games, which is when a PC draws the most) and never wakes the interface: the page reads `snap`.
use crate::backend::Backend;
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Src {
    /// read from the hardware
    Measured,
    /// worked out from load and typical power
    Estimated,
    /// a value the user entered
    Set,
    /// detected, but its power is already counted in another part
    Included,
}

#[derive(Clone, Debug)]
pub struct Part {
    pub kind: &'static str,
    pub name: String,
    pub watts: f32,
    pub src: Src,
    /// where the number comes from, in plain words
    pub how: String,
    /// 0..100 when known
    pub load: Option<f32>,
}

#[derive(Clone, Copy, Default, Debug)]
pub struct Day {
    /// wall energy, watt-hours
    pub wh: f64,
    /// seconds tracked
    pub s: f64,
    /// the part of `wh` that came from readings rather than estimates
    pub mwh: f64,
}

#[derive(Clone, Default, Debug)]
pub struct Summary {
    pub today: Day,
    pub last7: f64,
    pub last30: f64,
    pub last365: f64,
    /// calendar days with any tracked energy
    pub days_tracked: usize,
    /// expected wall energy per day, Wh, and what that is based on
    pub daily_wh: f64,
    pub basis: String,
    /// average wall power while the PC is on (last 30 days)
    pub avg_w: f64,
    /// hours a day the PC is on: learned from past days, or the user's setting
    pub hours: Option<f64>,
}

#[derive(Clone, Default)]
pub struct Snapshot {
    pub ready: bool,
    pub parts: Vec<Part>,
    /// power used inside the PC (before the power supply)
    pub dc_w: f32,
    /// power drawn from the wall right now (0 while a laptop runs on battery)
    pub wall_w: f32,
    /// share of `dc_w` that is read from hardware, 0..1
    pub measured: f32,
    pub laptop: bool,
    pub on_battery: bool,
    /// a laptop's measured drain while on battery, watts
    pub battery_w: Option<f32>,
    /// the history chart: wall watts (or battery drain), one sample per second
    pub hist: VecDeque<f32>,
    pub count: u64,
    pub summary: Summary,
    /// the system's currency symbol, used until the user sets one
    pub currency: String,
    pub rest_auto_w: f32,
    pub eff_auto: f32,
    /// Explains the measurement boundary; component estimates never become measured readings.
    pub source_note: String,
    pub dc_measured: bool,
}

pub struct PowerMon {
    pub snap: Mutex<Snapshot>,
    log: Mutex<Log>,
}

const HIST_N: usize = crate::app::HIST + 4;

impl PowerMon {
    pub fn start(be: Arc<Backend>) -> Arc<Self> {
        let me = Arc::new(Self { snap: Mutex::new(Snapshot { currency: hw::currency(), ..Default::default() }), log: Mutex::new(Log::load()) });
        let m = me.clone();
        let _ = std::thread::Builder::new().name("mc-power".into()).spawn(move || m.run(be));
        me
    }

    /// Write the energy log now (on quit).
    pub fn flush(&self) {
        self.log.lock().unwrap().save();
    }

    fn run(self: Arc<Self>, be: Arc<Backend>) {
        let mut sensors = hw::Sensors::new();
        let mut sys = sysinfo::System::new();
        sys.refresh_cpu_usage();
        let threads = sys.cpus().len().max(1);
        let cpu_name = sys.cpus().first().map(|c| c.brand().split_whitespace().collect::<Vec<_>>().join(" ")).unwrap_or_else(|| "CPU".into());
        let mut last = Instant::now();
        let mut last_save = Instant::now();
        std::thread::sleep(Duration::from_millis(400));
        loop {
            std::thread::sleep(Duration::from_secs(1));
            let now = Instant::now();
            // a long gap means the PC slept (or the thread was starved): count at most 2 s of it
            let dt = now.duration_since(last).as_secs_f64().min(2.0);
            last = now;
            sys.refresh_cpu_usage();
            let cpu_util = sys.global_cpu_usage().clamp(0.0, 100.0);
            let r = sensors.read();
            let cfg = Cfg::read(&be);
            let mut s = self.snap.lock().unwrap();
            compute(&mut s, &r, &cfg, cpu_util, threads, &cpu_name);
            #[cfg(target_os = "macos")]
            if let Some(currency) = &r.currency { s.currency = currency.clone(); }
            let wall = s.wall_w as f64;
            let measured = s.measured as f64;
            {
                let mut log = self.log.lock().unwrap();
                log.add(wall * dt / 3600.0, dt, measured);
                s.summary = log.summary(cfg.hours);
                if last_save.elapsed() > Duration::from_secs(30) {
                    log.save();
                    last_save = Instant::now();
                }
            }
            let shown = if s.on_battery { s.battery_w.unwrap_or(s.dc_w) } else { s.wall_w };
            s.hist.push_back(shown);
            if s.hist.len() > HIST_N {
                s.hist.pop_front();
            }
            s.count += 1;
            s.ready = true;
            if std::env::var_os("MC_DIAG_POWER").is_some() {
                let parts: Vec<String> = s.parts.iter().map(|p| format!("{} {:.1}W {:?}{}", p.name, p.watts, p.src, p.load.map(|l| format!(" load {l:.0}%")).unwrap_or_default())).collect();
                eprintln!("[power] wall {:.1}W dc {:.1}W measured {:.0}% | {} | today {:.2}Wh daily {:.0}Wh ({})", s.wall_w, s.dc_w, s.measured * 100.0, parts.join(" | "), s.summary.today.wh, s.summary.daily_wh, s.summary.basis);
            }
        }
    }
}

/// The user's numbers from settings (null = automatic).
struct Cfg {
    rest_w: Option<f32>,
    eff: Option<f32>,
    monitor_w: f32,
    hours: Option<f32>,
}

impl Cfg {
    fn read(be: &Backend) -> Self {
        let num = |k: &str| be.get(k).as_f64().map(|v| v as f32).filter(|v| v.is_finite());
        Self {
            rest_w: num("powerRestW").map(|v| v.clamp(0.0, 2000.0)),
            eff: num("powerPsuEff").map(|v| (v / 100.0).clamp(0.5, 1.0)),
            monitor_w: num("powerMonitorW").unwrap_or(0.0).clamp(0.0, 2000.0),
            hours: num("powerHours").map(|v| v.clamp(0.0, 24.0)),
        }
    }
}

/// Typical full-load board power of a graphics card with this much memory (a rough guide when the
/// card can't report its own power).
fn gpu_typical_w(vram_gb: f32) -> f32 {
    match vram_gb {
        v if v <= 2.5 => 75.0,
        v if v <= 4.5 => 100.0,
        v if v <= 6.5 => 150.0,
        v if v <= 8.5 => 180.0,
        v if v <= 12.5 => 230.0,
        v if v <= 16.5 => 280.0,
        _ => 330.0,
    }
}

fn compute(s: &mut Snapshot, r: &hw::Reading, cfg: &Cfg, cpu_util: f32, threads: usize, cpu_name: &str) {
    let laptop = r.battery.is_some_and(|b| b.present);
    let on_battery = r.battery.is_some_and(|b| b.present && !b.ac);
    let mut parts = Vec::new();

    // CPU
    let cpu_measured = r.cpu_pkg_w.is_some();
    match r.cpu_pkg_w {
        Some(w) => parts.push(Part { kind: "cpu", name: cpu_name.into(), watts: w, src: Src::Measured, how: "Windows energy meter (CPU package)".into(), load: Some(cpu_util) }),
        None => {
            let n = threads as f32;
            let (idle, max) = if cfg!(target_os = "macos") && cpu_name.starts_with("Apple") {
                (2.0, (8.0 + 2.5 * n).clamp(15.0, 140.0))
            } else if laptop { (3.0, (8.0 + 2.5 * n).clamp(15.0, 80.0)) } else { (10.0, (20.0 + 6.0 * n).clamp(45.0, 200.0)) };
            parts.push(Part {
                kind: "cpu",
                name: cpu_name.into(),
                watts: idle + (max - idle) * cpu_util / 100.0,
                src: Src::Estimated,
                how: format!("Load × typical power for {threads} threads (this CPU doesn't report its power)"),
                load: Some(cpu_util),
            });
        }
    }
    if let Some(w) = r.dram_w {
        parts.push(Part { kind: "ram", name: "Memory".into(), watts: w, src: Src::Measured, how: "Windows energy meter (DRAM)".into(), load: None });
    }

    // GPUs
    for g in &r.gpus {
        let util = g.util;
        if g.integrated {
            if cpu_measured {
                parts.push(Part { kind: "igpu", name: g.name.clone(), watts: 0.0, src: Src::Included, how: "Built-in graphics: counted in the CPU package".into(), load: util });
            } else {
                let max = if laptop { 6.0 } else { 12.0 };
                parts.push(Part { kind: "igpu", name: g.name.clone(), watts: max * util.unwrap_or(0.0) / 100.0, src: Src::Estimated, how: "Built-in graphics: load × typical power".into(), load: util });
            }
            continue;
        }
        if let Some(w) = g.power_w {
            parts.push(Part { kind: "gpu", name: g.name.clone(), watts: w, src: Src::Measured, how: format!("{} driver", g.vendor), load: util });
        } else {
            let gb = g.vram as f32 / 1073741824.0;
            let max = gpu_typical_w(gb) * if laptop { 0.5 } else { 1.0 };
            let idle = max * 0.1;
            let u = util.unwrap_or(0.0);
            let why = if g.vendor == "NVIDIA" { "the driver gives no power reading" } else { "no power reading from this vendor yet" };
            parts.push(Part {
                kind: "gpu",
                name: g.name.clone(),
                watts: idle + (max - idle) * u / 100.0,
                src: Src::Estimated,
                how: format!("Load × typical power for a {:.0} GB card ({why})", gb.round()),
                load: util,
            });
        }
    }

    // battery charging (measured), rest of the system
    if let Some(b) = r.battery.filter(|b| b.present && b.ac && b.rate_mw > 0) {
        parts.push(Part { kind: "charge", name: "Battery charging".into(), watts: b.rate_mw as f32 / 1000.0, src: Src::Measured, how: "Battery charge rate".into(), load: None });
    }
    let rest_auto = if laptop { 6.0 } else if cfg!(target_os = "macos") { 10.0 } else { 35.0 };
    let rest = cfg.rest_w.unwrap_or(rest_auto);
    parts.push(Part {
        kind: "rest",
        name: if laptop { "Screen, board, drives, fans".into() } else { "Motherboard, RAM, drives, fans".into() },
        watts: rest,
        src: if cfg.rest_w.is_some() { Src::Set } else { Src::Estimated },
        how: if cfg.rest_w.is_some() { "Your setting".into() } else { "Typical for this kind of PC; adjust it below or calibrate with a meter".into() },
        load: None,
    });

    #[cfg_attr(not(target_os = "macos"), allow(unused_mut))]
    let mut measured_dc = false;
    s.source_note.clear();
    #[cfg(target_os = "macos")]
    {
        let battery_dc = r.battery.filter(|b| b.present && !b.ac && b.rate_mw < 0).map(|b| -b.rate_mw as f32 / 1000.0);
        let total = if on_battery { battery_dc } else { r.system_dc_w };
        if cfg.rest_w.is_none() {
            if let Some(total) = total.filter(|v| v.is_finite() && *v > 0.0 && *v < 1000.0) {
                let fixed: f32 = parts.iter().filter(|p| p.src != Src::Estimated).map(|p| p.watts).sum();
                let estimates: f32 = parts.iter().filter(|p| p.src == Src::Estimated).map(|p| p.watts).sum();
                if total >= fixed && estimates > 0.0 {
                    for part in parts.iter_mut().filter(|p| p.src == Src::Estimated) {
                        part.watts *= (total - fixed) / estimates;
                        part.how = "Estimated share of measured Mac DC input; no per-part power sensor".into();
                    }
                    measured_dc = true;
                }
            }
        }
        s.source_note = if measured_dc {
            if on_battery { "Battery measured; component split estimated" } else { "DC measured; charger loss estimated" }
        } else { "Component and wall figures are estimates" }.into();
    }
    let dc: f32 = parts.iter().map(|p| p.watts).sum();
    let measured: f32 = parts.iter().filter(|p| p.src == Src::Measured).map(|p| p.watts).sum();
    let eff_auto = if laptop { 0.88 } else { 0.87 };
    let eff = cfg.eff.unwrap_or(eff_auto);
    let loss = if on_battery { 0.0 } else { dc / eff - dc };
    parts.push(Part {
        kind: "psu",
        name: if laptop { "Charger loss".into() } else { "Power supply loss".into() },
        watts: loss,
        src: if cfg.eff.is_some() { Src::Set } else { Src::Estimated },
        how: format!("{:.0}% efficient{}", eff * 100.0, if cfg.eff.is_some() { " (your setting)" } else { " (typical)" }),
        load: None,
    });
    if cfg.monitor_w > 0.0 {
        parts.push(Part { kind: "monitor", name: "Monitor".into(), watts: cfg.monitor_w, src: Src::Set, how: "Your setting".into(), load: None });
    }

    s.laptop = laptop;
    s.on_battery = on_battery;
    s.battery_w = r.battery.filter(|b| b.present && !b.ac && b.rate_mw < 0).map(|b| (-b.rate_mw) as f32 / 1000.0);
    s.dc_w = dc;
    s.dc_measured = measured_dc;
    // on battery nothing is drawn from the wall now; that energy is paid for when it recharges
    s.wall_w = if on_battery { 0.0 } else { dc + loss + cfg.monitor_w };
    s.measured = if measured_dc || (on_battery && s.battery_w.is_some()) { 1.0 } else if dc > 0.0 { (measured / dc).clamp(0.0, 1.0) } else { 0.0 };
    s.parts = parts;
    s.rest_auto_w = rest_auto;
    s.eff_auto = eff_auto;
}

// ---------------- the energy log ----------------
struct Log {
    days: BTreeMap<String, Day>,
    dirty: bool,
}

fn log_file() -> PathBuf {
    let dir = std::env::var("MC_SETTINGS_DIR").map(PathBuf::from).unwrap_or_else(|_| dirs::config_dir().unwrap_or_else(crate::util::home).join("Mission Control"));
    dir.join("power-log.json")
}

fn day_key(d: chrono::NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

impl Log {
    fn load() -> Self {
        let mut days = BTreeMap::new();
        if let Ok(t) = std::fs::read_to_string(log_file()) {
            if let Ok(v) = serde_json::from_str::<Value>(t.trim_start_matches('\u{feff}')) {
                if let Some(m) = v["days"].as_object() {
                    for (k, d) in m {
                        let f = |i: usize| d[i].as_f64().unwrap_or(0.0).max(0.0);
                        if chrono::NaiveDate::parse_from_str(k, "%Y-%m-%d").is_ok() {
                            days.insert(k.clone(), Day { wh: f(0), s: f(1), mwh: f(2) });
                        }
                    }
                }
            }
        }
        Self { days, dirty: false }
    }

    fn save(&mut self) {
        if !self.dirty {
            return;
        }
        // keep a little over two years
        while self.days.len() > 800 {
            let k = self.days.keys().next().cloned().unwrap();
            self.days.remove(&k);
        }
        let days: serde_json::Map<String, Value> = self.days.iter().map(|(k, d)| (k.clone(), json!([(d.wh * 1000.0).round() / 1000.0, d.s.round(), (d.mwh * 1000.0).round() / 1000.0]))).collect();
        let f = log_file();
        if let Some(dir) = f.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = f.with_extension("json.tmp");
        if std::fs::write(&tmp, json!({ "v": 1, "days": days }).to_string()).is_ok() && std::fs::rename(&tmp, &f).is_ok() {
            self.dirty = false;
        }
    }

    fn add(&mut self, wh: f64, secs: f64, measured: f64) {
        let d = self.days.entry(day_key(chrono::Local::now().date_naive())).or_default();
        d.wh += wh;
        d.s += secs;
        d.mwh += wh * measured;
        self.dirty = true;
    }

    fn summary(&self, hours_set: Option<f32>) -> Summary {
        let today = chrono::Local::now().date_naive();
        let sum_since = |n: i64| -> f64 {
            let from = day_key(today - chrono::Duration::days(n - 1));
            self.days.range(from..).map(|(_, d)| d.wh).sum()
        };
        let mut out = Summary {
            today: self.days.get(&day_key(today)).copied().unwrap_or_default(),
            last7: sum_since(7),
            last30: sum_since(30),
            last365: sum_since(365),
            days_tracked: self.days.values().filter(|d| d.s > 0.0).count(),
            ..Default::default()
        };
        // A typical day = average power while on × hours on per day. The hours come from the user, or are
        // learned from past calendar days (days the PC was off count as zero; the first day in the log is
        // usually only partly tracked, so it's left out when there's anything after it), or are assumed to
        // be 8 until a full day has been tracked.
        let from30 = day_key(today - chrono::Duration::days(29));
        let (wh30, s30) = self.days.range(from30..).fold((0.0, 0.0), |a, (_, d)| (a.0 + d.wh, a.1 + d.s));
        out.avg_w = if s30 > 3.0 { wh30 / s30 * 3600.0 } else { 0.0 };
        let yesterday = today - chrono::Duration::days(1);
        let first = self.days.iter().find(|(_, d)| d.s > 0.0).and_then(|(k, _)| chrono::NaiveDate::parse_from_str(k, "%Y-%m-%d").ok());
        let learned = first.filter(|f| *f <= yesterday).map(|first| {
            let start = if first < yesterday { first + chrono::Duration::days(1) } else { first };
            let start = start.max(yesterday - chrono::Duration::days(29));
            let n = (yesterday - start).num_days() + 1;
            let (wh, secs) = self.days.range(day_key(start)..=day_key(yesterday)).fold((0.0, 0.0), |a, (_, d)| (a.0 + d.wh, a.1 + d.s));
            (wh / n as f64, secs / n as f64 / 3600.0, n)
        });
        if let Some(h) = hours_set {
            out.hours = Some(h as f64);
            out.daily_wh = out.avg_w * h as f64;
            out.basis = format!("{h} h a day (your setting) at {:.0} W average", out.avg_w);
        } else if let Some((wh, h, n)) = learned {
            out.hours = Some(h);
            out.daily_wh = wh;
            out.basis = format!("{} · on about {h:.1} h a day", if n == 1 { "based on yesterday".to_string() } else { format!("average of the last {n} days") });
        } else {
            out.daily_wh = out.avg_w * 8.0;
            out.basis = format!("assuming 8 h a day at {:.0} W until a full day is tracked", out.avg_w);
        }
        out
    }
}

// ---------------- hardware ----------------
#[cfg(windows)]
mod hw {
    use std::ffi::c_void;
    use windows::core::{PCSTR, PCWSTR};
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE};
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32, LOAD_WITH_ALTERED_SEARCH_PATH};
    use windows::Win32::System::Performance::*;
    use windows::Win32::System::Power::{CallNtPowerInformation, SystemBatteryState, SYSTEM_BATTERY_STATE};

    #[derive(Clone, Copy, Debug)]
    pub struct Battery {
        pub present: bool,
        pub ac: bool,
        /// mW, positive while charging, negative while discharging
        pub rate_mw: i32,
    }

    #[derive(Clone, Debug)]
    pub struct Gpu {
        pub name: String,
        pub vendor: &'static str,
        pub vram: u64,
        pub integrated: bool,
        pub util: Option<f32>,
        pub power_w: Option<f32>,
    }

    #[derive(Default)]
    pub struct Reading {
        pub cpu_pkg_w: Option<f32>,
        pub dram_w: Option<f32>,
        pub gpus: Vec<Gpu>,
        pub battery: Option<Battery>,
    }

    struct Adapter {
        name: String,
        vendor: &'static str,
        vram: u64,
        integrated: bool,
        luid: String,
        nvml: Option<usize>,
    }

    pub struct Sensors {
        pdh: Option<Pdh>,
        adapters: Vec<Adapter>,
        nvml: Option<Nvml>,
    }

    impl Sensors {
        pub fn new() -> Self {
            // dev: MC_POWER_ESTIMATE=1 ignores the sensors, to see what a PC without them gets
            let no_sensors = std::env::var_os("MC_POWER_ESTIMATE").is_some();
            let nvml = if no_sensors { None } else { Nvml::load() };
            let mut adapters = adapters();
            // pair each NVIDIA adapter with an NVML device by name, in order
            if let Some(n) = &nvml {
                let mut used = vec![false; n.devs.len()];
                for a in adapters.iter_mut().filter(|a| a.vendor == "NVIDIA") {
                    let hit = n.devs.iter().enumerate().position(|(i, d)| !used[i] && d.1.eq_ignore_ascii_case(&a.name)).or_else(|| used.iter().position(|u| !u));
                    if let Some(i) = hit {
                        used[i] = true;
                        a.nvml = Some(i);
                    }
                }
            }
            let mut pdh = Pdh::new();
            if let Some(p) = pdh.as_mut().filter(|_| no_sensors) {
                p.energy = None;
            }
            Self { pdh, adapters, nvml }
        }

        pub fn read(&mut self) -> Reading {
            let mut r = Reading { battery: battery(), ..Default::default() };
            let mut gpu_util: Vec<(String, f32)> = Vec::new();
            if let Some(p) = &self.pdh {
                p.collect();
                if let Some(c) = p.energy {
                    let (mut pkg, mut dram, mut any_pkg) = (0.0f64, 0.0f64, false);
                    for (name, v) in p.array(c) {
                        let n = name.to_ascii_lowercase();
                        if n.ends_with("_pkg") {
                            any_pkg = true;
                            pkg += v;
                        } else if n.ends_with("_dram") {
                            dram += v;
                        }
                    }
                    // milliwatts; ignore zero or absurd readings
                    let w = (pkg / 1000.0) as f32;
                    if any_pkg && w > 0.05 && w < 1000.0 {
                        r.cpu_pkg_w = Some(w);
                    }
                    let d = (dram / 1000.0) as f32;
                    if d > 0.2 && d < 200.0 {
                        r.dram_w = Some(d);
                    }
                }
                if let Some(c) = p.gpu {
                    // instances look like pid_1234_luid_0x00000000_0x0000D1B5_phys_0_eng_3_engtype_3D: add up
                    // each engine across processes, and take the busiest engine per adapter
                    let mut eng: std::collections::HashMap<(String, String), f64> = Default::default();
                    for (name, v) in p.array(c) {
                        let n = name.to_ascii_lowercase();
                        let (Some(l), Some(e)) = (n.find("luid_"), n.find("_phys_")) else { continue };
                        if e <= l {
                            continue;
                        }
                        let luid = n[l + 5..e].to_string();
                        let engine = n[e..].split("_engtype").next().unwrap_or("").to_string();
                        *eng.entry((luid, engine)).or_default() += v;
                    }
                    for ((luid, _), v) in eng {
                        match gpu_util.iter_mut().find(|(l, _)| *l == luid) {
                            Some(x) => x.1 = x.1.max(v as f32),
                            None => gpu_util.push((luid, v as f32)),
                        }
                    }
                }
            }
            for a in &self.adapters {
                let mut util = self.pdh.as_ref().filter(|p| p.gpu.is_some()).map(|_| gpu_util.iter().find(|(l, _)| *l == a.luid).map(|x| x.1.clamp(0.0, 100.0)).unwrap_or(0.0));
                let mut power_w = None;
                if let (Some(n), Some(i)) = (&self.nvml, a.nvml) {
                    let (p, u) = n.read(i);
                    power_w = p.filter(|w| *w > 0.0 && *w < 2000.0);
                    if u.is_some() {
                        util = u;
                    }
                }
                r.gpus.push(Gpu { name: a.name.clone(), vendor: a.vendor, vram: a.vram, integrated: a.integrated, util, power_w });
            }
            r
        }
    }

    // ---- performance counters (PDH) ----
    struct Pdh {
        q: PDH_HQUERY,
        energy: Option<PDH_HCOUNTER>,
        gpu: Option<PDH_HCOUNTER>,
    }

    // the handles are only used from the power thread
    unsafe impl Send for Pdh {}

    impl Pdh {
        fn new() -> Option<Self> {
            unsafe {
                let mut q = PDH_HQUERY::default();
                if PdhOpenQueryW(PCWSTR::null(), 0, &mut q) != 0 {
                    return None;
                }
                let add = |path: &str| {
                    let w: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
                    let mut c = PDH_HCOUNTER::default();
                    (PdhAddEnglishCounterW(q, PCWSTR(w.as_ptr()), 0, &mut c) == 0).then_some(c)
                };
                let me = Self { q, energy: add("\\Energy Meter(*)\\Power"), gpu: add("\\GPU Engine(*)\\Utilization Percentage") };
                let _ = PdhCollectQueryData(q);
                Some(me)
            }
        }

        fn collect(&self) {
            unsafe {
                let _ = PdhCollectQueryData(self.q);
            }
        }

        fn array(&self, c: PDH_HCOUNTER) -> Vec<(String, f64)> {
            unsafe {
                let (mut size, mut count) = (0u32, 0u32);
                let st = PdhGetFormattedCounterArrayW(c, PDH_FMT(PDH_FMT_DOUBLE.0 | 0x8000), &mut size, &mut count, None);
                if st != PDH_MORE_DATA || size == 0 {
                    return Vec::new();
                }
                // the buffer holds the items followed by their names: allocate it 8-byte aligned
                let mut buf = vec![0u64; (size as usize).div_ceil(8)];
                let items = buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
                if PdhGetFormattedCounterArrayW(c, PDH_FMT(PDH_FMT_DOUBLE.0 | 0x8000), &mut size, &mut count, Some(items)) != 0 {
                    return Vec::new();
                }
                (0..count as usize)
                    .filter_map(|i| {
                        let it = &*items.add(i);
                        if it.FmtValue.CStatus != 0 {
                            return None;
                        }
                        let name = it.szName.to_string().ok()?;
                        Some((name, it.FmtValue.Anonymous.doubleValue))
                    })
                    .collect()
            }
        }
    }

    impl Drop for Pdh {
        fn drop(&mut self) {
            unsafe {
                let _ = PdhCloseQuery(self.q);
            }
        }
    }

    // ---- graphics adapters (DXGI) ----
    fn adapters() -> Vec<Adapter> {
        let mut out: Vec<Adapter> = Vec::new();
        unsafe {
            let Ok(f) = CreateDXGIFactory1::<IDXGIFactory1>() else { return out };
            let mut i = 0;
            while let Ok(a) = f.EnumAdapters1(i) {
                i += 1;
                let Ok(d) = a.GetDesc1() else { continue };
                if d.Flags & (DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0 || d.VendorId == 0x1414 {
                    continue; // Microsoft Basic Render Driver and other software adapters
                }
                let len = d.Description.iter().position(|c| *c == 0).unwrap_or(d.Description.len());
                let name = String::from_utf16_lossy(&d.Description[..len]).trim().to_string();
                let luid = format!("0x{:08x}_0x{:08x}", d.AdapterLuid.HighPart as u32, d.AdapterLuid.LowPart);
                if out.iter().any(|x| x.luid == luid) {
                    continue;
                }
                let vendor = match d.VendorId {
                    0x10de => "NVIDIA",
                    0x1002 | 0x1022 => "AMD",
                    0x8086 => "Intel",
                    0x5143 => "Qualcomm",
                    _ => "GPU",
                };
                let vram = d.DedicatedVideoMemory as u64;
                let low = name.to_ascii_lowercase();
                // built-in graphics share the CPU package: Intel non-Arc, AMD "Radeon Graphics" APUs, Qualcomm,
                // or anything with under 1 GB of its own memory
                let integrated = vram < (1u64 << 30)
                    || (vendor == "Intel" && !low.contains("arc"))
                    || (vendor == "AMD" && (low.contains("radeon(tm) graphics") || low == "amd radeon graphics" || low.ends_with("radeon graphics")) && !low.contains(" rx"))
                    || vendor == "Qualcomm";
                out.push(Adapter { name, vendor, vram, integrated, luid, nvml: None });
            }
        }
        out
    }

    // ---- NVIDIA (NVML, shipped with the driver) ----
    type Dev = *mut c_void;
    struct Nvml {
        power: unsafe extern "C" fn(Dev, *mut u32) -> i32,
        util: unsafe extern "C" fn(Dev, *mut [u32; 2]) -> i32,
        devs: Vec<(Dev, String)>,
    }
    unsafe impl Send for Nvml {}

    impl Nvml {
        fn load() -> Option<Self> {
            unsafe {
                let sys = windows::core::w!("nvml.dll");
                let lib = LoadLibraryExW(sys, None, LOAD_LIBRARY_SEARCH_SYSTEM32)
                    .or_else(|_| LoadLibraryExW(windows::core::w!("C:\\Program Files\\NVIDIA Corporation\\NVSMI\\nvml.dll"), None, LOAD_WITH_ALTERED_SEARCH_PATH))
                    .ok()?;
                let f = |n: &[u8]| GetProcAddress(lib, PCSTR(n.as_ptr()));
                let init: unsafe extern "C" fn() -> i32 = std::mem::transmute(f(b"nvmlInit_v2\0")?);
                let count: unsafe extern "C" fn(*mut u32) -> i32 = std::mem::transmute(f(b"nvmlDeviceGetCount_v2\0")?);
                let by_index: unsafe extern "C" fn(u32, *mut Dev) -> i32 = std::mem::transmute(f(b"nvmlDeviceGetHandleByIndex_v2\0")?);
                let name: unsafe extern "C" fn(Dev, *mut u8, u32) -> i32 = std::mem::transmute(f(b"nvmlDeviceGetName\0")?);
                let power: unsafe extern "C" fn(Dev, *mut u32) -> i32 = std::mem::transmute(f(b"nvmlDeviceGetPowerUsage\0")?);
                let util: unsafe extern "C" fn(Dev, *mut [u32; 2]) -> i32 = std::mem::transmute(f(b"nvmlDeviceGetUtilizationRates\0")?);
                if init() != 0 {
                    return None;
                }
                let mut n = 0u32;
                if count(&mut n) != 0 {
                    return None;
                }
                let mut devs = Vec::new();
                for i in 0..n.min(16) {
                    let mut d: Dev = std::ptr::null_mut();
                    if by_index(i, &mut d) != 0 {
                        continue;
                    }
                    let mut buf = [0u8; 96];
                    let nm = if name(d, buf.as_mut_ptr(), buf.len() as u32) == 0 {
                        let l = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
                        String::from_utf8_lossy(&buf[..l]).trim().to_string()
                    } else {
                        String::new()
                    };
                    devs.push((d, nm));
                }
                Some(Self { power, util, devs })
            }
        }

        /// (watts, load %)
        fn read(&self, i: usize) -> (Option<f32>, Option<f32>) {
            let Some((d, _)) = self.devs.get(i) else { return (None, None) };
            unsafe {
                let mut mw = 0u32;
                let p = ((self.power)(*d, &mut mw) == 0).then_some(mw as f32 / 1000.0);
                let mut u = [0u32; 2];
                let l = ((self.util)(*d, &mut u) == 0).then_some(u[0] as f32);
                (p, l)
            }
        }
    }

    // ---- battery ----
    fn battery() -> Option<Battery> {
        unsafe {
            let mut s = SYSTEM_BATTERY_STATE::default();
            let st = CallNtPowerInformation(SystemBatteryState, None, 0, Some(&mut s as *mut _ as *mut c_void), std::mem::size_of::<SYSTEM_BATTERY_STATE>() as u32);
            if st.is_err() {
                return None;
            }
            let rate = s.Rate as i32;
            // 0x80000000 means "unknown"
            let rate = if rate == i32::MIN { 0 } else { rate };
            Some(Battery { present: s.BatteryPresent, ac: s.AcOnLine, rate_mw: rate })
        }
    }

    /// The user's currency symbol from Windows' regional settings.
    pub fn currency() -> String {
        use windows::Win32::Globalization::{GetLocaleInfoEx, LOCALE_SCURRENCY};
        let mut buf = [0u16; 16];
        let n = unsafe { GetLocaleInfoEx(PCWSTR::null(), LOCALE_SCURRENCY, Some(&mut buf)) };
        if n > 1 {
            String::from_utf16_lossy(&buf[..n as usize - 1])
        } else {
            "$".into()
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod hw {
    #[derive(Clone, Copy, Debug)]
    pub struct Battery {
        pub present: bool,
        pub ac: bool,
        pub rate_mw: i32,
    }
    #[derive(Clone, Debug)]
    pub struct Gpu {
        pub name: String,
        pub vendor: &'static str,
        pub vram: u64,
        pub integrated: bool,
        pub util: Option<f32>,
        pub power_w: Option<f32>,
    }
    #[derive(Default)]
    pub struct Reading {
        pub cpu_pkg_w: Option<f32>,
        pub dram_w: Option<f32>,
        pub gpus: Vec<Gpu>,
        pub battery: Option<Battery>,
    }
    /// No readings outside Windows yet: everything is estimated.
    pub struct Sensors;
    impl Sensors {
        pub fn new() -> Self {
            Sensors
        }
        pub fn read(&mut self) -> Reading {
            Reading::default()
        }
    }
    pub fn currency() -> String {
        "$".into()
    }
}

#[cfg(target_os = "macos")]
mod hw {
    use serde_json::{json, Value};
    #[derive(Clone, Copy, Debug)]
    pub struct Battery { pub present: bool, pub ac: bool, pub rate_mw: i32 }
    #[derive(Clone, Debug)]
    pub struct Gpu { pub name: String, pub vendor: &'static str, pub vram: u64, pub integrated: bool, pub util: Option<f32>, pub power_w: Option<f32> }
    #[derive(Default)]
    pub struct Reading { pub cpu_pkg_w: Option<f32>, pub dram_w: Option<f32>, pub gpus: Vec<Gpu>, pub battery: Option<Battery>, pub system_dc_w: Option<f32>, pub currency: Option<String> }
    pub struct Sensors;
    impl Sensors {
        pub fn new() -> Self { Self }
        pub fn read(&mut self) -> Reading {
            match tauri::async_runtime::block_on(crate::native_mac::helper("power", &json!({}), false)) {
                Ok(v) => parse(&v),
                Err(_) => Reading::default(),
            }
        }
    }
    pub fn parse(v: &Value) -> Reading {
        let no_sensor = std::env::var_os("MC_POWER_ESTIMATE").is_some();
        let finite = |v: &Value, max: f64| v.as_f64().filter(|n| n.is_finite() && *n >= 0.0 && *n < max).map(|n| n as f32);
        let battery = v["battery"].as_object().map(|b| Battery {
            present: b.get("present").and_then(Value::as_bool).unwrap_or(false),
            ac: b.get("ac").and_then(Value::as_bool).unwrap_or(true),
            rate_mw: if no_sensor { 0 } else { b.get("rateMw").and_then(Value::as_f64).filter(|n| n.is_finite() && n.abs() < 500000.0).unwrap_or(0.0) as i32 },
        });
        let gpus = v["gpus"].as_array().map(|gpus| gpus.iter().map(|g| Gpu {
            name: g["name"].as_str().unwrap_or("Mac GPU").into(), vendor: "Mac", vram: 0,
            integrated: g["integrated"].as_bool().unwrap_or(true), util: finite(&g["util"], 100.1), power_w: None,
        }).collect()).unwrap_or_default();
        Reading { battery, gpus, system_dc_w: if no_sensor { None } else { finite(&v["systemDcW"], 500.0) }, currency: v["currency"].as_str().filter(|c| !c.is_empty()).map(String::from), ..Default::default() }
    }
    pub fn currency() -> String { "$".into() }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    fn config() -> Cfg { Cfg { rest_w: None, eff: Some(0.8), monitor_w: 0.0, hours: None } }
    fn reading(ac: bool, rate: i32, dc: Option<f32>) -> hw::Reading {
        hw::Reading { battery: Some(hw::Battery { present: true, ac, rate_mw: rate }), system_dc_w: dc, ..Default::default() }
    }
    #[test]
    fn mac_dc_total_includes_charging_without_double_counting() {
        let mut s = Snapshot::default();
        compute(&mut s, &reading(true, 5000, Some(20.0)), &config(), 40.0, 10, "Apple M4");
        assert!((s.dc_w - 20.0).abs() < 0.001);
        assert!((s.wall_w - 25.0).abs() < 0.001);
        assert!(s.dc_measured);
        assert_eq!(s.parts.iter().find(|p| p.kind == "charge").unwrap().watts, 5.0);
        assert_eq!(s.parts.iter().find(|p| p.kind == "cpu").unwrap().src, Src::Estimated);
    }
    #[test]
    fn battery_discharge_is_measured_but_does_not_bill_wall_energy() {
        let mut s = Snapshot::default();
        compute(&mut s, &reading(false, -9000, None), &config(), 80.0, 10, "Apple M4");
        assert!(s.on_battery && s.dc_measured);
        assert!((s.dc_w - 9.0).abs() < 0.001);
        assert_eq!(s.battery_w, Some(9.0));
        assert_eq!(s.wall_w, 0.0);
        assert_eq!(s.parts.iter().find(|p| p.kind == "psu").unwrap().watts, 0.0);
    }
    #[test]
    fn inconsistent_charge_and_adapter_samples_fall_back_to_estimates() {
        let mut s = Snapshot::default();
        compute(&mut s, &reading(true, 25000, Some(20.0)), &config(), 20.0, 10, "Apple M4");
        assert!(!s.dc_measured);
        assert!(s.parts.iter().all(|p| p.watts.is_finite() && p.watts >= 0.0));
    }
    #[test]
    fn manual_component_override_is_preserved() {
        let mut s = Snapshot::default();
        let mut c = config(); c.rest_w = Some(15.0);
        compute(&mut s, &reading(true, 5000, Some(20.0)), &c, 20.0, 10, "Apple M4");
        assert!(!s.dc_measured);
        let rest = s.parts.iter().find(|p| p.kind == "rest").unwrap();
        assert_eq!(rest.watts, 15.0); assert_eq!(rest.src, Src::Set);
    }
    #[test]
    fn malformed_power_values_are_rejected() {
        let r = hw::parse(&json!({"systemDcW": -5, "battery":{"present":true,"ac":true,"rateMw":1e20}, "gpus":[{"name":"GPU","util":101}]}));
        assert!(r.system_dc_w.is_none()); assert_eq!(r.battery.unwrap().rate_mw, 0);
        assert!(r.gpus[0].util.is_none());
    }
    #[test]
    fn energy_log_projects_watt_hours_using_explicit_daily_hours() {
        let mut log = Log { days: BTreeMap::new(), dirty: false };
        log.add(20.0, 3600.0, 0.5);
        let s = log.summary(Some(8.0));
        assert_eq!(s.today.wh, 20.0); assert_eq!(s.today.mwh, 10.0);
        assert_eq!(s.daily_wh, 160.0);
        assert_eq!(log.summary(Some(0.0)).daily_wh, 0.0);
    }
}
