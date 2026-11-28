// Health watch. It runs a check at startup, and whenever the PC is under real strain (CPU pinned, RAM
// nearly full, the GPU running hot, a disk almost full, heavy load that doesn't let go, or right after a
// long fullscreen session). With Claude Code or Codex connected, a small model (Haiku / Codex's smallest
// model at low effort, no tools) reads a snapshot of the stats and says whether anything is wrong and what
// to do. The finding pops up under the notch; "Fix it" opens the chat and hands the job to the top model
// (Opus / Codex at high effort), which works under the user's normal chat permission mode. Every check and
// an hourly summary of the stats are kept in health-log.json next to the settings.
use crate::app::{fmt_bytes, App, Mode};
use crate::draw::*;
use crate::pages::{card, card_h, ghost, PAGE_W};
use crate::theme::*;
use eframe::egui::{self, pos2, vec2, Align2, Color32, Rect, Sense, Stroke, Ui};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Event {
    pub id: u64,
    /// unix ms
    pub at: f64,
    pub kind: String,
    /// checking | ok | warn | critical
    pub status: String,
    pub title: String,
    pub summary: String,
    pub action: String,
    /// an instruction for the fixing agent; empty when there's nothing to fix
    pub fix: String,
    /// who judged it: "Haiku", "Codex", or "Mission Control" (no AI)
    pub by: String,
    pub snap: Value,
}

impl Event {
    fn to_json(&self) -> Value {
        json!({ "id": self.id, "at": self.at, "kind": self.kind, "status": self.status, "title": self.title, "summary": self.summary, "action": self.action, "fix": self.fix, "by": self.by, "snap": self.snap })
    }
    fn from_json(v: &Value) -> Option<Self> {
        let s = |k: &str| v[k].as_str().unwrap_or("").to_string();
        Some(Self { id: v["id"].as_u64()?, at: v["at"].as_f64()?, kind: s("kind"), status: s("status"), title: s("title"), summary: s("summary"), action: s("action"), fix: s("fix"), by: s("by"), snap: v["snap"].clone() })
    }
    fn bad(&self) -> bool {
        self.status == "warn" || self.status == "critical"
    }
}

/// One hour of stats, added up as samples arrive.
#[derive(Default, Clone)]
struct Hour {
    key: String,
    n: f64,
    cpu: (f64, f64),
    ram: (f64, f64),
    gpu: (f64, f64),
    gpu_t: f64,
    net: f64,
}

impl Hour {
    fn to_json(&self) -> Value {
        let n = self.n.max(1.0);
        let r = |v: f64| (v * 10.0).round() / 10.0;
        json!({ "n": self.n, "cpuAvg": r(self.cpu.0 / n), "cpuMax": r(self.cpu.1), "ramAvg": r(self.ram.0 / n), "ramMax": r(self.ram.1), "gpuAvg": r(self.gpu.0 / n), "gpuMax": r(self.gpu.1), "gpuTempMax": r(self.gpu_t), "netMax": self.net.round() })
    }
}

#[derive(Default)]
pub struct Health {
    pub events: Vec<Event>,
    /// the event in the popup, and until when it shows (app clock; 0 = until dismissed)
    pub alert: Option<u64>,
    alert_until: f64,
    alert_shown: bool,
    since: HashMap<&'static str, f64>,
    cooldown: HashMap<String, f64>,
    first_stats: f64,
    startup_done: bool,
    pending: Option<(f64, String, String)>,
    pub checking: bool,
    next_id: u64,
    hour: Hour,
    hours: std::collections::BTreeMap<String, Value>,
    loaded: bool,
}

fn log_file() -> PathBuf {
    let dir = std::env::var("MC_SETTINGS_DIR").map(PathBuf::from).unwrap_or_else(|_| dirs::config_dir().unwrap_or_else(crate::util::home).join("Mission Control"));
    dir.join("health-log.json")
}

const SYSTEM: &str = "You are the health checker inside a PC monitoring widget. You get a JSON snapshot of the computer's live stats and the reason the check ran. Decide whether anything needs the user's attention: overheating, memory pressure, a runaway or stuck process, a nearly full disk, sustained full load that isn't from something the user is obviously running, or anything else abnormal. Be factual: use only the numbers given and never invent readings. Heavy load from a game, render, compile or download the user is running is normal unless temperatures or memory are at risk. Reply with only one JSON object and nothing else: {\"status\":\"ok|warn|critical\",\"title\":\"at most 60 characters\",\"summary\":\"at most 160 characters, plain words, cite the key numbers\",\"action\":\"one thing the user could do, at most 100 characters, or empty\",\"fix\":\"if an AI agent with PowerShell on this PC could safely fix or relieve it, a precise instruction for that agent; otherwise empty\"}";

