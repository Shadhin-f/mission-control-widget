// Everything behind the interface that isn't a shared backend module: settings with their side effects,
// AI plan limits, Extreme mode and the work/idle switch. A port of rust/src-tauri/src/lib.rs's `App`
// without the web view; events still go out through the host's `emit` into the UI queue.
use crate::settings::{self, flag, Settings};
use crate::{claude, codex, login, network, stats, util};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

pub struct Backend {
    pub settings: Mutex<Settings>,
    pub idle: AtomicBool,
    working: AtomicBool,
    pub stats: Arc<stats::Stats>,
    pub net: Arc<network::NetMon>,
    pub claude: Arc<claude::Claude>,
    pub codex: Arc<codex::Codex>,
    /// Dev capture / diagnostic runs never change Windows colours or startup
    pub touch_system: bool,
}

pub fn emit(ch: &str, v: impl serde::Serialize + Clone) {
    let _ = AppHandle.emit(ch, v);
}

pub fn spawn<F: std::future::Future<Output = ()> + Send + 'static>(f: F) {
    tauri::async_runtime::spawn(f);
}

impl Backend {
    pub fn new() -> Arc<Self> {
        let touch_system = std::env::var_os("MC_SHOTS").is_none() && std::env::var_os("MC_DIAG").is_none();
        let claude = claude::Claude::new(
            Arc::new(|mut e: Value| {
                e["provider"] = json!("claude");
                emit("ai", e);
            }),
            Arc::new(|l| emit("claude-limits", l)),
        );
        let codex = codex::Codex::new(Arc::new(|mut e: Value| {
            e["provider"] = json!("codex");
            emit("ai", e);
        }));
        let s = settings::load();
        // A real Codex CLI always counts; the copy bundled with an editor extension only when opted in
        codex.detect(flag(&s, "codexFromEditor"));
        if std::env::var_os("MC_NO_AI").is_some() {
            *claude.exe.lock().unwrap() = None; // dev: preview the no-AI layout
            *codex.exe.lock().unwrap() = None;
        }
        *codex.on_turn_finished.lock().unwrap() = Some(Arc::new(|| {
            spawn(async {
                tokio::time::sleep(Duration::from_millis(1500)).await;
                emit("codex-turn", true);
            });
        }));
        Arc::new(Self {
            settings: Mutex::new(s),
            idle: AtomicBool::new(false),
            working: AtomicBool::new(false),
            stats: stats::Stats::new(),
            net: network::NetMon::new(),
            claude,
            codex,
            touch_system,
        })
    }

    pub fn get(&self, k: &str) -> Value {
        self.settings.lock().unwrap().get(k).cloned().unwrap_or(Value::Null)
    }
    pub fn on(&self, k: &str) -> bool {
        flag(&self.settings.lock().unwrap(), k)
    }
    pub fn save(&self) {
        settings::save(&self.settings.lock().unwrap());
    }
    pub fn any_ai(&self) -> bool {
        self.claude.found() || self.codex.found()
    }

    pub fn public_settings(&self) -> Value {
        let mut s = self.settings.lock().unwrap().clone();
        s.insert("claudeFound".into(), json!(self.claude.found()));
        s.insert("ai".into(), json!({ "claude": self.claude.found(), "codex": self.codex.found() }));
        s.insert("codexEditorAvailable".into(), json!(self.codex.editor_exe.lock().unwrap().is_some()));
        s.insert("platform".into(), json!(if cfg!(target_os = "macos") { "darwin" } else { "win32" }));
        Value::Object(s)
    }

    // ---------------- work / idle ----------------
    pub fn start_work(self: &Arc<Self>) {
        if self.working.swap(true, Ordering::SeqCst) {
            return;
        }
        self.stats.start(AppHandle);
        self.net.start_live(AppHandle);
    }

    pub fn stop_work(&self) {
        self.working.store(false, Ordering::SeqCst);
        self.stats.stop();
        self.net.stop_live();
    }

