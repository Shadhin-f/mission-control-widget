// The widget: state, events from the backend, timers, window management and the frame.
// The drawing lives in notch.rs (shell, bar, pet), pages.rs, controls.rs and chat.rs; it is a port of
// renderer/app.js + styles.css, with the same sizes, colours, easing curves and timings.
use crate::anim::Anims;
use crate::backend::{self, Backend};
use crate::util::now_ms;
use eframe::egui::{self, pos2, vec2, Pos2, Rect, Vec2, ViewportCommand};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

pub const WIN_W: f32 = 800.0;
pub const WIN_H: f32 = 700.0;
/// Small while collapsed (only the notch, bubble and toast) so idle compositing stays cheap.
pub const SMALL_H: f32 = 150.0;
pub const CX: f32 = WIN_W / 2.0;
pub const BAR_H: f32 = 46.0;
pub const TABS_H: f32 = 40.0;
pub const MAX_PANEL_H: f32 = 668.0;
pub const HIST: usize = 90;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Mode {
    Collapsed,
    Panel,
    Chat,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Tab {
    Overview,
    Controls,
    Network,
    Apps,
    Cleaner,
    Settings,
    Credits,
}
pub const TABS: [Tab; 7] = [Tab::Overview, Tab::Controls, Tab::Network, Tab::Apps, Tab::Cleaner, Tab::Settings, Tab::Credits];
impl Tab {
    pub fn label(self) -> &'static str {
        match self {
            Tab::Overview => "OVERVIEW",
            Tab::Controls => "CONTROLS",
            Tab::Network => "NETWORK",
            Tab::Apps => "APPS",
            Tab::Cleaner => "CLEANER",
            Tab::Settings => "SETTINGS",
            Tab::Credits => "CREDITS",
        }
    }
    pub fn idx(self) -> usize {
        TABS.iter().position(|t| *t == self).unwrap_or(0)
    }
}

/// Shared with the pointer thread: what part of the window catches the mouse, and where the pointer is.
#[derive(Default)]
pub struct Pointer {
    /// window origin, physical px
    pub origin: (f64, f64),
    pub ppp: f32,
    /// hit regions, window points
    pub hits: Vec<Rect>,
    pub hit: bool,
    /// pointer in window points
    pub pos: Option<Pos2>,
    pub pet_center: Pos2,
    pub eye_target: Vec2,
    pub think: bool,
    /// taskbar rect, physical px (Extreme mode)
    pub taskbar: Option<(f64, f64, f64, f64)>,
    pub edge_hot: bool,
    pub ready: bool,
}

pub struct Bubble {
    pub text: String,
    pub until: f64,
}

#[derive(Default)]
pub struct Hist {
    pub cpu: VecDeque<f32>,
    pub ram: VecDeque<f32>,
    pub gpu: VecDeque<f32>,
    pub down: VecDeque<f32>,
    pub up: VecDeque<f32>,
}

#[derive(Default)]
pub struct SpeedUi {
    pub phase: String,
    pub unit: String,
    pub num: f32,
    pub num_int: bool,
    pub arc: f32,
    pub result: Value,
    pub label: String,
}

#[derive(Default)]
pub struct AppsUi {
    pub data: Value,
    pub filter: String,
    pub kill_arm: HashMap<String, f64>,
    pub next_poll: f64,
    pub active: bool,
}

#[derive(Default)]
pub struct CleanerUi {
    pub items: Vec<Value>,
    pub selected: HashSet<String>,
    pub busy: bool,
    pub cleaning: bool,
    pub scanned: bool,
    pub freed: HashMap<String, f64>,
    pub now: String,
    pub busy_id: String,
    pub confirm: bool,
    pub confirm_text: (String, String),
    pub render_gen: u64,
}

pub struct App {
    pub ctx: egui::Context,
    pub be: Arc<Backend>,
    pub q: Arc<Mutex<VecDeque<(String, Value)>>>,
    pub a: Anims,
    pub ptr: Arc<Mutex<Pointer>>,
    pub now: f64,

    pub mode: Mode,
    pub mode_since: f64,
    pub tab: Tab,
    pub prev_tab: Tab,
    pub tab_changed: f64,
    pub pinned: bool,
    pub hover_raw: bool,
    pub hover_at: Option<(bool, f64)>,
    pub shell_hover: bool,
    pub pending_mode: Option<(Mode, u32)>,
    pub shrink_at: Option<f64>,
    pub win_full: bool,
    pub interactive: bool,
    pub was_focused: bool,
    pub idle: bool,
    pub quitting: bool,