impl Health {
    fn ensure_loaded(&mut self) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        if let Ok(t) = std::fs::read_to_string(log_file()) {
            if let Ok(v) = serde_json::from_str::<Value>(t.trim_start_matches('\u{feff}')) {
                self.events = v["events"].as_array().into_iter().flatten().filter_map(Event::from_json).collect();
                // a check that was still running when the app closed never finished
                for e in self.events.iter_mut().filter(|e| e.status == "checking") {
                    e.status = "ok".into();
                    e.by = "Mission Control".into();
                }
                if let Some(h) = v["hours"].as_object() {
                    self.hours = h.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                }
            }
        }
        self.next_id = self.events.iter().map(|e| e.id).max().unwrap_or(0) + 1;
    }

    fn save(&mut self) {
        while self.events.len() > 200 {
            self.events.remove(0);
        }
        while self.hours.len() > 24 * 30 {
            let k = self.hours.keys().next().cloned().unwrap();
            self.hours.remove(&k);
        }
        let mut hours: Map<String, Value> = self.hours.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        if self.hour.n > 0.0 {
            hours.insert(self.hour.key.clone(), self.hour.to_json());
        }
        let v = json!({ "v": 1, "events": self.events.iter().map(Event::to_json).collect::<Vec<_>>(), "hours": hours });
        let f = log_file();
        if let Some(d) = f.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let tmp = f.with_extension("json.tmp");
        if std::fs::write(&tmp, v.to_string()).is_ok() {
            let _ = std::fs::rename(&tmp, &f);
        }
    }

    pub fn get(&self, id: u64) -> Option<&Event> {
        self.events.iter().find(|e| e.id == id)
    }

    /// Highest CPU, RAM and GPU temperature over the last 24 hours of the log.
    pub fn peaks_24h(&self) -> (f64, f64, f64) {
        let from = (chrono::Local::now() - chrono::Duration::hours(24)).format("%Y-%m-%d %H").to_string();
        let mut p = (self.hour.cpu.1, self.hour.ram.1, self.hour.gpu_t);
        for (_, h) in self.hours.range(from..) {
            p.0 = p.0.max(h["cpuMax"].as_f64().unwrap_or(0.0));
            p.1 = p.1.max(h["ramMax"].as_f64().unwrap_or(0.0));
            p.2 = p.2.max(h["gpuTempMax"].as_f64().unwrap_or(0.0));
        }
        p
    }
}

fn ram_pct(s: &Value) -> f64 {
    s["mem"]["used"].as_f64().unwrap_or(0.0) / s["mem"]["total"].as_f64().unwrap_or(1.0).max(1.0) * 100.0
}

