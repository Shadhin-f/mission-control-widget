// Mission Control, Rust (Tauri) edition. The interface in ../../renderer is shared with the Electron
// edition unchanged; bridge.js gives it the same `window.mc` API, and everything behind that API is here.
mod claude;
mod cleaner;
mod codex;
mod credits;
mod login;
mod network;
mod processes;
mod settings;
mod stats;
mod util;

// ---- Shared with the pure Rust edition (../../rust-native/src), compiled unchanged ----
// power.rs reads its settings through `crate::backend::Backend::get` and sizes its history with
// `crate::app::HIST`; here those are this edition's App state and the interface's chart length.
mod backend {
    pub type Backend = crate::App;
}
mod app {
    pub const HIST: usize = 90;
}
#[path = "../../../rust-native/src/power.rs"]
mod power;
/// Extreme mode's centred dock (replaces the taskbar), drawn on its own thread
#[cfg(windows)]
#[path = "../../../rust-native/src/dock.rs"]
mod dock;

#[cfg(windows)]
mod extreme;
#[cfg(target_os = "macos")]
mod native_mac;
#[cfg(windows)]
#[allow(dead_code)] // the taskbar helper is used by the pure Rust edition only
mod native_win;
#[cfg(target_os = "macos")]
mod sys_mac;
#[cfg(windows)]
mod sys_win;

#[cfg(target_os = "macos")]
use native_mac as native;
#[cfg(windows)]
use native_win as native;

use serde_json::{json, Value};
use settings::{flag, Settings};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};

const W: f64 = 800.0;
const H: f64 = 700.0;
// Small while collapsed (only the notch, bubble and toast) so idle compositing stays cheap.
const SMALL_H: f64 = 150.0;

pub struct App {
    handle: AppHandle,
    settings: Mutex<Settings>,
    idle: AtomicBool,
    working: AtomicBool,
    size: Mutex<String>,
    stats: Arc<stats::Stats>,
    net: Arc<network::NetMon>,
    claude: Arc<claude::Claude>,
    codex: Arc<codex::Codex>,
    #[cfg_attr(not(windows), allow(dead_code))] // only Windows changes system colours
    touch_system: bool,
    cursor_gen: AtomicU64,
    work_area: Mutex<Option<(i32, i32, u32, u32, f64)>>,
    /// live power draw and the energy log (started once the state exists)
    power: std::sync::OnceLock<Arc<power::PowerMon>>,
    #[cfg(windows)]
    dock: dock::Dock,
    #[cfg(windows)]
    dock_on: AtomicBool,
}

type S<'a> = State<'a, Arc<App>>;

fn state(h: &AppHandle) -> Arc<App> {
    h.state::<Arc<App>>().inner().clone()
}

impl App {
    fn emit(&self, ch: &str, v: impl serde::Serialize + Clone) {
        let _ = self.handle.emit(ch, v);
    }
    fn main(&self) -> Option<WebviewWindow> {
        self.handle.get_webview_window("main")
    }
    fn get(&self, k: &str) -> Value {
        self.settings.lock().unwrap().get(k).cloned().unwrap_or(Value::Null)
    }
    fn on(&self, k: &str) -> bool {
        flag(&self.settings.lock().unwrap(), k)
    }
    fn save(&self) {
        settings::save(&self.settings.lock().unwrap());
    }
    fn any_ai(&self) -> bool {
        self.claude.found() || self.codex.found()
    }

    fn public_settings(&self) -> Value {
        let mut s = self.settings.lock().unwrap().clone();
        s.insert("claudeFound".into(), json!(self.claude.found()));
        s.insert("ai".into(), json!({ "claude": self.claude.found(), "codex": self.codex.found() }));
        s.insert("codexEditorAvailable".into(), json!(self.codex.editor_exe.lock().unwrap().is_some()));
        s.insert("platform".into(), json!(if cfg!(target_os = "macos") { "darwin" } else { "win32" }));
        Value::Object(s)
    }