    pub stats: Value,
    pub latency: Value,
    pub static_: Value,
    pub settings: Value,
    pub hist: Hist,
    pub last_push: f64,
    pub lat_hist: VecDeque<Option<f64>>,
    pub was_offline: bool,
    pub hot_since: f64,
    pub user_idle: f64,
    pub ai_busy: bool,
    pub speed_running: bool,
    pub provider: Option<&'static str>,

    pub pet: crate::notch::Pet,
    pub bubble: Option<Bubble>,
    pub cooldown: HashMap<String, f64>,
    pub toast: Option<Bubble>,

    pub page_h: HashMap<Tab, f32>,
    pub speed: SpeedUi,
    pub apps: AppsUi,
    pub cleaner: CleanerUi,
    pub ctl: crate::controls::Controls,
    pub chat: crate::chat::Chat,
    pub credits: crate::credits::Credits,
    pub limits: Value,
    pub lim_shown: Option<&'static str>,
    pub lim_prev: Option<(&'static str, f64)>,
    pub lim_loading_until: f64,
    pub next_lim_cycle: f64,
    pub next_tick: f64,
    pub next_place: f64,
    pub place_key: Option<(i64, i64, i64, i64, i64)>,
    pub native_handle: Option<isize>,
    pub styled: bool,
    #[cfg(windows)]
    pub edge: crate::edge::Edge,
    pub edge_on: bool,
    pub tray: Option<crate::tray::Tray>,
    pub greeted: bool,
    pub shots: Option<crate::shots::Shots>,
    pub shell: Rect,
    /// pages waiting on the left (CSS .page.left), by tab index
    pub page_left: [bool; 7],
    /// the page being drawn is the active one (inactive pages don't take clicks)
    pub live: bool,
    /// network chart scale (eased towards 1.25x the peak)
    pub net_max: f32,
}

impl App {
    pub fn new(cc: &eframe::CreationContext, q: Arc<Mutex<VecDeque<(String, Value)>>>) -> Self {
        let ctx = cc.egui_ctx.clone();
        crate::theme::install_fonts(&ctx);
        crate::notch::style(&ctx);
        crate::platform::set_main_thread();
        // macOS: the Swift helper ships in the app bundle's Resources (or next to the crate in dev)
        #[cfg(target_os = "macos")]
        crate::native_mac::init_helper(std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("../Resources"))));
        let be = Backend::new();
        let settings = be.public_settings();
        let static_ = be.stats.static_info.lock().unwrap().clone();
        let limits = be.all_limits();
        let ptr = Arc::new(Mutex::new(Pointer { ppp: 1.0, ..Default::default() }));
        crate::pointer::spawn(ptr.clone(), ctx.clone());
        be.start_work();
        be.start_loops();
        let native_handle = {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            match cc.window_handle().map(|h| h.as_raw()) {
                #[cfg(windows)]
                Ok(RawWindowHandle::Win32(h)) => Some(h.hwnd.get()),
                #[cfg(target_os = "macos")]
                Ok(RawWindowHandle::AppKit(h)) => Some(h.ns_view.as_ptr() as isize),
                _ => None,
            }
        };
        crate::idle::spawn(be.clone(), native_handle);
        let tray = crate::tray::Tray::new(&be);
        let mut app = Self {
            ctx: ctx.clone(),
            be,
            q,
            a: Anims::new(),
            ptr,
            now: 0.0,
            mode: Mode::Collapsed,
            mode_since: 0.0,
            tab: Tab::Overview,
            prev_tab: Tab::Overview,
            tab_changed: -10.0,
            pinned: false,
            hover_raw: false,
            hover_at: None,
            shell_hover: false,
            pending_mode: None,
            shrink_at: None,
            win_full: false,
            interactive: false,
            was_focused: false,
            idle: false,
            quitting: false,
            stats: Value::Null,
            latency: Value::Null,
            static_,
            settings: Value::Null,
            hist: Hist::default(),
            last_push: 0.0,
            lat_hist: VecDeque::new(),
            was_offline: false,
            hot_since: 0.0,
            user_idle: 0.0,
            ai_busy: false,
            speed_running: false,
            provider: None,
            pet: crate::notch::Pet::default(),
            bubble: None,
            cooldown: HashMap::new(),
            toast: None,
            page_h: HashMap::new(),
            speed: SpeedUi { phase: "Ready".into(), unit: "Mbps".into(), label: "Run speed test".into(), num_int: true, ..Default::default() },
            apps: AppsUi::default(),
            cleaner: CleanerUi::default(),
            ctl: Default::default(),
            chat: Default::default(),
            credits: crate::credits::Credits::load(),
            limits,
            lim_shown: None,
            lim_prev: None,
            lim_loading_until: 0.0,
            next_lim_cycle: 6.0,
            next_tick: 0.0,
            next_place: 0.0,
            place_key: None,
            native_handle,
            styled: false,
            #[cfg(windows)]
            edge: crate::edge::Edge::spawn(),
            edge_on: false,
            tray,
            greeted: false,
            shots: crate::shots::Shots::from_env(),
            shell: Rect::NOTHING,
            page_left: [false; 7],
            live: true,
            net_max: 1.0,
        };
        app.apply_settings(settings);
        app.pet.next_blink = 1.5;
        app.pet.next_fidget = 20.0;
        app
    }

    // ---------------- helpers ----------------
    pub fn set_bool(&self, k: &str) -> bool {
        match &self.settings[k] {
            Value::Bool(b) => *b,
            Value::Null => false,
            Value::Number(n) => n.as_f64().unwrap_or(0.0) != 0.0,
            Value::String(s) => !s.is_empty(),
            _ => true,
        }
    }
    pub fn is_win(&self) -> bool {
        self.settings["platform"] == "win32"
    }
    pub fn is_mac(&self) -> bool {
        self.settings["platform"] == "darwin"
    }
    pub fn has_ai(&self, p: &str) -> bool {
        self.settings["ai"][p].as_bool().unwrap_or(false)
    }
    pub fn no_ai(&self) -> bool {
        !self.has_ai("claude") && !self.has_ai("codex")
    }
    pub fn prov_list(&self) -> Vec<&'static str> {
        ["claude", "codex"].into_iter().filter(|p| self.has_ai(p)).collect()
    }
    pub fn pet_only(&self) -> bool {
        self.set_bool("hideSidesUntilHover")
    }
    /// macOS wording, like applyAi() rewriting the page's text on a Mac
    pub fn os_text(&self, s: &str) -> String {
        if self.is_mac() {
            s.replace("Windows settings", "System Settings").replace("sign in to Windows", "sign in to your Mac").replace("Switch Windows", "Switch macOS").replace("PC control", "Mac control")
        } else {
            s.to_string()
        }
    }

    pub fn say(&mut self, text: impl Into<String>, key: Option<&str>, ms: f64, cool: f64) {
        let text: String = text.into();
        if std::env::var_os("MC_DIAG_ANIM").is_some() {
            eprintln!("[say] {key:?} {text}");
        }
        if self.mode != Mode::Collapsed {
            return;
        }
        let now = now_ms();
        if let Some(k) = key {
            if let Some(t) = self.cooldown.get(k) {
                if now - t < cool {
                    return;
                }
            }
            self.cooldown.insert(k.to_string(), now);
        }
        self.bubble = Some(Bubble { text: text.into(), until: self.now + ms / 1000.0 });
    }
    pub fn hide_bubble(&mut self) {
        if let Some(b) = &mut self.bubble {
            b.until = b.until.min(self.now);
        }
    }
    pub fn toast(&mut self, text: impl Into<String>) {
        self.toast = Some(Bubble { text: text.into(), until: self.now + 2.6 });
    }

    // ---------------- modes ----------------
    pub fn set_mode(&mut self, m: Mode) {
        let m = if m == Mode::Chat && self.provider.is_none() { Mode::Panel } else { m };
        if m == self.mode && self.pending_mode.is_none() {
            return;
        }
        if m == Mode::Collapsed {
            self.pending_mode = None;
            self.apply_mode(m);
            self.shrink_at = Some(self.now + 0.75);
        } else {
            self.shrink_at = None;
            if !self.win_full {
                // Grow the window first, then expand on the next frames (like awaiting setSize + 2 rAFs)
                self.win_full = true;
                self.place();
                self.pending_mode = Some((m, 2));
            } else {
                self.pending_mode = None;
                self.apply_mode(m);
            }
        }
    }

    fn apply_mode(&mut self, m: Mode) {
        if m == self.mode {
            return;
        }
        self.mode = m;
        self.mode_since = self.now;
        self.shell_hover = false;
        if m != Mode::Collapsed {
            self.ctx.send_viewport_cmd(ViewportCommand::Focus);
            self.hide_bubble();
        }
        if m == Mode::Chat {
            self.chat.focus_at = Some(self.now + 0.35);
        }
        if m == Mode::Panel {
            self.on_tab();
        } else {
            self.apps.active = false;
            self.ctl.stop();
        }
    }

    pub fn set_tab(&mut self, t: Tab) {
        if t == self.tab {
            return;
        }
        let prev = self.tab;
        let dir = t.idx() > prev.idx();
        for (i, l) in self.page_left.iter_mut().enumerate() {
            *l = i != t.idx() && (if i == prev.idx() { dir } else { !dir });
        }
        self.prev_tab = prev;
        self.tab = t;
        self.tab_changed = self.now;
        self.on_tab();
    }

    pub fn on_tab(&mut self) {
        let t = self.tab;
        if t == Tab::Apps {
            if !self.apps.active {
                self.apps.active = true;
                self.apps.next_poll = 0.0;
            }
        } else {
            self.apps.active = false;
        }
        if t == Tab::Controls {
            self.ctl.start(self.now);
        } else {
            self.ctl.stop();
        }
        if t == Tab::Cleaner && !self.cleaner.scanned && !self.cleaner.busy {
            self.scan_cache();
        }
        if t == Tab::Credits {
            self.credits.ensure();
        }
    }

    pub fn panel_target_h(&self) -> f32 {
        let room = MAX_PANEL_H - BAR_H - TABS_H;
        let page = self.page_h.get(&self.tab).copied().unwrap_or(room);
        (BAR_H + TABS_H + page.min(room).ceil()).min(MAX_PANEL_H)
    }

    // ---------------- window ----------------
    pub fn place(&mut self) {
        let ppp = self.ctx.input(|i| i.viewport().native_pixels_per_point).unwrap_or(1.0);
        let Some((x, y, w, _h)) = crate::platform::work_area() else { return };
        let pw = (WIN_W * ppp).round() as f64;
        let ph = ((if self.win_full { WIN_H } else { SMALL_H }) * ppp).round() as f64;
        let px = (x + (w - pw) / 2.0).round();
        let py = y;
        self.ctx.send_viewport_cmd(ViewportCommand::OuterPosition(pos2((px / ppp as f64) as f32, (py / ppp as f64) as f32)));
        self.ctx.send_viewport_cmd(ViewportCommand::InnerSize(vec2((pw / ppp as f64) as f32, (ph / ppp as f64) as f32)));
        let mut p = self.ptr.lock().unwrap();
        p.origin = (px, py);
        p.ppp = ppp;
        p.ready = true;
    }

    fn check_place(&mut self) {
        let ppp = self.ctx.input(|i| i.viewport().native_pixels_per_point).unwrap_or(1.0);
        if let Some((x, y, w, h)) = crate::platform::work_area() {
            let k = (x as i64, y as i64, w as i64, h as i64, (ppp * 100.0) as i64);
            if self.place_key != Some(k) {
                self.place_key = Some(k);
                self.place();
            }
        }
        #[cfg(windows)]
        {
            let on = self.is_win() && self.set_bool("extremeMode") && !self.idle;
            let t = if on { crate::native::taskbar() } else { None };
            let place = match t {
                Some(t) if t.edge == 3 && !t.autohide => Some((t.x, t.y, t.width, ppp)),
                _ => None,
            };
            self.ptr.lock().unwrap().taskbar = place.map(|(x, y, w, _)| (x as f64, y as f64, w as f64, t.map(|t| t.height as f64).unwrap_or(0.0)));
            self.edge.send(crate::edge::Msg::Place(place));
            if on != self.edge_on {
                self.edge_on = on;
                self.edge.send(crate::edge::Msg::On(on));
            }
        }
    }

    fn set_interactive(&mut self, v: bool) {
        if v != self.interactive {
            self.interactive = v;
            self.ctx.send_viewport_cmd(ViewportCommand::MousePassthrough(!v));
        }
    }

    fn set_hover(&mut self, v: bool) {
        if v == self.hover_raw {
            return;
        }
        self.hover_raw = v;
        self.hover_at = Some((v, self.now + if v { 0.14 } else { 0.26 }));
    }

    // ---------------- settings ----------------
    pub fn apply_settings(&mut self, s: Value) {
        self.settings = s;
        self.apply_ai();
        if let Some(h) = self.settings["speedHistory"].as_array().and_then(|a| a.first()).cloned() {
            if self.speed.result.is_null() {
                self.speed.result = h;
            }
        }
        if let Some(t) = &self.tray {
            t.update(&self.be, self.idle);
        }
        self.place_key = None; // re-check the glow line right away
    }

    pub fn set_setting(&mut self, k: &str, v: Value) {
        let s = self.be.set_setting(k, v);
        self.apply_settings(s);
    }

    fn apply_ai(&mut self) {
        let list = self.prov_list();
        let pref = self.settings["chatProvider"].as_str().unwrap_or("auto").to_string();
        let prev = self.provider;
        self.provider = list.iter().copied().find(|p| *p == pref).or(list.first().copied());
        if prev != self.provider {
            self.chat.model = String::new();
        }
        if list.is_empty() && self.mode == Mode::Chat {
            self.set_mode(Mode::Collapsed);
        }
        if !self.lim_shown.is_some_and(|p| list.contains(&p)) {
            self.lim_shown = list.first().copied();
            self.lim_prev = None;
        }
    }

    // ---------------- events ----------------
    fn drain(&mut self) {
        let evs: Vec<(String, Value)> = std::mem::take(&mut *self.q.lock().unwrap()).into();
        for (ch, v) in evs {
            self.on_event(&ch, v);
        }
    }

    fn on_event(&mut self, ch: &str, v: Value) {
        match ch {
            "stats" => self.on_stats(v),
            "latency" => self.on_latency(v),
            "speed:progress" => self.on_speed(v),
            "r:speed" => self.on_speed_done(v),
            "cleaner:progress" => {
                self.cleaner.now = v["name"].as_str().unwrap_or("").to_string();
                self.cleaner.busy_id = v["id"].as_str().unwrap_or("").to_string();
            }
            "r:scan" => self.on_scan(v),
            "r:clean" => self.on_clean(v),
            "r:procs" => {
                if v["ok"] == true && self.apps.kill_arm.is_empty() {
                    self.apps.data = v["v"].clone();
                }
            }
            "r:kill" => {
                let name = v["ctx"].as_str().unwrap_or("").to_string();
                let short = name.trim_end_matches(".exe").trim_end_matches(".EXE").to_string();
                if v["ok"] == true && v["v"]["ok"] != false {
                    self.toast(format!("Ended {short}"));
                } else {
                    let m = v["v"]["message"].as_str().or(v["err"].as_str()).unwrap_or("").lines().next().unwrap_or("").to_string();
                    self.toast(format!("Could not end {name}: {m}"));
                }
                self.apps.next_poll = 0.0;
            }
            "ai" => self.on_ai(v),
            "limits" => {
                self.lim_loading_until = 0.0;
                if !v.is_null() {
                    self.limits = v;
                }
                self.check_limit_bubbles();
            }
            "claude-limits" => self.be.on_claude_limits(v),
            "codex-turn" => {
                let be = self.be.clone();
                backend::spawn(async move { be.refresh_codex_limits().await });
            }
            "settings" => self.apply_settings(v),
            "user-idle" => self.user_idle = v.as_f64().unwrap_or(0.0),
            "fullscreen" => self.on_fullscreen(v.as_bool().unwrap_or(false)),
            "open" => match v.as_str() {
                Some("chat") => self.open_mode(Mode::Chat),
                _ => self.open_mode(Mode::Panel),
            },
            "tray" => self.on_tray(v.as_str().unwrap_or("")),
            "edge-hot" => {
                #[cfg(windows)]
                self.edge.send(crate::edge::Msg::Hot(v.as_bool().unwrap_or(false)));
            }
            "r:credits" => self.credits.on_result(v, &self.ctx),
            "r:avatar" => self.credits.on_avatar(v, &self.ctx),
            "r:vol" => {
                if v["ok"] == true {
                    self.toast(format!("Volume {}%", v["v"].as_f64().unwrap_or(0.0).round()));
                } else {
                    self.toast(v["err"].as_str().unwrap_or("Could not change the volume").to_string());
                }
                self.pet.wheel_busy = false;
            }
            "r:toast" => {
                if let Some(t) = v["v"]["toast"].as_str() {
                    self.toast(t.to_string());
                } else if v["ok"] == false {
                    self.toast(v["err"].as_str().unwrap_or("Failed").to_string());
                }
            }
            c if c.starts_with("r:ctl") => self.ctl_event(c, v),
            _ => {}
        }
    }

    fn open_mode(&mut self, m: Mode) {
        if self.idle {
            return;
        }
        let m = if m == Mode::Chat && !self.be.any_ai() { Mode::Panel } else { m };
        self.ctx.send_viewport_cmd(ViewportCommand::Focus);
        self.set_mode(m);
    }

    fn on_tray(&mut self, id: &str) {
        match id {
            "open" => self.open_mode(Mode::Panel),
            "chat" => self.open_mode(Mode::Chat),
            "reload" => {
                // There is no page to reload: reset the interface state and re-read settings
                self.set_mode(Mode::Collapsed);
                self.chat = Default::default();
                self.cleaner = Default::default();
                self.apps = Default::default();
                self.ctl = Default::default();
                self.page_h.clear();
                let s = self.be.public_settings();
                self.apply_settings(s);
            }
            "quit" => self.quit(),
            k @ ("openAtLogin" | "idleOnFullscreen" | "extremeMode") => {
                let v = !self.be.on(k);
                self.set_setting(k, json!(v));
            }
            _ => {}
        }
    }

    pub fn quit(&mut self) {
        self.quitting = true;
        self.ctx.send_viewport_cmd(ViewportCommand::Close);
    }

    fn on_fullscreen(&mut self, full: bool) {
        if full == self.idle {
            return;
        }
        self.idle = full;
        self.be.idle.store(full, Ordering::SeqCst);
        if full {
            self.be.stop_work();
            self.set_mode(Mode::Collapsed);
            self.hide_bubble();
            crate::idle::hide(self.native_handle);
        } else {
            crate::idle::show(self.native_handle);
            self.be.start_work();
        }
        self.place_key = None;
        if let Some(t) = &self.tray {
            t.update(&self.be, self.idle);
        }
    }

    fn on_stats(&mut self, s: Value) {
        let total = s["mem"]["total"].as_f64().unwrap_or(1.0).max(1.0);
        let ram = s["mem"]["used"].as_f64().unwrap_or(0.0) / total * 100.0;
        let cpu = s["cpu"].as_f64().unwrap_or(0.0);
        let push = |d: &mut VecDeque<f32>, v: f64| {
            d.push_back(v as f32);
            if d.len() > HIST {
                d.pop_front();
            }
        };
        push(&mut self.hist.cpu, cpu);
        self.pet.sample_cpu(cpu);
        push(&mut self.hist.ram, ram);
        push(&mut self.hist.gpu, s["gpu"]["util"].as_f64().unwrap_or(0.0));
        push(&mut self.hist.down, s["net"]["down"].as_f64().unwrap_or(0.0));
        push(&mut self.hist.up, s["net"]["up"].as_f64().unwrap_or(0.0));
        self.last_push = self.now;
        // pet reactions
        if cpu > 90.0 {
            if self.hot_since == 0.0 {
                self.hot_since = now_ms();
            }
            if now_ms() - self.hot_since > 8000.0 {
                self.say(format!("CPU is maxed out ({}%). Check the Apps tab?", cpu.round()), Some("cpu"), 4200.0, 120000.0);
            }
        } else {
            self.hot_since = 0.0;
        }
        if ram > 92.0 {
            let (u, t) = (s["mem"]["used"].as_f64().unwrap_or(0.0), total);
            self.say(format!("RAM is almost full: {} of {}", fmt_bytes(u, 1), fmt_bytes(t, 1)), Some("ram"), 4200.0, 120000.0);
        }
        if let Some(d) = s["disks"].as_array().and_then(|a| a.iter().find(|d| d["free"].as_f64().unwrap_or(1.0) / d["total"].as_f64().unwrap_or(1.0).max(1.0) < 0.06)) {
            let msg = format!("{} is almost full ({} left). Try the Cleaner.", d["drive"].as_str().unwrap_or(""), fmt_bytes(d["free"].as_f64().unwrap_or(0.0), 1));
            self.say(msg, Some("disk"), 5000.0, 30.0 * 60000.0);
        }
        self.stats = s;
    }

    fn on_latency(&mut self, l: Value) {
        self.lat_hist.push_back(l["last"].as_f64());
        if self.lat_hist.len() > 40 {
            self.lat_hist.pop_front();
        }
        let status = l["status"].as_str().unwrap_or("").to_string();
        let ping = match l["last"].as_f64() {
            Some(v) => format!("{} ms", v.round()),
            None => "lost".into(),
        };
        if status == "offline" && !self.was_offline {
            self.was_offline = true;
            self.say("Internet connection lost. I will keep checking.", Some("off"), 5000.0, 60000.0);
        }
        if status == "online" && self.was_offline {
            self.was_offline = false;
            self.say(format!("Back online · {ping}"), Some("on"), 3500.0, 30000.0);
        }
        self.latency = l;
    }

    fn check_limit_bubbles(&mut self) {
        let now = now_ms();
        for p in ["claude", "codex"] {
            if !self.has_ai(p) {
                continue;
            }
            let wins = self.limits[p]["windows"].as_array().cloned().unwrap_or_default();
            for w in wins {
                let resets = w["resetsAt"].as_f64().unwrap_or(0.0);
                let pct = if resets > 0.0 && resets < now { 0.0 } else { w["pct"].as_f64().unwrap_or(0.0) };
                if pct >= 90.0 && resets > now {
                    let label = w["label"].as_str().unwrap_or("").to_string();
                    let name = if p == "codex" { "Codex" } else { "Claude Code" };
                    self.say(format!("{name} {label} limit at {}%. Resets {}.", pct.round(), fmt_reset(resets)), Some(&format!("lim-{p}-{label}")), 6000.0, 30.0 * 60000.0);
                }
            }
        }
    }

    // ---------------- timers ----------------
    fn timers(&mut self) {
        let now = self.now;
        if let Some((v, at)) = self.hover_at {
            if now >= at {
                self.hover_at = None;
                if v {
                    if self.hover_raw && self.mode == Mode::Collapsed {
                        self.shell_hover = true;
                        self.hide_bubble();
                    }
                } else {
                    self.shell_hover = false;
                }
            }
        }
        if let Some(t) = self.shrink_at {
            if now >= t {
                self.shrink_at = None;
                if self.mode == Mode::Collapsed && self.win_full {
                    self.win_full = false;
                    self.place();
                }
            }
        }
        if let Some((m, n)) = self.pending_mode {
            if n == 0 {
                self.pending_mode = None;
                self.apply_mode(m);
            } else {
                self.pending_mode = Some((m, n - 1));
            }
        }
        if now >= self.next_place {
            self.next_place = now + 1.5;
            self.check_place();
        }
        // pet blink / fidget
        self.pet.timers(now);
        // limits chip: alternate providers every 6 s
        if now >= self.next_lim_cycle {
            self.next_lim_cycle = now + 6.0;
            let provs = self.prov_list();
            if provs.len() >= 2 {
                let i = provs.iter().position(|p| Some(*p) == self.lim_shown).map(|i| (i + 1) % provs.len()).unwrap_or(0);
                self.lim_prev = self.lim_shown.map(|p| (p, now));
                self.lim_shown = Some(provs[i]);
            }
        }
        // apps polling (3 s) while the Apps tab is open
        if self.apps.active && self.mode == Mode::Panel && now >= self.apps.next_poll && self.apps.kill_arm.is_empty() {
            self.apps.next_poll = now + 3.0;
            backend::call("r:procs", async { tokio::task::spawn_blocking(crate::processes::list).await.map_err(|e| e.to_string()) });
        }
        self.apps.kill_arm.retain(|_, t| *t > now);
        self.ctl_timers(now);
        if !self.greeted && now > 0.9 {
            self.greeted = true;
            let greet = match self.provider {
                Some(p) => format!("Hi! Click me for Mission Control, right-click to ask {}.", if p == "codex" { "Codex" } else { "Claude" }),
                None => "Hi! Click me for Mission Control. Scroll on me to change the volume.".into(),
            };
            self.say(greet, Some("hello"), 5000.0, 1e12);
        }
    }

    fn next_deadline(&self) -> Option<f64> {
        let mut d: Vec<f64> = vec![self.next_place, self.pet.next_blink, self.pet.next_fidget, self.next_tick];
        if self.prov_list().len() >= 2 {
            d.push(self.next_lim_cycle);
        }
        if let Some((_, t)) = self.hover_at {
            d.push(t);
        }
        if let Some(t) = self.shrink_at {
            d.push(t);
        }
        if let Some(b) = &self.bubble {
            d.push(b.until);
        }
        if let Some(b) = &self.toast {
            d.push(b.until);
        }
        if self.pet.blink_until > self.now {
            d.push(self.pet.blink_until);
        }
        if self.pet.blink2_at > self.now {
            d.push(self.pet.blink2_at);
        }
        if self.pet.fidget_until > self.now {
            d.push(self.pet.fidget_until);
        }
        if self.apps.active {
            d.push(self.apps.next_poll);
        }
        d.extend(self.apps.kill_arm.values().copied());
        d.extend(self.ctl.deadlines());
        if let Some(t) = self.chat.focus_at {
            d.push(t);
        }
        d.into_iter().filter(|t| *t > self.now).fold(None, |a: Option<f64>, t| Some(a.map_or(t, |a| a.min(t))))
    }
}