/// Called for every stats sample (once a second).
pub fn on_stats(app: &mut App) {
    app.health.ensure_loaded();
    let s = app.stats.clone();
    let now = app.now;
    let cpu = s["cpu"].as_f64().unwrap_or(0.0);
    let ram = ram_pct(&s);
    let gpu = s["gpu"]["util"].as_f64().unwrap_or(0.0);
    let gpu_t = s["gpu"]["temp"].as_f64().unwrap_or(0.0);
    let net = s["net"]["down"].as_f64().unwrap_or(0.0).max(s["net"]["up"].as_f64().unwrap_or(0.0));
    let enabled = app.set_bool("healthChecks");

    // hourly stats log
    let key = chrono::Local::now().format("%Y-%m-%d %H").to_string();
    let h = &mut app.health;
    if h.hour.key != key {
        if h.hour.n > 0.0 {
            let done = h.hour.to_json();
            h.hours.insert(h.hour.key.clone(), done);
        }
        let had = h.hour.n > 0.0;
        h.hour = Hour { key, ..Default::default() };
        if had {
            h.save();
        }
    }
    let hr = &mut h.hour;
    hr.n += 1.0;
    hr.cpu = (hr.cpu.0 + cpu, hr.cpu.1.max(cpu));
    hr.ram = (hr.ram.0 + ram, hr.ram.1.max(ram));
    hr.gpu = (hr.gpu.0 + gpu, hr.gpu.1.max(gpu));
    hr.gpu_t = hr.gpu_t.max(gpu_t);
    hr.net = hr.net.max(net);

    if !enabled {
        return;
    }
    if h.first_stats == 0.0 {
        h.first_stats = now;
        // dev: MC_HEALTH_DEMO=1 shows a sample finding, to see the popup and the card
        if std::env::var_os("MC_HEALTH_DEMO").is_some() {
            let id = h.next_id;
            h.next_id += 1;
            h.events.push(Event {
                id,
                at: crate::util::now_ms(),
                kind: "ram".into(),
                status: "warn".into(),
                title: "Chrome is holding 9.2 GB of RAM".into(),
                summary: "RAM has been 93% full for 2 min; 41 Chrome processes use 9.2 GB of 15.8 GB.".into(),
                action: "Close tabs you don't need, or restart Chrome".into(),
                fix: "Find the Chrome tabs using the most memory and suggest which to close.".into(),
                by: "Haiku".into(),
                snap: Value::Null,
            });
            show(app, id);
            return;
        }
    }
    // sustained conditions: (key, holds now, seconds it must hold)
    let disk_low = s["disks"].as_array().into_iter().flatten().find(|d| d["free"].as_f64().unwrap_or(1.0) / d["total"].as_f64().unwrap_or(1.0).max(1.0) < 0.06).cloned();
    let conds: [(&'static str, bool, f64); 5] = [
        ("cpu", cpu >= 92.0, 90.0),
        ("ram", ram >= 92.0, 30.0),
        ("gpu_temp", gpu_t >= 84.0, 30.0),
        ("disk", disk_low.is_some(), 0.0),
        ("heavy", cpu >= 85.0 || gpu >= 90.0 || ram >= 88.0, 60.0),
    ];
    let mut fire: Option<(&'static str, f64)> = None;
    for (k, on, need) in conds {
        if !on {
            h.since.remove(k);
            continue;
        }
        let t0 = *h.since.entry(k).or_insert(now);
        if now - t0 >= need && fire.is_none() {
            fire = Some((k, now - t0));
        }
    }
    let total = s["mem"]["total"].as_f64().unwrap_or(0.0);
    let trigger: Option<(String, String, String)> = if let Some((k, held)) = fire {
        let cool = if k == "disk" { 6.0 * 3600.0 } else { 20.0 * 60.0 };
        if h.cooldown.get(k).is_some_and(|t| now - t < cool) {
            None
        } else {
            let mins = (held / 60.0).max(1.0).round();
            let (title, summary) = match k {
                "cpu" => (format!("CPU pinned at {cpu:.0}%"), format!("The processor has been above 92% for about {mins:.0} min.")),
                "ram" => (format!("RAM {ram:.0}% full"), format!("{} of {} in use for over 30 s; apps may slow down or crash.", fmt_bytes(s["mem"]["used"].as_f64().unwrap_or(0.0), 1), fmt_bytes(total, 1))),
                "gpu_temp" => (format!("GPU at {gpu_t:.0}°C"), format!("{} has been at or above 84°C for over 30 s.", s["gpu"]["name"].as_str().unwrap_or("The GPU"))),
                "disk" => {
                    let d = disk_low.clone().unwrap_or(Value::Null);
                    (format!("{} almost full", d["drive"].as_str().unwrap_or("A drive")), format!("Only {} left of {}.", fmt_bytes(d["free"].as_f64().unwrap_or(0.0), 1), fmt_bytes(d["total"].as_f64().unwrap_or(0.0), 0)))
                }
                _ => ("Heavy load".to_string(), format!("CPU {cpu:.0}%, GPU {gpu:.0}%, RAM {ram:.0}% for over a minute.")),
            };
            // one strain episode gives one check: the specific finding also counts for "heavy"
            h.cooldown.insert(k.to_string(), now);
            h.cooldown.insert("heavy".into(), now);
            Some((k.to_string(), title, summary))
        }
    } else if !h.startup_done && now - h.first_stats > 25.0 {
        h.startup_done = true;
        Some(("startup".into(), "Startup check".into(), format!("CPU {cpu:.0}%, RAM {ram:.0}%, GPU {gpu:.0}%{}.", if gpu_t > 0.0 { format!(" at {gpu_t:.0}°C") } else { String::new() })))
    } else if h.pending.as_ref().is_some_and(|p| now >= p.0) {
        h.pending.take().map(|(_, title, summary)| ("after_game".into(), title, summary))
    } else {
        None
    };
    if let Some((kind, title, summary)) = trigger {
        start(app, &kind, title, summary);
    }
}

/// A long fullscreen session (usually a game) just ended: check once the stats are flowing again.
pub fn after_fullscreen(app: &mut App, secs: f64) {
    if secs < 300.0 || !app.set_bool("healthChecks") {
        return;
    }
    app.health.pending = Some((app.now + 20.0, "After your fullscreen session".into(), format!("A fullscreen app ran for {:.0} min; checking that everything settled.", secs / 60.0)));
}

/// Run a check now (the Check now button).
pub fn check_now(app: &mut App) {
    app.health.ensure_loaded();
    let s = app.stats.clone();
    let summary = format!("CPU {:.0}%, RAM {:.0}%, GPU {:.0}%.", s["cpu"].as_f64().unwrap_or(0.0), ram_pct(&s), s["gpu"]["util"].as_f64().unwrap_or(0.0));
    start(app, "manual", "Health check".into(), summary);
}

fn local_status(kind: &str) -> &'static str {
    match kind {
        "cpu" | "ram" | "gpu_temp" | "disk" => "warn",
        _ => "ok",
    }
}

fn local_action(kind: &str) -> &'static str {
    match kind {
        "cpu" => "Open the Apps tab to see what is using it",
        "ram" => "Close apps you are not using",
        "gpu_temp" => "Check airflow and the GPU fan",
        "disk" => "Run the Cleaner",
        _ => "",
    }
}