    // ---------------- window ----------------
    fn place(&self) {
        let Some(w) = self.main() else { return };
        let Some(m) = w.primary_monitor().ok().flatten().or_else(|| w.current_monitor().ok().flatten()) else { return };
        let scale = m.scale_factor();
        let wa = m.work_area();
        *self.work_area.lock().unwrap() = Some((wa.position.x, wa.position.y, wa.size.width, wa.size.height, scale));
        let full = *self.size.lock().unwrap() == "full";
        let (pw, ph) = ((W * scale).round() as i32, ((if full { H } else { SMALL_H }) * scale).round() as i32);
        let x = wa.position.x + (wa.size.width as i32 - pw) / 2;
        let _ = w.set_size(PhysicalSize::new(pw as u32, ph as u32));
        let _ = w.set_position(PhysicalPosition::new(x, wa.position.y));
    }

    fn show_inactive(&self, w: &WebviewWindow) {
        #[cfg(windows)]
        {
            if let Ok(h) = w.hwnd() {
                native::show_inactive(h.0 as isize);
                return;
            }
        }
        let _ = w.show();
    }

    fn hide(&self, w: &WebviewWindow) {
        #[cfg(windows)]
        {
            if let Ok(h) = w.hwnd() {
                native::hide(h.0 as isize);
                return;
            }
        }
        let _ = w.hide();
    }

    #[cfg(windows)]
    fn own_hwnds(&self) -> Vec<isize> {
        self.main().into_iter().filter_map(|w| w.hwnd().ok().map(|h| h.0 as isize)).collect()
    }

    fn open_mode(&self, mode: &str) {
        if self.idle.load(Ordering::SeqCst) {
            return;
        }
        let mode = if mode == "chat" && !self.any_ai() { "panel" } else { mode }; // no Claude Code or Codex: no chat
        if let Some(w) = self.main() {
            self.show_inactive(&w);
            let _ = w.set_focus();
        }
        self.emit("ui:open", mode);
    }

    // ---------------- work / idle ----------------
    fn start_work(self: &Arc<Self>) {
        if self.working.swap(true, Ordering::SeqCst) {
            return;
        }
        self.stats.start(self.handle.clone());
        self.net.start_live(self.handle.clone());
        let gen = self.cursor_gen.fetch_add(1, Ordering::SeqCst) + 1;
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            let mut last = (f64::NAN, f64::NAN);
            while me.cursor_gen.load(Ordering::SeqCst) == gen {
                if let Ok(p) = me.handle.cursor_position() {
                    if (p.x, p.y) != last {
                        last = (p.x, p.y);
                        let scale = me.main().and_then(|w| w.scale_factor().ok()).unwrap_or(1.0);
                        me.emit("cursor", json!({ "x": (p.x / scale).round(), "y": (p.y / scale).round() }));
                    }
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        });
    }

    fn stop_work(&self) {
        self.working.store(false, Ordering::SeqCst);
        self.stats.stop();
        self.net.stop_live();
        self.cursor_gen.fetch_add(1, Ordering::SeqCst);
    }

    fn enter_idle(self: &Arc<Self>) {
        if self.idle.swap(true, Ordering::SeqCst) {
            return;
        }
        self.stop_work();
        self.emit("idle", true);
        if let Some(w) = self.main() {
            self.hide(&w);
        }
        #[cfg(windows)]
        self.dock.send(dock::Msg::Idle(true));
        self.update_tray();
    }

    fn exit_idle(self: &Arc<Self>) {
        if !self.idle.swap(false, Ordering::SeqCst) {
            return;
        }
        if let Some(w) = self.main() {
            self.show_inactive(&w);
        }
        #[cfg(windows)]
        self.dock.send(dock::Msg::Idle(false));
        self.emit("idle", false);
        self.start_work();
        self.update_tray();
    }

    // ---------------- tray ----------------
    fn tray_menu(&self) -> tauri::Result<Menu<tauri::Wry>> {
        let h = &self.handle;
        let menu = Menu::new(h)?;
        menu.append(&MenuItem::with_id(h, "open", "Open Mission Control", true, Some("Ctrl+Alt+M"))?)?;
        if self.any_ai() {
            let label = if self.claude.found() { "Ask Claude" } else { "Ask Codex" };
            menu.append(&MenuItem::with_id(h, "chat", label, true, Some("Ctrl+Alt+Space"))?)?;
        }
        menu.append(&PredefinedMenuItem::separator(h)?)?;
        menu.append(&CheckMenuItem::with_id(h, "openAtLogin", "Launch at startup", true, self.on("openAtLogin"), None::<&str>)?)?;
        menu.append(&CheckMenuItem::with_id(h, "idleOnFullscreen", "Idle when an app is fullscreen", true, self.on("idleOnFullscreen"), None::<&str>)?)?;
        if cfg!(windows) {
            menu.append(&CheckMenuItem::with_id(h, "extremeMode", "Extreme mode (themed taskbar)", true, self.on("extremeMode"), None::<&str>)?)?;
        }
        menu.append(&MenuItem::with_id(h, "reload", "Reload widget", true, None::<&str>)?)?;
        menu.append(&PredefinedMenuItem::separator(h)?)?;
        menu.append(&MenuItem::with_id(h, "quit", "Quit", true, None::<&str>)?)?;
        Ok(menu)
    }