impl eframe::App for App {
    fn clear_color(&self, _v: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.now = ctx.input(|i| i.time);
        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
        }
        self.drain();
        self.timers();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let t0 = std::time::Instant::now();
        self.now = ctx.input(|i| i.time);
        self.a.begin(self.now);
        if !self.styled {
            self.styled = true;
            if let Some(h) = self.native_handle {
                crate::platform::style_widget(h);
            }
            self.place();
            ctx.send_viewport_cmd(ViewportCommand::MousePassthrough(true));
        }
        // focus / blur
        let focused = ctx.input(|i| i.viewport().focused).unwrap_or(false);
        if self.was_focused && !focused && self.set_bool("collapseOnBlur") && !self.pinned && !self.cleaner.confirm {
            self.set_mode(Mode::Collapsed);
        }
        self.was_focused = focused;
        // keys
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            if self.cleaner.confirm {
                self.cleaner.confirm = false;
            } else if self.ctl.drawer.is_some() {
                self.ctl.drawer = None;
            } else {
                self.set_mode(Mode::Collapsed);
            }
        }

        // pointer: hit-testing happens on the pointer thread while clicks pass through
        let (hit, pos) = {
            let p = self.ptr.lock().unwrap();
            (p.hit, p.pos)
        };
        let over = hit || self.cleaner.confirm;
        self.set_interactive(over);
        self.set_hover(hit && self.mode == Mode::Collapsed);
        let _ = pos;

        let mut hits = Vec::new();
        self.draw(ui, &mut hits);
        if let Some(mut s) = self.shots.take() {
            s.step(&ctx, self);
            self.shots = Some(s);
        }

        {
            let mut p = self.ptr.lock().unwrap();
            p.hits = hits;
            p.pet_center = self.pet.center;
            p.think = self.pet.mood == crate::notch::Mood::Think;
        }
        if std::env::var_os("MC_DIAG_FPS").is_some() {
            eprintln!("[frame] ui={:.2}ms t={:.2} anim={} pet={} page={} next={:?} hit={} interactive={}", t0.elapsed().as_secs_f64() * 1000.0, self.now, self.a.active, self.pet.continuous, self.continuous_page(), self.next_deadline().map(|t| t - self.now), hit, self.interactive);
        }
        if self.a.active || self.pet.continuous || self.continuous_page() {
            ctx.request_repaint();
        } else if self.a.active_lo {
            ctx.request_repaint_after(std::time::Duration::from_millis(33));
        } else if let Some(t) = self.next_deadline() {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64((t - self.now).max(0.0) + 0.002));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.be.shutdown();
    }
}