fn start(app: &mut App, kind: &str, title: String, summary: String) {
    let id = app.health.next_id;
    app.health.next_id += 1;
    let snap = snapshot(app, kind, &summary);
    let ai = app.provider.filter(|_| !app.health.checking);
    let by = match ai {
        Some("codex") => "Codex",
        Some(_) => "Haiku",
        None => "Mission Control",
    };
    let ev = Event {
        id,
        at: crate::util::now_ms(),
        kind: kind.into(),
        status: if ai.is_some() { "checking".into() } else { local_status(kind).into() },
        title,
        summary,
        action: local_action(kind).into(),
        fix: String::new(),
        by: by.into(),
        snap: snap.clone(),
    };
    let bad = ev.bad() || (ai.is_some() && local_status(kind) == "warn");
    app.health.events.push(ev);
    // a real problem shows at once (the AI's verdict replaces the text when it arrives); routine checks
    // only pop up with their result
    if bad || (ai.is_none() && kind == "manual") {
        show(app, id);
    }
    app.health.save();
    let Some(prov) = ai else { return };
    app.health.checking = true;
    let be = app.be.clone();
    let prov = prov.to_string();
    crate::backend::spawn(async move {
        let mut snap = snap;
        let procs = tokio::task::spawn_blocking(top_processes).await.unwrap_or_default();
        snap["topCpu"] = procs.0;
        snap["topMemory"] = procs.1;
        let prompt = format!("Snapshot:\n{}", serde_json::to_string_pretty(&snap).unwrap_or_default());
        let reply = if prov == "codex" { be.codex.quick(&format!("{SYSTEM}\n\n{prompt}"), 90).await } else { be.claude.quick(SYSTEM, &prompt, 60).await };
        crate::backend::emit("health", json!({ "id": id, "reply": reply, "snap": snap }));
    });
}

/// The AI's verdict arrived.
pub fn on_result(app: &mut App, v: Value) {
    app.health.checking = false;
    let id = v["id"].as_u64().unwrap_or(0);
    let parsed = v["reply"].as_str().and_then(|r| {
        let (a, b) = (r.find('{')?, r.rfind('}')?);
        serde_json::from_str::<Value>(&r[a..=b]).ok()
    });
    let Some(e) = app.health.events.iter_mut().find(|e| e.id == id) else { return };
    if !v["snap"].is_null() {
        e.snap = v["snap"].clone();
    }
    match parsed {
        Some(p) => {
            let st = p["status"].as_str().unwrap_or("ok");
            e.status = if matches!(st, "ok" | "warn" | "critical") { st.into() } else { "ok".into() };
            let cut = |s: &str, n: usize| s.chars().take(n).collect::<String>();
            if let Some(t) = p["title"].as_str().filter(|t| !t.trim().is_empty()) {
                e.title = cut(t.trim(), 70);
            }
            if let Some(t) = p["summary"].as_str().filter(|t| !t.trim().is_empty()) {
                e.summary = cut(t.trim(), 200);
            }
            e.action = cut(p["action"].as_str().unwrap_or("").trim(), 120);
            e.fix = cut(p["fix"].as_str().unwrap_or("").trim(), 600);
        }
        None => {
            // no usable answer: keep Mission Control's own reading
            e.status = local_status(&e.kind).into();
            e.by = "Mission Control".into();
            if e.summary.len() < 150 {
                e.summary.push_str(" (AI check unavailable.)");
            }
        }
    }
    let (bad, routine) = (e.bad(), matches!(e.kind.as_str(), "startup" | "manual" | "after_game"));
    if bad || routine {
        show(app, id);
    } else if app.health.alert == Some(id) {
        // it turned out fine: let the popup go after a moment
        app.health.alert_until = app.now + 6.0;
    }
    app.health.save();
}