    fn update_tray(&self) {
        let Some(t) = self.handle.tray_by_id("main") else { return };
        let _ = t.set_tooltip(Some(if self.idle.load(Ordering::SeqCst) { "Mission Control (idle: fullscreen app)" } else { "Mission Control" }));
        if let Ok(m) = self.tray_menu() {
            let _ = t.set_menu(Some(m));
        }
    }

    // ---------------- settings ----------------
    fn set_setting(self: &Arc<Self>, key: &str, value: Value) {
        self.settings.lock().unwrap().insert(key.into(), value.clone());
        self.save();
        let truthy = flag(&self.settings.lock().unwrap(), key);
        match key {
            "openAtLogin" => {
                let on = truthy;
                tauri::async_runtime::spawn(async move { login::apply(on).await });
            }
            "extremeMode" => {
                self.sync_dock();
                let me = self.clone();
                tauri::async_runtime::spawn(async move {
                    if truthy {
                        me.apply_extreme().await
                    } else {
                        me.disable_extreme().await
                    }
                });
            }
            "codexFromEditor" => {
                self.codex.reset();
                self.codex.detect(truthy);
                if !self.codex.found() {
                    self.settings.lock().unwrap().insert("codexLimits".into(), Value::Null);
                    self.save();
                }
                self.send_limits();
                let me = self.clone();
                tauri::async_runtime::spawn(async move { me.refresh_codex_limits().await });
            }
            _ => {}
        }
        self.emit("settings", self.public_settings());
        self.update_tray();
    }

    // ---------------- Extreme mode (Windows) ----------------
    async fn apply_extreme(&self) {
        #[cfg(windows)]
        {
            if !self.touch_system {
                return;
            }
            // After a crash the taskbar is still themed: keep the saved originals rather than backing up our theme
            let themed = extreme::is_themed().await;
            if !themed || self.get("extremeBackup").is_null() {
                let b = extreme::backup().await;
                self.settings.lock().unwrap().insert("extremeBackup".into(), b);
                self.save();
            }
            extreme::apply().await;
            // The dock replaces the taskbar: auto-hide it (it still slides up at the bottom edge)
            if self.get("dockBackup").is_null() {
                let was = dock::taskbar_autohide();
                self.settings.lock().unwrap().insert("dockBackup".into(), json!({ "autohide": was }));
                self.save();
            }
            dock::set_taskbar_autohide(true);
        }
    }

    async fn disable_extreme(&self) {
        #[cfg(windows)]
        {
            if !self.touch_system {
                return;
            }
            extreme::restore(&self.get("extremeBackup")).await;
            self.restore_taskbar();
            self.settings.lock().unwrap().insert("extremeBackup".into(), Value::Null);
            self.settings.lock().unwrap().insert("dockBackup".into(), Value::Null);
            self.save();
        }
    }

    /// Gives the taskbar back its own setting and removes the dock's reserved strip.
    #[cfg(windows)]
    fn restore_taskbar(&self) {
        dock::release_appbar();
        let b = self.get("dockBackup");
        if b.is_object() {
            dock::set_taskbar_autohide(b["autohide"].as_bool().unwrap_or(false));
        }
    }

    /// Extreme mode shows the Mission Control dock in place of the taskbar (Windows).
    fn sync_dock(&self) {
        #[cfg(windows)]
        {
            let on = self.on("extremeMode");
            if on != self.dock_on.swap(on, Ordering::SeqCst) {
                self.dock.send(dock::Msg::On(on));
            }
        }
    }