impl App {
    fn draw(&mut self, ui: &mut egui::Ui, hits: &mut Vec<Rect>) {
        crate::notch::draw(self, ui, hits);
    }

    /// Something on screen animates continuously (charts scrolling, visualiser, sweep, spinners)
    fn continuous_page(&self) -> bool {
        let panel = self.mode == Mode::Panel;
        (panel && matches!(self.tab, Tab::Overview | Tab::Network))
            || (panel && self.tab == Tab::Controls && self.ctl.playing())
            || (panel && self.tab == Tab::Cleaner && (self.cleaner.busy || self.cleaner.cleaning))
            || (self.mode != Mode::Collapsed && crate::notch::sweep_active(self))
            || self.ai_busy
            || self.lim_loading_until > self.now
            || self.speed_running
            || (self.mode == Mode::Panel && self.tab == Tab::Credits && self.credits.loading())
            || (self.mode == Mode::Panel && self.tab == Tab::Controls && self.ctl.armed_any(self.now))
    }
}

// ---------------- formatting (app.js) ----------------
pub fn fmt_bytes(b: f64, d: usize) -> String {
    let u = ["B", "KB", "MB", "GB", "TB"];
    let mut b = b;
    let mut i = 0;
    while b >= 1024.0 && i < u.len() - 1 {
        b /= 1024.0;
        i += 1;
    }
    let dec = if i == 0 { 0 } else if b >= 100.0 { 0 } else { d };
    format!("{:.*} {}", dec, b, u[i])
}
pub fn fmt_rate(b: Option<f64>) -> String {
    match b {
        None => "--".into(),
        Some(b) => format!("{}/s", fmt_bytes(b, if b >= 1024.0 * 1024.0 { 1 } else { 0 })),
    }
}
pub fn fmt_mbps(m: Option<f64>) -> String {
    match m {
        None => "--".into(),
        Some(m) if m >= 100.0 => format!("{:.0}", m),
        Some(m) => format!("{:.1}", m),
    }
}
pub fn fmt_uptime(s: f64) -> String {
    let s = s as u64;
    let (d, h, m) = (s / 86400, (s % 86400) / 3600, (s % 3600) / 60);
    if d > 0 {
        format!("{d}d {h}h {m}m")
    } else {
        format!("{h}h {m}m")
    }
}
pub fn hue_of(s: &str) -> f32 {
    let mut h: u32 = 0;
    for c in s.encode_utf16() {
        h = (h * 31 + c as u32) % 360;
    }
    h as f32
}
pub fn fmt_reset(ms: f64) -> String {
    use chrono::{Local, TimeZone};
    let Some(d) = Local.timestamp_millis_opt(ms as i64).single() else { return String::new() };
    let now = Local::now();
    let time = crate::platform::fmt_time(d.clone());
    if d.date_naive() == now.date_naive() {
        return format!("today {time}");
    }
    if ms - now_ms() > 6.0 * 86400000.0 {
        return d.format("%-d %b").to_string();
    }
    format!("{} {time}", d.format("%a"))
}
pub fn short_name(n: &str) -> String {
    let re = regex::Regex::new(r"\s*\(.*\)\s*$").unwrap();
    let s = re.replace(n, "").to_string();
    if s.is_empty() {
        if n.is_empty() {
            "Device".into()
        } else {
            n.into()
        }
    } else {
        s
    }
}