fn show(app: &mut App, id: u64) {
    let bad = app.health.get(id).is_some_and(|e| e.bad() || e.status == "checking");
    app.health.alert = Some(id);
    app.health.alert_shown = false;
    app.health.alert_until = if bad { 0.0 } else { 9.0 };
}

/// Whether the popup is up (the speech bubble waits while it is).
pub fn alert_visible(app: &App) -> bool {
    app.health.alert.is_some() && app.mode == Mode::Collapsed
}

fn snapshot(app: &App, kind: &str, note: &str) -> Value {
    let s = &app.stats;
    let gb = |b: f64| (b / 1073741824.0 * 10.0).round() / 10.0;
    let pw = app.power.snap.lock().unwrap().clone();
    let disks: Vec<Value> = s["disks"].as_array().into_iter().flatten().map(|d| json!({ "drive": d["drive"], "freeGB": gb(d["free"].as_f64().unwrap_or(0.0)), "totalGB": gb(d["total"].as_f64().unwrap_or(0.0)) })).collect();
    let cpu_w = pw.parts.iter().find(|p| p.kind == "cpu").map(|p| (p.watts * 10.0).round() / 10.0);
    json!({
        "reason": kind,
        "note": note,
        "localTime": chrono::Local::now().format("%a %H:%M").to_string(),
        "cpuModel": app.static_["cpuModel"],
        "cpuPercent": (s["cpu"].as_f64().unwrap_or(0.0) * 10.0).round() / 10.0,
        "cpuCores": app.static_["cores"],
        "ram": { "usedGB": gb(s["mem"]["used"].as_f64().unwrap_or(0.0)), "totalGB": gb(s["mem"]["total"].as_f64().unwrap_or(0.0)), "percent": ram_pct(s).round() },
        "gpu": s["gpu"],
        "disks": disks,
        "network": { "downBytesPerSec": s["net"]["down"], "upBytesPerSec": s["net"]["up"] },
        "uptimeHours": (s["uptime"].as_f64().unwrap_or(0.0) / 360.0).round() / 10.0,
        "power": { "wallWatts": pw.wall_w.round(), "cpuPackageWatts": cpu_w, "onBattery": pw.on_battery },
        "os": app.static_["os"],
    })
}

/// The busiest processes by CPU (percent of the whole CPU) and by memory, grouped by name.
fn top_processes() -> (Value, Value) {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
    let kind = ProcessRefreshKind::nothing().with_cpu().with_memory();
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, kind);
    std::thread::sleep(std::time::Duration::from_millis(1000));
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, kind);
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) as f64;
    let mut by: HashMap<String, (f64, f64, u32)> = HashMap::new();
    for p in sys.processes().values() {
        let n = p.name().to_string_lossy().to_string();
        if n.is_empty() || n.eq_ignore_ascii_case("System Idle Process") || n.eq_ignore_ascii_case("Idle") {
            continue;
        }
        let e = by.entry(n).or_default();
        e.0 += p.cpu_usage() as f64 / cores;
        e.1 += p.memory() as f64;
        e.2 += 1;
    }
    let mut v: Vec<(String, (f64, f64, u32))> = by.into_iter().collect();
    v.sort_by(|a, b| b.1 .0.total_cmp(&a.1 .0));
    let cpu: Vec<Value> = v.iter().take(6).filter(|x| x.1 .0 >= 0.5).map(|(n, (c, _, k))| json!({ "name": n, "cpuPercent": (c * 10.0).round() / 10.0, "processes": k })).collect();
    v.sort_by(|a, b| b.1 .1.total_cmp(&a.1 .1));
    let mem: Vec<Value> = v.iter().take(6).map(|(n, (_, m, k))| json!({ "name": n, "memoryGB": (m / 1073741824.0 * 100.0).round() / 100.0, "processes": k })).collect();
    (Value::Array(cpu), Value::Array(mem))
}