    // ---------------- AI plan limits ----------------
    fn all_limits(&self) -> Value {
        let claude = if self.claude.found() {
            let l = self.get("claudeLimits");
            if l.is_object() {
                let mut w = Vec::new();
                if l["fiveHour"].is_object() {
                    let mut x = l["fiveHour"].clone();
                    x["label"] = json!("5h");
                    w.push(x);
                }
                if l["sevenDay"].is_object() {
                    let mut x = l["sevenDay"].clone();
                    x["label"] = json!("7d");
                    w.push(x);
                }
                json!({ "windows": w, "at": l["at"] })
            } else {
                Value::Null
            }
        } else {
            Value::Null
        };
        let codex = if self.codex.found() { self.get("codexLimits") } else { Value::Null };
        json!({ "claude": claude, "codex": codex })
    }

    fn send_limits(&self) {
        self.emit("limits", self.all_limits());
    }

    fn on_claude_limits(&self, l: Option<Value>) {
        if let Some(Value::Object(l)) = l {
            let mut s = self.settings.lock().unwrap();
            let mut prev = match s.get("claudeLimits") {
                Some(Value::Object(p)) => p.clone(),
                _ => Default::default(),
            };
            for (k, v) in l {
                if !v.is_null() {
                    prev.insert(k, v);
                }
            }
            s.insert("claudeLimits".into(), Value::Object(prev));
            drop(s);
            self.save();
        }
        self.send_limits();
    }

    async fn refresh_codex_limits(&self) {
        if !self.codex.found() || self.idle.load(Ordering::SeqCst) {
            return;
        }
        if let Some(l) = self.codex.read_limits().await {
            self.settings.lock().unwrap().insert("codexLimits".into(), l);
            self.save();
        }
        self.send_limits();
    }

    fn auto_probe(&self) {
        if self.idle.load(Ordering::SeqCst) {
            return;
        }
        // Only while a 5-hour window is already open, so the widget never starts a usage window on its own
        let open = self.get("claudeLimits")["fiveHour"]["resetsAt"].as_f64().unwrap_or(0.0) > util::now_ms();
        if self.claude.found() && self.on("autoLimits") && !self.claude.busy() && open {
            self.claude.probe_limits();
        }
    }

    fn shutdown(&self) {
        self.stop_work();
        self.claude.stop();
        self.codex.stop();
        if let Some(p) = self.power.get() {
            p.flush();
        }
        // The taskbar goes back to the user's colours (and its own auto-hide setting) whenever the widget
        // isn't running; the saved originals stay in settings, and Extreme mode returns on the next launch.
        #[cfg(windows)]
        if self.touch_system && self.on("extremeMode") {
            extreme::restore_sync(&self.get("extremeBackup"));
            self.restore_taskbar();
        }
    }

    /// The Power tab's data: the latest power snapshot, as JSON for the interface.
    fn power_json(&self) -> Value {
        let Some(p) = self.power.get() else { return json!({ "ready": false }) };
        let s = p.snap.lock().unwrap().clone();
        let src = |s: power::Src| match s {
            power::Src::Measured => "measured",
            power::Src::Estimated => "estimated",
            power::Src::Set => "set",
            power::Src::Included => "included",
        };
        let day = |d: &power::Day| json!({ "wh": d.wh, "s": d.s, "mwh": d.mwh });
        let sm = &s.summary;
        json!({
            "ready": s.ready,
            "parts": s.parts.iter().map(|p| json!({ "kind": p.kind, "name": p.name, "watts": p.watts, "src": src(p.src), "how": p.how, "load": p.load })).collect::<Vec<_>>(),
            "dcW": s.dc_w, "wallW": s.wall_w, "measured": s.measured,
            "laptop": s.laptop, "onBattery": s.on_battery, "batteryW": s.battery_w,
            "hist": s.hist, "count": s.count,
            "summary": {
                "today": day(&sm.today), "last7": sm.last7, "last30": sm.last30, "last365": sm.last365,
                "daysTracked": sm.days_tracked, "dailyWh": sm.daily_wh, "basis": sm.basis, "avgW": sm.avg_w, "hours": sm.hours,
            },
            "currency": s.currency, "restAutoW": s.rest_auto_w, "effAuto": s.eff_auto,
        })
    }
}

// ---------------- commands (the `window.mc` API) ----------------

#[tauri::command]
fn set_interactive(s: S, on: bool) {
    if let Some(w) = s.main() {
        let _ = w.set_ignore_cursor_events(!on);
    }
}

#[tauri::command]
fn ui_focus(s: S) {
    if let Some(w) = s.main() {
        let _ = w.set_focus();
    }
}