    // ---------------- settings ----------------
    pub fn set_setting(self: &Arc<Self>, key: &str, value: Value) -> Value {
        self.settings.lock().unwrap().insert(key.into(), value);
        self.save();
        let truthy = flag(&self.settings.lock().unwrap(), key);
        match key {
            "openAtLogin" => {
                if self.touch_system {
                    spawn(async move { login::apply(truthy).await });
                }
            }
            "extremeMode" => {
                let me = self.clone();
                spawn(async move {
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
                spawn(async move { me.refresh_codex_limits().await });
            }
            _ => {}
        }
        let s = self.public_settings();
        emit("settings", s.clone());
        s
    }

    // ---------------- Extreme mode (Windows) ----------------
    pub async fn apply_extreme(&self) {
        #[cfg(windows)]
        {
            if !self.touch_system {
                return;
            }
            // After a crash the taskbar is still themed: keep the saved originals rather than backing up our theme
            let themed = crate::extreme::is_themed().await;
            if !themed || self.get("extremeBackup").is_null() {
                let b = crate::extreme::backup().await;
                self.settings.lock().unwrap().insert("extremeBackup".into(), b);
                self.save();
            }
            crate::extreme::apply().await;
            // The dock replaces the taskbar: auto-hide it (it still slides up at the bottom edge)
            if self.get("dockBackup").is_null() {
                let was = crate::dock::taskbar_autohide();
                self.settings.lock().unwrap().insert("dockBackup".into(), json!({ "autohide": was }));
                self.save();
            }
            crate::dock::set_taskbar_autohide(true);
        }
    }

    pub async fn disable_extreme(&self) {
        #[cfg(windows)]
        {
            if !self.touch_system {
                return;
            }
            crate::extreme::restore(&self.get("extremeBackup")).await;
            self.restore_taskbar();
            self.settings.lock().unwrap().insert("extremeBackup".into(), Value::Null);
            self.settings.lock().unwrap().insert("dockBackup".into(), Value::Null);
            self.save();
        }
    }

    #[cfg(windows)]
    fn restore_taskbar(&self) {
        crate::dock::release_appbar();
        let b = self.get("dockBackup");
        if b.is_object() {
            crate::dock::set_taskbar_autohide(b["autohide"].as_bool().unwrap_or(false));
        }
    }

    // ---------------- AI plan limits ----------------
    pub fn all_limits(&self) -> Value {
        let claude = if self.claude.found() {
            let l = self.get("claudeLimits");
            if l.is_object() {
                let mut w = Vec::new();
                for (k, label) in [("fiveHour", "5h"), ("sevenDay", "7d")] {
                    if l[k].is_object() {
                        let mut x = l[k].clone();
                        x["label"] = json!(label);
                        w.push(x);
                    }
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

    pub fn send_limits(&self) {
        emit("limits", self.all_limits());
    }

    pub fn on_claude_limits(&self, l: Value) {
        if let Value::Object(l) = l {
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

    pub async fn refresh_codex_limits(&self) {
        if !self.codex.found() || self.idle.load(Ordering::SeqCst) {
            return;
        }
        if let Some(l) = self.codex.read_limits().await {
            self.settings.lock().unwrap().insert("codexLimits".into(), l);
            self.save();
        }
        self.send_limits();
    }

    pub fn refresh_limits(self: &Arc<Self>) {
        if self.claude.found() {
            self.claude.probe_limits();
        }
        let me = self.clone();
        spawn(async move { me.refresh_codex_limits().await });
    }

    pub fn auto_probe(&self) {
        if self.idle.load(Ordering::SeqCst) {
            return;
        }
        // Only while a 5-hour window is already open, so the widget never starts a usage window on its own
        let open = self.get("claudeLimits")["fiveHour"]["resetsAt"].as_f64().unwrap_or(0.0) > util::now_ms();
        if self.claude.found() && self.on("autoLimits") && !self.claude.busy() && open {
            self.claude.probe_limits();
        }
    }

    pub fn ai_send(&self, provider: &str, prompt: String, mode: String) {
        if provider == "codex" {
            self.codex.send(prompt, mode);
        } else {
            self.claude.send(prompt, mode);
        }
    }

    pub fn speed_done(&self, r: &Value) {
        let mut st = self.settings.lock().unwrap();
        let mut hist = st.get("speedHistory").and_then(|v| v.as_array().cloned()).unwrap_or_default();
        hist.insert(0, r.clone());
        hist.truncate(8);
        st.insert("speedHistory".into(), Value::Array(hist));
        drop(st);
        self.save();
    }

    pub fn shutdown(&self) {
        self.stop_work();
        self.claude.stop();
        self.codex.stop();
        // The taskbar goes back to the user's colours whenever the widget isn't running; the saved
        // originals stay in settings, and the theme is applied again on the next launch.
        #[cfg(windows)]
        if self.touch_system && self.on("extremeMode") {
            crate::extreme::restore_sync(&self.get("extremeBackup"));
            self.restore_taskbar();
        }
    }

    /// Background loops: limits refresh/probe and the sign-in entry check (fullscreen idle and the
    /// window placement run in the UI, which owns the window).
    pub fn start_loops(self: &Arc<Self>) {
        let s = self.clone();
        spawn(async move {
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
        if self.touch_system && std::env::var_os("MC_NO_LOGIN").is_none() {
            let on = self.on("openAtLogin");
            spawn(async move {
                tokio::time::sleep(Duration::from_secs(20)).await;
                login::apply(on).await;
            });
        }
        if self.on("extremeMode") {
            let s = self.clone();
            spawn(async move { s.apply_extreme().await });
        }
        // user idle seconds (the pet falls asleep)
        spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(5)).await;
                emit("user-idle", crate::native::user_idle_secs());
            }
        });
    }
}

/// Runs an async call on the worker runtime and posts its result back to the UI as event `ch`.
pub fn call<F>(ch: &'static str, f: F)
where
    F: std::future::Future<Output = Result<Value, String>> + Send + 'static,
{
    spawn(async move {
        let r = f.await;
        emit(ch, match r {
            Ok(v) => json!({ "ok": true, "v": v }),
            Err(e) => json!({ "ok": false, "err": e }),
        });
    });
}

/// `call` with extra context echoed back (e.g. which device a command was for).
pub fn call_ctx<F>(ch: &'static str, ctx: Value, f: F)
where
    F: std::future::Future<Output = Result<Value, String>> + Send + 'static,
{
    spawn(async move {
        let r = f.await;
        emit(ch, match r {
            Ok(v) => json!({ "ok": true, "v": v, "ctx": ctx }),
            Err(e) => json!({ "ok": false, "err": e, "ctx": ctx }),
        });
    });
}

// ---------------- system calls (the `mc.*` API the renderer used) ----------------
pub async fn sys(cmd: String, args: Value) -> Result<Value, String> {
    #[cfg(windows)]
    {
        crate::sys_win::helper(cmd, args).await
    }
    #[cfg(target_os = "macos")]
    {
        crate::sys_mac::sys_helper(cmd, args).await
    }
}

pub async fn wifi() -> Result<Value, String> {
    #[cfg(windows)]
    {
        Ok(crate::sys_win::wifi().await)
    }
    #[cfg(target_os = "macos")]
    {
        crate::sys_mac::wifi().await
    }
}

pub async fn wifi_connect(ssid: String) -> Result<Value, String> {
    #[cfg(windows)]
    {
        Ok(crate::sys_win::wifi_connect(ssid).await)
    }
    #[cfg(target_os = "macos")]
    {
        crate::sys_mac::wifi_connect(ssid).await
    }
}

pub async fn wifi_disconnect() -> Result<Value, String> {
    #[cfg(windows)]
    {
        Ok(crate::sys_win::wifi_disconnect().await)
    }
    #[cfg(target_os = "macos")]
    {
        crate::sys_mac::wifi_disconnect().await
    }
}

pub async fn power_plans() -> Result<Value, String> {
    #[cfg(windows)]
    {
        Ok(crate::sys_win::power_plans().await)
    }
    #[cfg(target_os = "macos")]
    {
        Ok(crate::sys_mac::power_plans().await)
    }
}

pub async fn set_power_plan(guid: String) -> Result<Value, String> {
    #[cfg(windows)]
    {
        crate::sys_win::set_power_plan(guid).await
    }
    #[cfg(target_os = "macos")]
    {
        crate::sys_mac::set_power_plan(guid).await
    }
}

pub async fn session(kind: String) -> Result<Value, String> {
    #[cfg(windows)]
    {
        crate::sys_win::session(kind).await
    }
    #[cfg(target_os = "macos")]
    {
        crate::sys_mac::session(kind).await
    }
}

pub fn settings_page(page: &str) -> Option<&'static str> {
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
    Some(v)
}

pub fn open_settings(page: &str) {
    if let Some(u) = settings_page(page) {
        crate::platform::open_url(u);
    }
}