/// "Fix it": a fresh chat with the top model (Opus / Codex at high effort), in the user's permission mode.
pub fn fix(app: &mut App, id: u64) {
    let Some(e) = app.health.get(id).cloned() else { return };
    let Some(prov) = app.provider else {
        app.toast("Connect Claude Code or Codex to fix issues");
        return;
    };
    if app.ai_busy {
        app.toast("The assistant is busy; try again when it finishes");
        return;
    }
    let snap = serde_json::to_string(&e.snap).unwrap_or_default();
    let prompt = format!(
        "Mission Control's health check found a problem on this PC and I pressed \"Fix it\".\n\nFinding: {}. {}\nSuggested: {}\nTask: {}\n\nSnapshot when it was found: {}\n\nCheck the current state first (it may have changed), then fix or relieve the problem. Keep it safe: say exactly what you will do and ask me before closing apps that may have unsaved work, deleting anything, or changing system settings. Finish with one line on what changed.",
        e.title,
        e.summary,
        if e.action.is_empty() { "-" } else { &e.action },
        if e.fix.is_empty() { "Find the cause and resolve it." } else { &e.fix },
        snap
    );
    app.new_chat(String::new());
    if prov == "codex" {
        *app.be.codex.model_override.lock().unwrap() = Some((None, "high".into()));
    } else {
        *app.be.claude.model_override.lock().unwrap() = Some(("opus".into(), "high".into()));
    }
    app.health.alert = None;
    app.set_mode(Mode::Chat);
    app.send_chat(&prompt);
}

fn fix_label(app: &App) -> &'static str {
    if app.provider == Some("codex") {
        "Fix with Codex"
    } else {
        "Fix with Opus"
    }
}

fn status_color(s: &str) -> Color32 {
    match s {
        "critical" => RED,
        "warn" => AMBER,
        "checking" => CYAN,
        _ => GREEN,
    }
}

fn ago(at: f64) -> String {
    let t = chrono::DateTime::from_timestamp_millis(at as i64).map(|d| d.with_timezone(&chrono::Local));
    match t {
        Some(t) if t.date_naive() == chrono::Local::now().date_naive() => t.format("%H:%M").to_string(),
        Some(t) => t.format("%a %H:%M").to_string(),
        None => String::new(),
    }
}