#[tauri::command]
fn ui_size(s: S, size: String) -> bool {
    let changed = {
        let mut cur = s.size.lock().unwrap();
        let c = *cur != size;
        *cur = size;
        c
    };
    if changed {
        s.place();
    }
    true
}

#[tauri::command]
fn settings_get(s: S) -> Value {
    s.public_settings()
}

#[tauri::command]
fn settings_set(s: S, key: String, value: Value) -> Value {
    s.inner().set_setting(&key, value);
    s.public_settings()
}

#[tauri::command]
fn static_get(s: S) -> Value {
    s.stats.static_info.lock().unwrap().clone()
}

#[tauri::command]
async fn procs_list() -> Result<Value, String> {
    tokio::task::spawn_blocking(processes::list).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn procs_kill(pids: Vec<u32>) -> Result<Value, String> {
    processes::kill(pids).await
}

#[tauri::command]
async fn cleaner_scan(app: AppHandle) -> Result<Value, String> {
    tokio::task::spawn_blocking(move || {
        let running = processes::running_names();
        Value::Array(cleaner::scan(&running, |p| {
            let _ = app.emit("cleaner:progress", p);
        }))
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn cleaner_clean(app: AppHandle, ids: Vec<String>) -> Result<Value, String> {
    tokio::task::spawn_blocking(move || {
        Value::Array(cleaner::clean(&ids, |p| {
            let _ = app.emit("cleaner:progress", p);
        }))
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn speed_run(app: AppHandle) -> Result<Value, String> {
    let s = state(&app);
    let r = s.net.speed_test(&app).await?;
    {
        let mut st = s.settings.lock().unwrap();
        let mut hist = st.get("speedHistory").and_then(|v| v.as_array().cloned()).unwrap_or_default();
        hist.insert(0, r.clone());
        hist.truncate(8);
        st.insert("speedHistory".into(), Value::Array(hist));
    }
    s.save();
    Ok(r)
}

#[tauri::command]
fn ai_send(s: S, provider: String, prompt: String, mode: Option<String>) {
    let mode = mode.unwrap_or_else(|| s.get("chatMode").as_str().unwrap_or("auto").to_string());
    if provider == "codex" {
        s.codex.send(prompt, mode);
    } else {
        s.claude.send(prompt, mode);
    }
}

#[tauri::command]
fn ai_stop(s: S) {
    s.claude.stop();
    s.codex.stop();
}

#[tauri::command]
fn ai_new(s: S) {
    s.claude.reset();
    s.codex.reset();
}

#[tauri::command]
fn app_quit(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
async fn sys_helper(cmd: String, args: Value) -> Result<Value, String> {
    const CMDS: [&str; 14] = ["state", "bt", "brightness", "setVolume", "stepVolume", "setMute", "setDefault", "setRadio", "media", "setBrightness", "setDark", "lock", "sleep", "requestLocation"];
    if !CMDS.contains(&cmd.as_str()) {
        return Err(format!("Unknown command {cmd}"));
    }
    #[cfg(windows)]
    {
        sys_win::helper(cmd, args).await
    }
    #[cfg(target_os = "macos")]
    {
        sys_mac::sys_helper(cmd, args).await
    }
}

#[tauri::command]
async fn sys_wifi() -> Result<Value, String> {
    #[cfg(windows)]
    {
        Ok(sys_win::wifi().await)
    }
    #[cfg(target_os = "macos")]
    {
        sys_mac::wifi().await
    }
}

#[tauri::command]
async fn sys_wifi_connect(ssid: String) -> Result<Value, String> {
    #[cfg(windows)]
    {
        Ok(sys_win::wifi_connect(ssid).await)
    }
    #[cfg(target_os = "macos")]
    {
        sys_mac::wifi_connect(ssid).await
    }
}

#[tauri::command]
async fn sys_wifi_disconnect() -> Result<Value, String> {
    #[cfg(windows)]
    {
        Ok(sys_win::wifi_disconnect().await)
    }
    #[cfg(target_os = "macos")]
    {
        sys_mac::wifi_disconnect().await
    }
}

#[tauri::command]
async fn sys_plans() -> Result<Value, String> {
    #[cfg(windows)]
    {
        Ok(sys_win::power_plans().await)
    }
    #[cfg(target_os = "macos")]
    {
        Ok(sys_mac::power_plans().await)
    }
}

#[tauri::command]
async fn sys_set_plan(guid: String) -> Result<Value, String> {
    #[cfg(windows)]
    {
        sys_win::set_power_plan(guid).await
    }
    #[cfg(target_os = "macos")]
    {
        sys_mac::set_power_plan(guid).await
    }
}

#[tauri::command]
async fn sys_session(kind: String) -> Result<Value, String> {
    #[cfg(windows)]
    {
        sys_win::session(kind).await
    }
    #[cfg(target_os = "macos")]
    {
        sys_mac::session(kind).await
    }
}

fn settings_page(page: &str) -> Option<String> {
    #[cfg(windows)]
    let v = match page {
        "bluetooth" => "ms-settings:bluetooth",
        "wifi" => "ms-settings:network-wifi",
        "network" => "ms-settings:network",
        "sound" => "ms-settings:sound",
        "display" => "ms-settings:display",
        "power" => "ms-settings:powersleep",
        "location" => "ms-settings:privacy-location",
        "devices" => "ms-settings:connecteddevices",
        "nightlight" => "ms-settings:nightlight",
        "notifications" => "ms-settings:notifications",
        _ => return None,
    };
    #[cfg(target_os = "macos")]
    let v = match page {
        "bluetooth" | "devices" => "x-apple.systempreferences:com.apple.BluetoothSettings",
        "wifi" => "x-apple.systempreferences:com.apple.wifi-settings-extension",
        "network" => "x-apple.systempreferences:com.apple.Network-Settings.extension",
        "sound" => "x-apple.systempreferences:com.apple.Sound-Settings.extension",
        "display" | "nightlight" => "x-apple.systempreferences:com.apple.Displays-Settings.extension",
        "power" => "x-apple.systempreferences:com.apple.Battery-Settings.extension",
        "location" => "x-apple.systempreferences:com.apple.preference.security?Privacy_LocationServices",
        "notifications" => "x-apple.systempreferences:com.apple.Notifications-Settings.extension",
        _ => return None,
    };
    Some(v.to_string())
}

#[tauri::command]
fn sys_open(app: AppHandle, page: String) {
    use tauri_plugin_opener::OpenerExt;
    if let Some(url) = settings_page(&page) {
        let _ = app.opener().open_url(url, None::<&str>);
    }
}

#[tauri::command]
fn limits_get(s: S) -> Value {
    s.all_limits()
}

#[tauri::command]
fn limits_refresh(s: S) {
    if s.claude.found() {
        s.claude.probe_limits();
    }
    let me = s.inner().clone();
    tauri::async_runtime::spawn(async move { me.refresh_codex_limits().await });
}

// ---- Power and Credits tabs (shared with the pure Rust edition) ----

#[tauri::command]
fn power_get(s: S) -> Value {
    s.power_json()
}

#[tauri::command]
async fn credits_get(force: bool) -> Value {
    credits::get(force).await
}

/// Opens a contributor's or the project's GitHub page in the browser (GitHub links only).
#[tauri::command]
fn open_url(app: AppHandle, url: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    if !url.starts_with("https://github.com/") {
        return Err("Only GitHub links can be opened".into());
    }
    app.opener().open_url(url, None::<&str>).map_err(|e| e.to_string())
}

/// Dev only: receives the MC_SELFTEST report and writes it to that file.
#[tauri::command]
fn dev_report(report: Value) {
    if let Some(path) = std::env::var_os("MC_SELFTEST") {
        let _ = std::fs::write(path, serde_json::to_string_pretty(&report).unwrap_or_default());
    }
}

// ---------------- tray icon (the pet's face, drawn in code like the Electron edition) ----------------
fn tray_icon() -> tauri::image::Image<'static> {
    let s = 32usize;
    let mut buf = vec![0u8; s * s * 4];
    for y in 0..s {
        for x in 0..s {
            let (dx, dy) = (x as f64 - 15.5, y as f64 - 15.5);
            let d = (dx * dx + dy * dy).sqrt();
            let a = (14.0 - d).clamp(0.0, 1.0);
            let eye = ((dx + 5.0).abs() < 2.0 || (dx - 5.0).abs() < 2.0) && (dy + 1.0).abs() < 3.5;
            let t = d / 14.0;
            let i = (y * s + x) * 4;
            // RGBA (the Electron code fills BGRA)
            if eye {
                buf[i..i + 3].copy_from_slice(&[10, 18, 40]);
            } else {
                buf[i] = (70.0 + 60.0 * t).round() as u8;
                buf[i + 1] = (240.0 - 90.0 * t).round() as u8;
                buf[i + 2] = 255;
            }
            buf[i + 3] = (a * 255.0).round() as u8;
        }
    }
    tauri::image::Image::new_owned(buf, s as u32, s as u32)
}

pub fn run() {
    use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};
    let panel_key = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyM);
    let chat_key = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::Space);

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| state(app).open_mode("panel")))
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(move |app, sc, ev| {
                    if ev.state() != ShortcutState::Pressed {
                        return;
                    }
                    if sc == &panel_key {
                        state(app).open_mode("panel");
                    } else if sc == &chat_key {
                        state(app).open_mode("chat");
                    }
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            set_interactive, ui_focus, ui_size, settings_get, settings_set, static_get, procs_list, procs_kill, cleaner_scan, cleaner_clean,
            speed_run, ai_send, ai_stop, ai_new, app_quit, sys_helper, sys_wifi, sys_wifi_connect, sys_wifi_disconnect, sys_plans,
            sys_set_plan, sys_session, sys_open, limits_get, limits_refresh, power_get, credits_get, open_url, dev_report
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            #[cfg(target_os = "macos")]
            {
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);
                native_mac::init_helper(app.path().resource_dir().ok());
            }
            // Dev capture / diagnostic runs never change Windows colours or startup
            let touch_system = std::env::var_os("MC_SHOTS").is_none() && std::env::var_os("MC_DIAG").is_none();

            let h1 = handle.clone();
            let h2 = handle.clone();
            let h3 = handle.clone();
            let claude = claude::Claude::new(
                Arc::new(move |mut e: Value| {
                    e["provider"] = json!("claude");
                    let _ = h1.emit("ai", e);
                }),
                Arc::new(move |l| state(&h2).on_claude_limits(l)),
            );
            let codex = codex::Codex::new(Arc::new(move |mut e: Value| {
                e["provider"] = json!("codex");
                let _ = h3.emit("ai", e);
            }));
            let s = settings::load();
            // A real Codex CLI always counts; the copy bundled with an editor extension only when opted in
            codex.detect(flag(&s, "codexFromEditor"));
            if std::env::var_os("MC_NO_AI").is_some() {
                *claude.exe.lock().unwrap() = None; // dev: preview the no-AI layout
                *codex.exe.lock().unwrap() = None;
            }
            let st = Arc::new(App {
                handle: handle.clone(),
                settings: Mutex::new(s),
                idle: AtomicBool::new(false),
                working: AtomicBool::new(false),
                size: Mutex::new("small".into()),
                stats: stats::Stats::new(),
                net: network::NetMon::new(),
                claude,
                codex: codex.clone(),
                touch_system,
                cursor_gen: AtomicU64::new(0),
                work_area: Mutex::new(None),
                power: std::sync::OnceLock::new(),
                #[cfg(windows)]
                dock: dock::Dock::spawn(),
                #[cfg(windows)]
                dock_on: AtomicBool::new(false),
            });
            // Power readings run on their own thread for as long as the app runs (also while idle)
            let _ = st.power.set(power::PowerMon::start(st.clone()));
            app.manage(st.clone());
            {
                let h = handle.clone();
                *codex.on_turn_finished.lock().unwrap() = Some(Arc::new(move || {
                    let s = state(&h);
                    tauri::async_runtime::spawn(async move {
                        tokio::time::sleep(Duration::from_millis(1500)).await;
                        s.refresh_codex_limits().await;
                    });
                }));
            }

            // ---- the widget window ----
            #[allow(unused_mut)]
            let mut b = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("Mission Control")
                .inner_size(W, SMALL_H)
                .decorations(false)
                .transparent(true)
                .resizable(false)
                .maximizable(false)
                .minimizable(false)
                .skip_taskbar(true)
                .always_on_top(true)
                .shadow(false)
                .focused(false)
                // Windows: created visible but unfocused and off-screen, then placed; see App::hide
                .visible(cfg!(windows))
                .position(-32000.0, -32000.0)
                .initialization_script(include_str!("bridge.js"));
            #[cfg(target_os = "macos")]
            {
                b = b.visible_on_all_workspaces(true);
            }
            let win = b.build()?;
            st.place();
            let _ = win.set_ignore_cursor_events(true);
            st.show_inactive(&win);
            {
                let h = handle.clone();
                win.on_window_event(move |e| match e {
                    WindowEvent::Focused(false) => {
                        let _ = h.emit("win:blur", ());
                    }
                    WindowEvent::ScaleFactorChanged { .. } => {
                        state(&h).place();
                    }
                    _ => {}
                });
            }

            // ---- tray ----
            let menu = st.tray_menu()?;
            TrayIconBuilder::with_id("main")
                .icon(tray_icon())
                .tooltip("Mission Control")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, ev| {
                    let s = state(app);
                    match ev.id().as_ref() {
                        "open" => s.open_mode("panel"),
                        "chat" => s.open_mode("chat"),
                        "reload" => {
                            if let Some(w) = s.main() {
                                let _ = w.eval("location.reload()");
                            }
                        }
                        "quit" => app.exit(0),
                        k @ ("openAtLogin" | "idleOnFullscreen" | "extremeMode") => {
                            let v = !s.on(k);
                            s.set_setting(k, json!(v));
                        }
                        _ => {}
                    }
                })
                .on_tray_icon_event(|tray, ev| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = ev {
                        state(tray.app_handle()).open_mode("panel");
                    }
                })
                .build(app)?;

            let gs = app.global_shortcut();
            let _ = gs.register(panel_key);
            let _ = gs.register(chat_key);

            if std::env::var_os("MC_SELFTEST").is_some() {
                let w = win.clone();
                let speed = std::env::var_os("MC_SELFTEST_SPEED").is_some();
                let chat = std::env::var("MC_SELFTEST_CHAT").ok();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(4)).await;
                    let flags = format!("window.__MC_SELFTEST_SPEED = {speed}; window.__MC_SELFTEST_CHAT = {};", serde_json::to_string(&chat).unwrap());
                    let _ = w.eval(format!("{flags}\n{}", include_str!("selftest.js")));
                });
            }

            if st.on("extremeMode") {
                st.sync_dock();
                let s = st.clone();
                tauri::async_runtime::spawn(async move { s.apply_extreme().await });
            }

            // ---- background loops ----
            st.start_work();
            {
                // Idle while a fullscreen app is in front; re-place after display changes
                let s = st.clone();
                tauri::async_runtime::spawn(async move {
                    loop {
                        tokio::time::sleep(Duration::from_millis(1500)).await;
                        #[cfg(windows)]
                        let full = s.on("idleOnFullscreen") && native::is_foreground_fullscreen(&s.own_hwnds());
                        #[cfg(target_os = "macos")]
                        let full = s.on("idleOnFullscreen") && native::is_foreground_fullscreen();
                        if full {
                            s.enter_idle();
                        } else {
                            s.exit_idle();
                        }
                        if let Some(w) = s.main() {
                            if let Some(m) = w.primary_monitor().ok().flatten() {
                                let wa = m.work_area();
                                let now = Some((wa.position.x, wa.position.y, wa.size.width, wa.size.height, m.scale_factor()));
                                if now != *s.work_area.lock().unwrap() {
                                    s.place();
                                }
                            }
                        }
                    }
                });
            }
            {
                let s = st.clone();
                tauri::async_runtime::spawn(async move {
                    loop {
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        if !s.idle.load(Ordering::SeqCst) {
                            s.emit("user-idle", native::user_idle_secs());
                        }
                    }
                });
            }
            {
                let s = st.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(6)).await;
                    s.refresh_codex_limits().await;
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    s.auto_probe();
                    let mut n = 0u64;
                    loop {
                        tokio::time::sleep(Duration::from_secs(60)).await;
                        n += 1;
                        if n % 5 == 0 {
                            s.refresh_codex_limits().await;
                        }
                        if n % 10 == 0 {
                            s.auto_probe();
                        }
                    }
                });
            }
            // Keep sign-in fast: verify the startup entry once things settle. Dev runs never touch it.
            if touch_system && std::env::var_os("MC_NO_LOGIN").is_none() {
                let on = st.on("openAtLogin");
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(20)).await;
                    login::apply(on).await;
                });
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Mission Control");

    app.run(|h, ev| match ev {
        tauri::RunEvent::ExitRequested { code, api, .. } => {
            // Closing the last window must not quit the widget; only Quit (an explicit exit code) does
            if code.is_none() && h.get_webview_window("main").is_some() {
                api.prevent_exit();
            }
        }
        tauri::RunEvent::Exit => state(h).shutdown(),
        _ => {}
    });
}