// ---------------- popup under the notch ----------------
pub fn draw_alert(app: &mut App, ui: &mut Ui, hits: &mut Vec<Rect>) {
    let id = app.health.alert;
    let ev = id.and_then(|i| app.health.get(i).cloned());
    let collapsed = app.mode == Mode::Collapsed;
    if ev.is_some() && collapsed && !app.health.alert_shown {
        app.health.alert_shown = true;
        if app.health.alert_until > 0.0 {
            app.health.alert_until += app.now; // relative until first shown
        }
    }
    let expired = app.health.alert_shown && app.health.alert_until > 0.0 && app.now >= app.health.alert_until;
    if expired {
        app.health.alert = None;
    }
    let show = ev.is_some() && collapsed && !expired;
    let o = app.a.v("hl_op", if show { 1.0 } else { 0.0 }, 0.25, EASE);
    let k = app.a.v("hl_k", if show { 1.0 } else { 0.0 }, 0.45, SPRING);
    if o < 0.01 {
        return;
    }
    let Some(e) = ev.or_else(|| app.health.events.last().cloned()) else { return };
    let mut p = ui.painter().clone();
    p.set_opacity(o);
    let w = 430.0;
    let pad = 12.0;
    let inner_w = w - pad * 2.0;
    let sc = status_color(&e.status);
    let mut sj = job(&e.summary, font(11.0), DIM, 0.0);
    sj.wrap.max_width = inner_w;
    sj.wrap.max_rows = 2;
    let sg = p.layout_job(sj);
    let has_action = !e.action.is_empty();
    let can_fix = app.provider.is_some() && e.bad() && !e.fix.is_empty();
    let h = pad + 16.0 + 4.0 + sg.size().y + if has_action { 18.0 } else { 0.0 } + 8.0 + 26.0 + pad;
    let top = 54.0 - 10.0 * (1.0 - k);
    let r = Rect::from_min_size(pos2(crate::app::CX - w / 2.0, top), vec2(w, h));
    if show {
        hits.push(r);
    }
    glow(&p, r, 14.0, 20.0, fade(sc, 0.18));
    boxed(&p, r, 14.0, rgba(10, 16, 30, 0.97), fade(sc, 0.45));
    // status dot (pulses while checking), title, who checked it, close
    let d = pos2(r.left() + pad + 4.0, r.top() + pad + 8.0);
    p.circle_filled(d, 4.0, sc);
    if e.status == "checking" || e.bad() {
        let ph = (app.now as f32 / 1.4).fract();
        p.circle_stroke(d, 4.0 + ph * 6.0, Stroke::new(1.2, fade(sc, 0.7 * (1.0 - ph))));
        app.pet.continuous = true;
    }
    let by = if e.status == "checking" { format!("asking {}...", e.by) } else { e.by.clone() };
    let close = Rect::from_center_size(pos2(r.right() - pad - 6.0, d.y), vec2(18.0, 18.0));
    let byr = text(&p, pos2(close.left() - 6.0, d.y), Align2::RIGHT_CENTER, &by.to_uppercase(), font(8.5), DIM2, 1.2);
    text_fit(&p, pos2(d.x + 10.0, d.y), Align2::LEFT_CENTER, &e.title, font6(12.5), TEXT, 0.0, byr.left() - d.x - 18.0);
    let interactive = app.interactive && show;
    let cresp = interactive.then(|| ui.interact(close, ui.scope_id().with(("hl_close", e.id)), Sense::click()));
    let ch = cresp.as_ref().is_some_and(|c| c.hovered());
    let cc = if ch { TEXT } else { DIM };
    let x = 3.5;
    p.line_segment([close.center() + vec2(-x, -x), close.center() + vec2(x, x)], Stroke::new(1.3, cc));
    p.line_segment([close.center() + vec2(-x, x), close.center() + vec2(x, -x)], Stroke::new(1.3, cc));
    if cresp.is_some_and(|c| c.clicked()) {
        app.health.alert = None;
    }
    let mut y = r.top() + pad + 16.0 + 4.0;
    let sgh = sg.size().y;
    p.galley(pos2(r.left() + pad, y), sg, DIM);
    y += sgh;
    if has_action {
        text_fit(&p, pos2(r.left() + pad, y + 3.0), Align2::LEFT_TOP, &format!("→ {}", e.action), font(11.0), CYAN, 0.0, inner_w);
        y += 18.0;
    }
    y += 8.0;
    // buttons
    let mut bx = r.left() + pad;
    if can_fix {
        let label = fix_label(app);
        let bw = text_w(&p, &label.to_uppercase(), font7(10.5), 1.5) + 26.0;
        let b = Rect::from_min_size(pos2(bx, y), vec2(bw, 26.0));
        if small_button(app, ui, &p, b, ("hl_fix", e.id), label, true, interactive) {
            fix(app, e.id);
        }
        bx = b.right() + 8.0;
    }
    let b = Rect::from_min_size(pos2(bx, y), vec2(74.0, 26.0));
    if small_button(app, ui, &p, b, ("hl_det", e.id), "Details", false, interactive) {
        app.health.alert = None;
        app.set_tab(crate::app::Tab::Overview);
        app.set_mode(Mode::Panel);
    }
}

#[allow(clippy::too_many_arguments)]
fn small_button(app: &mut App, ui: &Ui, p: &egui::Painter, r: Rect, id: (&str, u64), label: &str, primary: bool, interactive: bool) -> bool {
    let resp = interactive.then(|| ui.interact(r, ui.scope_id().with(id), Sense::click()));
    let hov = resp.as_ref().is_some_and(|r| r.hovered());
    let hk = app.a.v(("hl_bh", id.0, id.1), if hov { 1.0 } else { 0.0 }, 0.2, EASE);
    if primary {
        glow(p, r, 8.0, 12.0, rgba(62, 243, 255, 0.2 + 0.15 * hk));
        hgrad(p, r, 8.0, &[(0.0, lerp_c(CYAN, Color32::WHITE, 0.12 * hk)), (1.0, lerp_c(VIOLET, Color32::WHITE, 0.12 * hk))]);
        text(p, r.center(), Align2::CENTER_CENTER, &label.to_uppercase(), font7(10.5), INK, 1.5);
    } else {
        boxed(p, r, 8.0, fade(rgba(62, 243, 255, 0.08), hk), lerp_c(LINE2, rgba(62, 243, 255, 0.45), hk));
        text(p, r.center(), Align2::CENTER_CENTER, &label.to_uppercase(), font(10.5), TEXT, 1.3);
    }
    resp.is_some_and(|r| r.clicked())
}

// ---------------- the Overview card ----------------
pub fn card_ui(app: &mut App, ui: &mut Ui) {
    app.health.ensure_loaded();
    let list: Vec<Event> = app.health.events.iter().rev().take(5).cloned().collect();
    let row_h = 40.0;
    let rows = list.len().max(1) as f32;
    let (r, _) = ui.allocate_exact_size(vec2(PAGE_W, 12.0 + 14.0 + 8.0 + rows * row_h + 22.0 + 10.0), Sense::hover());
    let p = ui.painter().clone();
    card(&p, r);
    let hh = card_h(&p, r, "Health checks");
    // Check now (right of the heading)
    let enabled = !app.health.checking && !app.stats.is_null();
    let label = if app.health.checking { "Checking..." } else { "Check now" };
    let bw = text_w(&p, &label.to_uppercase(), font(10.0), 1.2) + 20.0;
    let b = Rect::from_min_size(pos2(hh.right() - bw, hh.center().y - 11.0), vec2(bw, 22.0));
    if ghost(app, ui, &p, b, "health-now", label, enabled, false) {
        check_now(app);
    }
    let who = match app.provider {
        Some("codex") => "Codex's small model",
        Some(_) => "Haiku",
        None => "",
    };
    if !who.is_empty() {
        text(&p, pos2(b.left() - 10.0, hh.center().y), Align2::RIGHT_CENTER, &format!("CHECKED BY {}", who.to_uppercase()), font(9.0), DIM2, 1.3);
    }
    let mut y = hh.bottom() + 8.0;
    if list.is_empty() {
        let msg = if app.set_bool("healthChecks") { "No checks yet. The first one runs shortly after startup." } else { "Health checks are off (Settings)." };
        text(&p, pos2(r.left() + 14.0, y + 14.0), Align2::LEFT_CENTER, msg, font(11.5), DIM, 0.0);
        y += row_h;
    }
    for (i, e) in list.iter().enumerate() {
        let rr = Rect::from_min_size(pos2(r.left() + 14.0, y), vec2(r.width() - 28.0, row_h));
        y += row_h;
        if i > 0 {
            p.line_segment([pos2(rr.left(), rr.top() + 0.5), pos2(rr.right(), rr.top() + 0.5)], Stroke::new(1.0, rgba(110, 200, 255, 0.06)));
        }
        let sc = status_color(&e.status);
        let d = pos2(rr.left() + 5.0, rr.top() + 13.0);
        p.circle_filled(d, 3.5, sc);
        if e.bad() {
            radial(&p, d, 9.0, fade(sc, 0.35));
        }
        let tr = text(&p, pos2(rr.right(), rr.top() + 13.0), Align2::RIGHT_CENTER, &ago(e.at), mono(10.0), DIM2, 0.0);
        let mut right = tr.left() - 12.0;
        if app.provider.is_some() && e.bad() && !e.fix.is_empty() {
            let fb = Rect::from_min_size(pos2(right - 46.0, rr.top() + 3.0), vec2(46.0, 20.0));
            if ghost(app, ui, &p, fb, &format!("health-fix-{}", e.id), "Fix", !app.ai_busy, false) {
                fix(app, e.id);
            }
            right = fb.left() - 10.0;
        }
        let by = if e.status == "checking" { "checking...".to_string() } else { e.by.clone() };
        let byr = text(&p, pos2(right, rr.top() + 13.0), Align2::RIGHT_CENTER, &by, font(10.0), DIM2, 0.0);
        text_fit(&p, pos2(rr.left() + 16.0, rr.top() + 13.0), Align2::LEFT_CENTER, &e.title, font(12.0), TEXT, 0.0, byr.left() - rr.left() - 28.0);
        let sub = if e.action.is_empty() { e.summary.clone() } else { format!("{}  →  {}", e.summary, e.action) };
        text_fit(&p, pos2(rr.left() + 16.0, rr.top() + 29.0), Align2::LEFT_CENTER, &sub, font(10.5), DIM, 0.0, rr.width() - 16.0);
    }
    let (c, m, g) = app.health.peaks_24h();
    let mut peaks = format!("Peaks in the last 24 h: CPU {c:.0}% · RAM {m:.0}%");
    if g > 0.0 {
        peaks.push_str(&format!(" · GPU {g:.0}°C"));
    }
    peaks.push_str(" · stats logged hourly");
    text(&p, pos2(r.left() + 14.0, y + 12.0), Align2::LEFT_CENTER, &peaks, font(10.0), DIM2, 0.0);
}
