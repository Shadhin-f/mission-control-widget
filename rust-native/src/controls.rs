// Controls tab: Wi-Fi / Bluetooth / sound / mic / dark mode / wired tiles, now playing with its
// visualiser, volume / mic / brightness sliders, power plans and actions, settings shortcuts, and the
// device / network picker drawer. In sync with the system through the shared sys helpers.
use crate::app::*;
use crate::backend::{self, call, call_ctx};
use crate::draw::*;
use crate::icons;
use crate::pages::{brackets, card, card_h, card_h_right, hit, PAGE_W};
use crate::theme::*;
use eframe::egui::{self, pos2, vec2, Align2, Color32, Painter, Rect, Sense, Stroke, Ui};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub struct Controls {
    pub active: bool,
    pub st: Value,
    pub bt: Option<Vec<Value>>,
    pub wifi: Value,
    pub mons: Option<Vec<Value>>,
    pub plans: Vec<Value>,
    inflight: bool,
    hold_until: f64,
    pub drawer: Option<&'static str>,
    drawer_since: f64,
    next_state: f64,
    next_bt: f64,
    next_wifi: f64,
    next_bright: f64,
    plans_due: bool,
    dragging: Option<String>,
    last_send: HashMap<String, f64>,
    pending: Option<(String, f32, f64)>,
    busy: HashSet<String>,
    power_arm: HashMap<String, f64>,
    connecting: Option<String>,
    playing_since: f64,
}

impl Controls {
    pub fn start(&mut self, _now: f64) {
        if self.active {
            return;
        }
        self.active = true;
        self.next_state = 0.0;
        self.next_bt = 0.0;
        self.next_wifi = 0.0;
        self.next_bright = 0.0;
        self.plans_due = true;
    }
    pub fn stop(&mut self) {
        self.active = false;
    }
    pub fn playing(&self) -> bool {
        self.st["media"]["status"] == "Playing"
    }
    pub fn armed_any(&self, now: f64) -> bool {
        self.power_arm.values().any(|t| *t > now) || !self.busy.is_empty()
    }
    pub fn deadlines(&self) -> Vec<f64> {
        let mut v: Vec<f64> = self.power_arm.values().copied().collect();
        if self.active {
            v.extend([self.next_state, self.next_bt, self.next_wifi, self.next_bright]);
        }
        if let Some((_, _, t)) = &self.pending {
            v.push(*t);
        }
        v
    }
    fn out(&self) -> Option<&Value> {
        let l = self.st["audio"]["out"].as_array()?;
        l.iter().find(|d| d["isDefault"] == true).or(l.first())
    }
    fn inp(&self) -> Option<&Value> {
        let l = self.st["audio"]["in"].as_array()?;
        l.iter().find(|d| d["isDefault"] == true).or(l.first())
    }
    fn radio(&self, kind: &str) -> Option<&Value> {
        self.st["radios"].as_array()?.iter().find(|r| r["kind"] == kind)
    }
    fn set_out(&mut self, k: &str, v: Value) {
        if let Some(l) = self.st["audio"]["out"].as_array_mut() {
            let i = l.iter().position(|d| d["isDefault"] == true).unwrap_or(0);
            if let Some(d) = l.get_mut(i) {
                d[k] = v;
            }
        }
    }
    fn set_in(&mut self, k: &str, v: Value) {
        if let Some(l) = self.st["audio"]["in"].as_array_mut() {
            let i = l.iter().position(|d| d["isDefault"] == true).unwrap_or(0);
            if let Some(d) = l.get_mut(i) {
                d[k] = v;
            }
        }
    }
}

impl App {
    pub fn ctl_timers(&mut self, now: f64) {
        self.ctl.power_arm.retain(|_, t| *t > now);
        if let Some((k, v, t)) = self.ctl.pending.clone() {
            if now >= t {
                self.ctl.pending = None;
                self.apply_range(&k, v);
            }
        }
        if !self.ctl.active || self.mode != Mode::Panel {
            return;
        }
        if now >= self.ctl.next_state && !self.ctl.inflight && now >= self.ctl.hold_until {
            self.ctl.next_state = now + 1.5;
            self.ctl.inflight = true;
            call("r:ctl:state", backend::sys("state".into(), json!({})));
        }
        if now >= self.ctl.next_bt {
            self.ctl.next_bt = now + 8.0;
            call("r:ctl:bt", backend::sys("bt".into(), json!({})));
        }
        if now >= self.ctl.next_wifi {
            self.ctl.next_wifi = now + 10.0;
            call("r:ctl:wifi", backend::wifi());
        }
        if now >= self.ctl.next_bright {
            self.ctl.next_bright = now + 30.0;
            call("r:ctl:bright", backend::sys("brightness".into(), json!({})));
        }
        if self.ctl.plans_due {
            self.ctl.plans_due = false;
            call("r:ctl:plans", backend::power_plans());
        }
    }

    pub fn ctl_event(&mut self, ch: &str, v: Value) {
        let ok = v["ok"] == true;
        match ch {
            "r:ctl:state" => {
                self.ctl.inflight = false;
                if ok && self.now >= self.ctl.hold_until && self.ctl.dragging.is_none() {
                    self.ctl.st = v["v"].clone();
                }
            }
            "r:ctl:bt" => {
                if ok {
                    self.ctl.bt = Some(v["v"]["devices"].as_array().cloned().unwrap_or_default());
                }
            }
            "r:ctl:wifi" => {
                if ok {
                    self.ctl.wifi = v["v"].clone();
                }
            }
            "r:ctl:bright" => {
                self.ctl.mons = Some(if ok { v["v"]["monitors"].as_array().cloned().unwrap_or_default() } else { vec![] });
            }
            "r:ctl:plans" => {
                if ok {
                    self.ctl.plans = v["v"].as_array().cloned().unwrap_or_default();
                }
            }
            "r:ctl:toggle" => {
                let k = v["ctx"].as_str().unwrap_or("").to_string();
                self.ctl.busy.remove(&k);
                if !ok {
                    self.toast(v["err"].as_str().unwrap_or("Failed").to_string());
                }
            }
            "r:ctl:default" => {
                if ok {
                    let name = v["ctx"].as_str().unwrap_or("").to_string();
                    self.toast(format!("Switched to {}", short_name(&name)));
                } else {
                    self.toast(v["err"].as_str().unwrap_or("Failed").to_string());
                }
                self.ctl.next_state = 0.0;
                self.ctl.hold_until = 0.0;
            }
            "r:ctl:range" => {
                if !ok {
                    self.toast(format!("Could not change it: {}", v["err"].as_str().unwrap_or("")));
                }
            }
            "r:ctl:media" => {
                if !ok {
                    self.toast(v["err"].as_str().unwrap_or("Failed").to_string());
                }
                self.ctl.next_state = self.now + 0.3;
            }
            "r:ctl:plan" => {
                if !ok {
                    self.toast(v["err"].as_str().unwrap_or("Failed").to_string());
                }
                call("r:ctl:plans", backend::power_plans());
            }
            "r:ctl:wifi-connect" => {
                let ssid = v["ctx"].as_str().unwrap_or("").to_string();
                self.ctl.connecting = None;
                if ok && v["v"]["ok"] != false {
                    self.toast(format!("Connecting to {ssid}"));
                } else {
                    let m = v["v"]["message"].as_str().or(v["err"].as_str()).unwrap_or("").to_string();
                    self.toast(format!("Could not connect: {m}"));
                }
                self.ctl.next_wifi = self.now + 3.0;
            }
            "r:ctl:wifi-off" => {
                self.toast("Disconnected");
                self.ctl.next_wifi = 0.0;
            }
            "r:ctl:location" => {
                let t = if ok && v["v"]["ok"] == true { "Wi-Fi access granted" } else { "Allow Mission Control Helper in Location settings" };
                self.toast(t);
                self.ctl.next_wifi = 0.0;
            }
            _ => {}
        }
    }

    fn ctl_toggle(&mut self, kind: &str) {
        if self.ctl.st.is_null() {
            return;
        }
        self.ctl.hold_until = self.now + 0.8;
        let fut: Option<std::pin::Pin<Box<dyn std::future::Future<Output = Result<Value, String>> + Send>>> = match kind {
            "wifi" | "bt" => {
                let rk = if kind == "wifi" { "WiFi" } else { "Bluetooth" };
                let Some(r) = self.ctl.radio(rk) else {
                    self.toast(if kind == "wifi" { "No Wi-Fi adapter" } else { "No Bluetooth adapter" });
                    return;
                };
                let on = r["state"] != "On";
                if let Some(rs) = self.ctl.st["radios"].as_array_mut() {
                    for x in rs.iter_mut().filter(|x| x["kind"] == rk) {
                        x["state"] = json!(if on { "On" } else { "Off" });
                    }
                }
                Some(Box::pin(backend::sys("setRadio".into(), json!({ "kind": rk, "on": on }))))
            }
            "sound" => {
                let Some(o) = self.ctl.out().cloned() else { return };
                let m = !o["muted"].as_bool().unwrap_or(false);
                self.ctl.set_out("muted", json!(m));
                Some(Box::pin(backend::sys("setMute".into(), json!({ "id": o["id"], "muted": m }))))
            }
            "mic" => {
                let Some(i) = self.ctl.inp().cloned() else { return };
                let m = !i["muted"].as_bool().unwrap_or(false);
                self.ctl.set_in("muted", json!(m));
                Some(Box::pin(backend::sys("setMute".into(), json!({ "id": i["id"], "muted": m }))))
            }
            "dark" => {
                let d = !self.ctl.st["dark"].as_bool().unwrap_or(false);
                self.ctl.st["dark"] = json!(d);
                Some(Box::pin(backend::sys("setDark".into(), json!({ "dark": d }))))
            }
            _ => None,
        };
        if let Some(f) = fut {
            self.ctl.busy.insert(kind.to_string());
            call_ctx("r:ctl:toggle", json!(kind), f);
        }
    }

    /// sendRange: throttled while dragging (60 ms audio, 250 ms brightness), always on release.
    fn send_range(&mut self, key: &str, v: f32, fin: bool) {
        let gap = if key.starts_with('b') { 0.25 } else { 0.06 };
        let last = self.ctl.last_send.get(key).copied().unwrap_or(0.0);
        if fin || self.now - last > gap {
            self.ctl.pending = None;
            self.ctl.last_send.insert(key.to_string(), self.now);
            self.apply_range(key, v);
        } else {
            self.ctl.pending = Some((key.to_string(), v, self.now + 0.08));
        }
    }

    fn apply_range(&mut self, key: &str, v: f32) {
        self.ctl.hold_until = self.now + 0.6; // don't let a poll snap the slider back mid-drag
        let pct = (v * 100.0).round() as i64;
        if key == "vol" {
            if let Some(o) = self.ctl.out().cloned() {
                self.ctl.set_out("volume", json!(pct));
                self.ctl.set_out("muted", json!(false));
                call("r:ctl:range", backend::sys("setVolume".into(), json!({ "id": o["id"], "value": pct })));
            }
        } else if key == "mic" {
            if let Some(i) = self.ctl.inp().cloned() {
                self.ctl.set_in("volume", json!(pct));
                self.ctl.set_in("muted", json!(false));
                call("r:ctl:range", backend::sys("setVolume".into(), json!({ "id": i["id"], "value": pct })));
            }
        } else if let Some(idx) = key.strip_prefix('b') {
            if let Some(mons) = &mut self.ctl.mons {
                if let Some(m) = mons.iter_mut().find(|m| m["index"].to_string() == idx) {
                    let (lo, hi) = (m["min"].as_f64().unwrap_or(0.0), m["max"].as_f64().unwrap_or(100.0));
                    let val = (lo + v as f64 * (hi - lo)).round();
                    m["cur"] = json!(val);
                    call("r:ctl:range", backend::sys("setBrightness".into(), json!({ "index": m["index"], "value": val })));
                }
            }
        }
    }

    fn open_drawer(&mut self, kind: &'static str) {
        self.ctl.drawer = Some(kind);
        self.ctl.drawer_since = self.now;
        if kind == "bt" {
            self.ctl.next_bt = 0.0;
        }
        if kind == "wifi" {
            self.ctl.next_wifi = 0.0;
        }
    }
}

const TILE_INFO: [(&str, &str, Color32); 6] = [
    ("wifi", "Wi-Fi", CYAN),
    ("bt", "Bluetooth", rgb(0x5b, 0x8c, 0xff)),
    ("sound", "Sound", VIOLET),
    ("mic", "Microphone", PINK),
    ("dark", "Dark mode", rgb(0xff, 0xd1, 0x66)),
    ("net", "Ethernet", GREEN),
];

pub fn page(app: &mut App, ui: &mut Ui) {
    let col_w = (PAGE_W - 12.0) / 2.0;
    let mons = app.ctl.mons.clone().unwrap_or_default();
    let bright_n = mons.len().max(1);
    let sl_h = 36.0;
    let sliders_h = 14.0 + 16.0 + 2.0 + (2 + bright_n) as f32 * sl_h + (1 + bright_n) as f32 * 14.0;
    let power_h = 12.0 + 14.0 + 8.0 + 10.0 + 36.0 + 10.0 + 54.0 + 12.0 + 2.0;
    let quick_min = 12.0 + 14.0 + 8.0 + 56.0 * 2.0 + 8.0 + 12.0;
    let right_h = sliders_h + 12.0 + power_h + 12.0 + quick_min;
    let tiles_h = 64.0 * 3.0 + 18.0;
    let media_min = 12.0 + 14.0 + 12.0 + 34.0 + 12.0 + 42.0 + 12.0 + 84.0;
    let left_h: f32 = tiles_h + 12.0 + media_min;
    let h = left_h.max(right_h);
    let (area, _) = ui.allocate_exact_size(vec2(PAGE_W, h), Sense::hover());
    let drawer_k = app.a.v("drawer", if app.ctl.drawer.is_some() { 1.0 } else { 0.0 }, 0.55, SPRING);
    let was_live = app.live;
    if drawer_k > 0.98 {
        app.live = false;
    }
    let p = ui.painter().clone();
    let left = Rect::from_min_size(area.min, vec2(col_w, h));
    let right = Rect::from_min_size(pos2(area.left() + col_w + 12.0, area.top()), vec2(col_w, h));
    if drawer_k < 0.999 {
        // tiles
        let tw = (col_w - 9.0) / 2.0;
        for (i, (k, name, tc)) in TILE_INFO.into_iter().enumerate() {
            let r = Rect::from_min_size(pos2(left.left() + (i % 2) as f32 * (tw + 9.0), left.top() + (i / 2) as f32 * 73.0), vec2(tw, 64.0));
            tile(app, ui, &p, r, k, name, tc);
        }
        media(app, ui, &p, Rect::from_min_max(pos2(left.left(), left.top() + tiles_h + 12.0), left.max));
        let sr = Rect::from_min_size(right.min, vec2(col_w, sliders_h));
        sliders(app, ui, &p, sr, &mons);
        let pr = Rect::from_min_size(pos2(right.left(), sr.bottom() + 12.0), vec2(col_w, power_h));
        power(app, ui, &p, pr);
        let qr = Rect::from_min_max(pos2(right.left(), pr.bottom() + 12.0), right.max);
        quick(app, ui, &p, qr);
    }
    app.live = was_live;
    if drawer_k > 0.001 {
        let full = Rect::from_min_max(area.min - vec2(18.0, 14.0), area.max + vec2(18.0, 18.0));
        let x = full.left() + full.width() * 1.04 * (1.0 - drawer_k);
        let dr = Rect::from_min_size(pos2(x, full.top()), full.size());
        drawer(app, ui, dr);
    }
}

fn tile(app: &mut App, ui: &Ui, p: &Painter, r: Rect, k: &'static str, name: &str, tc: Color32) {
    let c = &app.ctl;
    let st = &c.st;
    let (on, sub, muted) = match k {
        "wifi" => {
            let w = c.radio("WiFi");
            let on = w.is_some_and(|w| w["state"] == "On");
            let ssid = (c.wifi["current"]["state"] == "connected").then(|| c.wifi["current"]["ssid"].as_str().unwrap_or("").to_string());
            (on, if st.is_null() { "--".into() } else if w.is_none() { "No adapter".into() } else if on { ssid.unwrap_or("Not connected".into()) } else { "Off".into() }, false)
        }
        "bt" => {
            let b = c.radio("Bluetooth");
            let on = b.is_some_and(|b| b["state"] == "On");
            let conn = c.bt.as_ref().map(|l| l.iter().filter(|d| d["connected"] == true).count()).unwrap_or(0);
            (on, if st.is_null() { "--".into() } else if b.is_none() { "No adapter".into() } else if on { if conn > 0 { format!("{conn} connected") } else { "On".into() } } else { "Off".into() }, false)
        }
        "sound" => match c.out() {
            Some(o) => {
                let m = o["muted"].as_bool().unwrap_or(false);
                (!m, if m { "Muted".into() } else { format!("{}% · {}", o["volume"].as_f64().unwrap_or(0.0).round(), short_name(o["name"].as_str().unwrap_or(""))) }, m)
            }
            None => (false, if st.is_null() { "--".into() } else { "No output device".into() }, false),
        },
        "mic" => match c.inp() {
            Some(i) => {
                let m = i["muted"].as_bool().unwrap_or(false);
                (!m, if m { "Muted".into() } else { format!("On · {}%", i["volume"].as_f64().unwrap_or(0.0).round()) }, m)
            }
            None => (false, if st.is_null() { "--".into() } else { "No microphone".into() }, false),
        },
        "dark" => {
            let d = st["dark"].as_bool().unwrap_or(false);
            (d, if st.is_null() { "--".into() } else if d { "On".into() } else { "Off".into() }, false)
        }
        _ => {
            let a = &app.stats["net"]["adapter"];
            let online = app.latency["status"] == "online";
            if a.is_object() && a["wifi"] != true {
                let bps = a["linkBps"].as_f64().unwrap_or(0.0);
                let link = if bps >= 1e9 { format!("{:.0} Gbps", bps / 1e9) } else if bps > 0.0 { format!("{} Mbps", (bps / 1e6).round()) } else { a["name"].as_str().unwrap_or("").to_string() };
                (online, format!("{link} · {}", if online { "Online" } else { app.latency["status"].as_str().unwrap_or("--") }), false)
            } else {
                (false, if st.is_null() { "--".into() } else { "Not connected".into() }, false)
            }
        }
    };
    let has_chev = matches!(k, "wifi" | "bt" | "sound" | "mic");
    let chev_r = Rect::from_min_size(pos2(r.right() - 10.0 - 24.0, r.center().y - 20.0), vec2(24.0, 40.0));
    let resp = hit(app, ui, r, ("tile", k), Sense::click());
    let chev = if has_chev { Some(hit(app, ui, chev_r, ("chev", k), Sense::click())) } else { None };
    let hov = resp.hovered() || chev.as_ref().is_some_and(|c| c.hovered());
    let hk = app.a.v(("tile_h", k), if hov { 1.0 } else { 0.0 }, 0.35, SPRING);
    let ok = app.a.v(("tile_on", k), if on { 1.0 } else { 0.0 }, 0.35, EASE);
    let wash = app.a.v(("tile_wash", k), if on { 1.0 } else { 0.0 }, 0.45, EASE);
    let press = app.a.v(("tile_press", k), if resp.is_pointer_button_down_on() { 1.0 } else { 0.0 }, 0.35, SPRING);
    let s = 1.0 - 0.03 * press;
    let r = Rect::from_center_size(r.center() - vec2(0.0, hk), r.size() * s);
    if ok > 0.01 {
        shadow(p, r, 14.0, vec2(0.0, 0.0), 22.0, -6.0_f32.max(0.0), fade(tc, 0.6 * ok * 0.5));
    }
    let border = lerp_c(lerp_c(LINE, LINE2, hk), fade(tc, 0.55), ok);
    if ok > 0.01 {
        fill(p, r, 14.0, rgb(7, 11, 21)); // keep the glow outside the tile, like CSS box-shadow
    }
    boxed(p, r, 14.0, BG_CARD, Color32::TRANSPARENT);
    if wash > 0.01 {
        // radial-gradient(120% 140% at 0% 0%, tc 28%, transparent 60%)
        let (w, h) = (r.width(), r.height());
        let o = r.min;
        fill_with(p, r, [14.0; 4], 4, |q| {
            let dx = (q.x - o.x) / (1.2 * w);
            let dy = (q.y - o.y) / (1.4 * h);
            let d = (dx * dx + dy * dy).sqrt() / 0.6;
            fade(tc, 0.28 * (1.0 - d).max(0.0) * wash)
        });
    }
    p.rect_stroke(r, cr(14.0), Stroke::new(1.0, border), egui::StrokeKind::Inside);
    if ok > 0.01 {
        p.rect_stroke(r.shrink(1.0), cr(13.0), Stroke::new(1.0, fade(tc, 0.18 * ok)), egui::StrokeKind::Inside);
    }
    // icon tile
    let busy = app.ctl.busy.contains(k);
    let pulse_k = if busy { let ph = ((app.now / 0.9) % 1.0) as f32; 1.0 - (ph * 2.0 - 1.0).abs() } else { 0.0 };
    let rot = app.a.v(("tile_rot", k), if on { 1.0 } else { 0.0 }, 0.45, SPRING);
    let ic = Rect::from_min_size(pos2(r.left() + 11.0, r.center().y - 18.0), vec2(36.0, 36.0));
    let ic = Rect::from_center_size(ic.center(), ic.size() * (1.0 + 0.04 * rot) * (1.0 - 0.12 * pulse_k));
    if ok > 0.01 {
        glow(p, ic, 11.0, 16.0, fade(tc, 0.7 * ok));
    }
    let ib = lerp_c(WHITE05, tc, ok);
    rotated_rrect(p, ic, 11.0, -6f32.to_radians() * rot, ib);
    let stroke_c = app.a.color(("tile_ic", k), if on { rgb(0x04, 0x12, 0x1a) } else { DIM }, 0.35);
    let stroke_c = fade(stroke_c, 1.0 - 0.4 * pulse_k);
    let svg = Rect::from_center_size(ic.center(), vec2(19.0, 19.0));
    match k {
        "wifi" => icon(p, svg, icons::WIFI_TILE, 1.9, stroke_c),
        "bt" => icon(p, svg, icons::BT, 1.9, stroke_c),
        "sound" => {
            icon(p, svg, icons::SOUND_BODY, 1.9, stroke_c);
            icon(p, svg, icons::SOUND_WAVE, 1.9, fade(stroke_c, if muted { 0.25 } else { 1.0 }));
            let sl = app.a.v(("slash", k), if muted { 1.0 } else { 0.0 }, 0.3, EASE);
            icon(p, svg, icons::SOUND_SLASH, 1.9, fade(RED, sl));
        }
        "mic" => {
            icon(p, svg, icons::MIC_BODY, 1.9, stroke_c);
            let sl = app.a.v(("slash", k), if muted { 1.0 } else { 0.0 }, 0.3, EASE);
            icon(p, svg, icons::MIC_SLASH, 1.9, fade(RED, sl));
        }
        "dark" => icon(p, svg, icons::MOON, 1.9, stroke_c),
        _ => icon(p, svg, icons::ETHERNET, 1.9, stroke_c),
    }
    // text
    let tx = ic.right() + 10.0;
    let tw = (if has_chev { chev_r.left() - 10.0 } else { r.right() - 10.0 }) - tx;
    text_fit(p, pos2(tx, r.center().y - 1.0), Align2::LEFT_BOTTOM, name, font6(12.5), TEXT, 0.0, tw);
    let sub_c = lerp_c(DIM, mix_white(tc, 0.75), ok);
    text_fit(p, pos2(tx, r.center().y + 1.0), Align2::LEFT_TOP, &sub, font(10.5), sub_c, 0.0, tw);
    if let Some(cr_) = chev {
        let ch = app.a.v(("chev_h", k), if cr_.hovered() { 1.0 } else { 0.0 }, 0.3, SPRING);
        let cr2 = chev_r.translate(vec2(2.0 * ch, -hk));
        fill(p, cr2, 8.0, fade(rgba(255, 255, 255, 0.07), ch));
        icon(p, Rect::from_center_size(cr2.center(), vec2(16.0, 16.0)), icons::CHEV_R, 1.8, lerp_c(DIM, TEXT, ch));
        let tip = match k {
            "wifi" => "Networks",
            "bt" => "Devices",
            "sound" => "Output device",
            _ => "Input device",
        };
        if cr_.on_hover_text(tip).clicked() {
            let d: &'static str = match k {
                "wifi" => "wifi",
                "bt" => "bt",
                "sound" => "out",
                _ => "in",
            };
            app.open_drawer(d);
            return;
        }
    }
    if resp.clicked() {
        if k == "net" {
            backend::open_settings("network");
        } else {
            app.ctl_toggle(k);
        }
    }
}

fn rotated_rrect(p: &Painter, r: Rect, rad: f32, ang: f32, c: Color32) {
    if ang.abs() < 0.001 {
        fill(p, r, rad, c);
        return;
    }
    let center = r.center();
    let (s, co) = (ang.sin(), ang.cos());
    let mut pts = Vec::new();
    use std::f32::consts::{FRAC_PI_2, PI};
    let rr = rad.min(r.width() / 2.0);
    for (cx, cy, a0) in [(r.left() + rr, r.top() + rr, PI), (r.right() - rr, r.top() + rr, PI * 1.5), (r.right() - rr, r.bottom() - rr, 0.0), (r.left() + rr, r.bottom() - rr, FRAC_PI_2)] {
        pts.extend(arc_pts(pos2(cx, cy), rr, a0, a0 + FRAC_PI_2));
    }
    let pts: Vec<_> = pts.into_iter().map(|q| {
        let d = q - center;
        center + vec2(d.x * co - d.y * s, d.x * s + d.y * co)
    }).collect();
    fill_star(p, &pts, c);
}

fn media(app: &mut App, ui: &Ui, p: &Painter, r: Rect) {
    card(p, r);
    let m = app.ctl.st["media"].clone();
    let none = m.is_null() || !m.is_object();
    let playing = !none && m["status"] == "Playing";
    let inner = r.shrink2(vec2(14.0, 12.0));
    let badge_c = if playing { CYAN } else { DIM };
    let br = text(p, pos2(inner.left(), inner.top()), Align2::LEFT_TOP, "NOW PLAYING", font(9.5), badge_c, 1.71);
    let app_name = if none { String::new() } else { m["app"].as_str().unwrap_or("").trim_end_matches(".exe").split('!').next().unwrap_or("").rsplit('.').next().unwrap_or("").to_string() };
    text(p, pos2(inner.right(), br.center().y), Align2::RIGHT_CENTER, &app_name, font(10.0), DIM2, 0.0);
    let title = if none { "Nothing playing".to_string() } else { m["title"].as_str().filter(|s| !s.is_empty()).unwrap_or("Unknown title").to_string() };
    let artist = if none { "Play something in any app".to_string() } else { m["artist"].as_str().filter(|s| !s.is_empty()).map(|s| s.to_string()).unwrap_or(if app_name.is_empty() { "Unknown artist".into() } else { app_name.clone() }) };
    let tr = text_fit(p, pos2(inner.left(), br.bottom() + 12.0), Align2::LEFT_TOP, &title, font6(14.0), TEXT, 0.0, inner.width());
    let ar = text_fit(p, pos2(inner.left(), tr.bottom() + 2.0), Align2::LEFT_TOP, &artist, font(11.0), DIM, 0.0, inner.width());
    // controls
    let cy = ar.bottom() + 12.0 + 21.0;
    let mut q = p.clone();
    if none {
        q.set_opacity(0.35 * p.opacity());
    }
    let mut x = inner.left();
    for (k, d) in [("prev", 34.0), ("toggle", 42.0), ("next", 34.0)] {
        let b = Rect::from_min_size(pos2(x, cy - d / 2.0), vec2(d, d));
        x += d + 10.0;
        let resp = hit(app, ui, b, ("media", k), if none { Sense::hover() } else { Sense::click() });
        let hk = app.a.v(("m_h", k), if resp.hovered() && !none { 1.0 } else { 0.0 }, 0.2, EASE);
        let pk = app.a.v(("m_p", k), if resp.is_pointer_button_down_on() { 1.0 } else { 0.0 }, 0.3, SPRING);
        let b = Rect::from_center_size(b.center(), b.size() * (1.0 - 0.12 * pk));
        let ic = Rect::from_center_size(b.center(), vec2(16.0, 16.0));
        if k == "toggle" {
            glow(&q, b, b.width() / 2.0, 18.0, rgba(62, 243, 255, 0.3));
            diag(&q, b, b.width() / 2.0, CYAN, VIOLET);
            if playing {
                icon(&q, ic, icons::PAUSE, 2.6, INK);
            } else {
                icon(&q, ic, icons::PLAY, 2.0, INK);
            }
        } else {
            q.circle(b.center(), b.width() / 2.0 - 0.5, fade(rgba(255, 255, 255, 0.06), hk), Stroke::new(1.0, lerp_c(LINE2, rgba(62, 243, 255, 0.4), hk)));
            icon(&q, ic, if k == "prev" { icons::PREV } else { icons::NEXT }, 2.0, TEXT);
        }
        if resp.clicked() && !none {
            call("r:ctl:media", backend::sys("media".into(), json!({ "action": k })));
        }
    }
    // visualiser: 30 bars, each with its own rhythm; only animates while playing and visible
    let viz = Rect::from_min_max(pos2(r.left(), r.bottom() - 84.0), pos2(r.right(), r.bottom()));
    let vp = p.clone().with_clip_rect(viz.intersect(p.clip_rect()));
    let bw = (viz.width() - 28.0 - 3.0 * 29.0) / 30.0;
    let live = playing && app.mode == Mode::Panel && app.tab == Tab::Controls;
    if live && app.ctl.playing_since == 0.0 {
        app.ctl.playing_since = app.now;
    }
    if !live {
        app.ctl.playing_since = 0.0;
    }
    let vo = app.a.v("viz_op", if live { 0.9 } else { 0.35 }, 0.4, EASE);
    for i in 0..30 {
        let fi = i as f32;
        let hi = 0.35 + (fi * 1.7).sin().abs() * 0.6;
        let d = 0.45 + ((i * 37) % 50) as f32 / 100.0;
        let dl = -(((i * 13) % 10) as f32) / 10.0;
        let lo = 0.06 + ((i * 7) % 5) as f32 / 40.0;
        let target = if live {
            let u = ((app.now - app.ctl.playing_since) as f32 - dl) / d;
            let cyc = u.floor() as i64;
            let fr = u - u.floor();
            let e = EASE_IN_OUT.at(if cyc % 2 == 0 { fr } else { 1.0 - fr });
            lo + (hi - lo) * e
        } else {
            0.06
        };
        let sy = if live { target } else { app.a.v(("viz", i), 0.06, 0.6, SMOOTH) };
        if live {
            app.a.snap(("viz", i), sy);
        }
        let x = viz.left() + 14.0 + fi * (bw + 3.0);
        // mask: fade the outer 12% on each side
        let u = (x + bw / 2.0 - viz.left()) / viz.width();
        let mask = (u / 0.12).min((1.0 - u) / 0.12).clamp(0.0, 1.0);
        let h = 84.0 * sy;
        let b = Rect::from_min_max(pos2(x, viz.bottom() - h), pos2(x + bw, viz.bottom()));
        let a = vo * mask;
        vgrad(&vp, b, [3.0, 3.0, 0.0, 0.0], &[(0.0, fade(rgba(62, 243, 255, 0.9), a)), (1.0, fade(rgba(155, 123, 255, 0.9), a))]);
    }
}

fn sliders(app: &mut App, ui: &Ui, p: &Painter, r: Rect, mons: &[Value]) {
    card(p, r);
    let inner = Rect::from_min_max(r.min + vec2(14.0, 14.0), r.max - vec2(14.0, 16.0));
    let mut y = inner.top();
    let o = app.ctl.out().cloned();
    let i = app.ctl.inp().cloned();
    let mut rows: Vec<(String, &'static str, String, Option<f32>, bool, Option<&'static str>, (Color32, Color32))> = vec![];
    rows.push(("vol".into(), "Volume", o.as_ref().map(|d| short_name(d["name"].as_str().unwrap_or(""))).unwrap_or("--".into()), o.as_ref().map(|d| d["volume"].as_f64().unwrap_or(0.0) as f32 / 100.0), o.as_ref().is_some_and(|d| d["muted"] == true), Some("out"), (CYAN, VIOLET)));
    rows.push(("mic".into(), "Mic level", i.as_ref().map(|d| short_name(d["name"].as_str().unwrap_or(""))).unwrap_or("--".into()), i.as_ref().map(|d| d["volume"].as_f64().unwrap_or(0.0) as f32 / 100.0), i.as_ref().is_some_and(|d| d["muted"] == true), Some("in"), (PINK, VIOLET)));
    for (n, m) in mons.iter().enumerate() {
        let (lo, hi) = (m["min"].as_f64().unwrap_or(0.0), m["max"].as_f64().unwrap_or(100.0));
        let pct = if hi > lo { ((m["cur"].as_f64().unwrap_or(0.0) - lo) / (hi - lo)) as f32 } else { 0.0 };
        let name = if mons.len() > 1 { format!("Display {}", n + 1) } else { short_name(m["name"].as_str().unwrap_or("")) };
        rows.push((format!("b{}", m["index"]), "Brightness", name, Some(pct), false, None, (rgb(0xff, 0xd1, 0x66), ORANGE)));
    }
    for (key, label, dev, val, muted, drawer, (c1, c2)) in rows {
        // header
        let hr = Rect::from_min_size(pos2(inner.left(), y), vec2(inner.width(), 16.0));
        let dragging = app.ctl.dragging.as_deref() == Some(key.as_str());
        let disp = app.a.v(("rng", &key), val.unwrap_or(0.0), 0.45, SMOOTH);
        let v_now = if dragging { disp } else { val.unwrap_or(0.0) };
        let vtxt = if val.is_none() { "--".into() } else if muted && !dragging { "Muted".into() } else { format!("{}%", (v_now * 100.0).round()) };
        let vr = text(p, pos2(hr.right(), hr.bottom()), Align2::RIGHT_BOTTOM, &vtxt, font6(13.0), TEXT, 0.0);
        let name_r = Rect::from_min_max(hr.min, pos2(vr.left() - 10.0, hr.bottom()));
        let nresp = drawer.map(|_| hit(app, ui, name_r, ("slname", &key), Sense::click()));
        let nh = nresp.as_ref().is_some_and(|r| r.hovered());
        let er = text(p, pos2(hr.left(), hr.bottom() - 1.0), Align2::LEFT_BOTTOM, &label.to_uppercase(), font(9.5), TEXT, 1.52);
        let suffix = if drawer.is_some() { " ▾" } else { "" };
        text_fit(p, pos2(er.right() + 6.0, hr.bottom()), Align2::LEFT_BOTTOM, &format!("{dev}{suffix}"), font(11.0), if nh { TEXT } else { DIM }, 0.0, name_r.right() - er.right() - 6.0);
        if let (Some(resp), Some(d)) = (nresp, drawer) {
            if resp.on_hover_text(if d == "out" { "Choose output device" } else { "Choose input device" }).clicked() {
                app.open_drawer(d);
            }
        }
        y = hr.bottom() + 8.0;
        let track = Rect::from_min_size(pos2(inner.left(), y), vec2(inner.width(), 12.0));
        range(app, ui, p, track, &key, val, muted, c1, c2);
        y = track.bottom() + 14.0;
    }
    if mons.is_empty() && app.ctl.mons.is_some() {
        let er = text(p, pos2(inner.left(), y), Align2::LEFT_TOP, "BRIGHTNESS", font(9.5), TEXT, 1.52);
        let mut j = job("This display does not expose brightness control. Use its buttons or Display settings.", font(11.0), DIM2, 0.0);
        j.wrap.max_width = inner.width();
        let g = p.layout_job(j);
        p.galley(pos2(inner.left(), er.bottom() + 8.0), g, DIM2);
    }
}

fn range(app: &mut App, ui: &Ui, p: &Painter, track: Rect, key: &str, val: Option<f32>, off: bool, c1: Color32, c2: Color32) {
    let hit_r = track.expand2(vec2(9.0, 4.0));
    let resp = hit(app, ui, hit_r, ("range", key), Sense::click_and_drag());
    let dragging = app.ctl.dragging.as_deref() == Some(key);
    let mut v = if dragging { app.a.v(("rng", key), 0.0, 0.0, Ease::Linear) } else { app.a.v(("rng", key), val.unwrap_or(0.0), 0.45, SMOOTH) };
    if val.is_some() {
        if resp.drag_started() || resp.is_pointer_button_down_on() && !dragging {
            app.ctl.dragging = Some(key.to_string());
        }
        if let (true, Some(pos)) = (app.ctl.dragging.as_deref() == Some(key), resp.interact_pointer_pos()) {
            v = ((pos.x - track.left()) / track.width()).clamp(0.0, 1.0);
            app.a.snap(("rng", key), v);
            let fin = resp.drag_stopped() || resp.clicked();
            app.send_range(key, v, fin);
            if fin {
                app.ctl.dragging = None;
            }
        }
        if app.ctl.dragging.as_deref() == Some(key) && !ui.input(|i| i.pointer.any_down()) {
            app.ctl.dragging = None;
            app.send_range(key, v, true);
        }
        if resp.hovered() {
            let dy = wheel(ui);
            if dy != 0.0 {
                v = (v + if dy > 0.0 { 0.02 } else { -0.02 }).clamp(0.0, 1.0);
                app.a.snap(("rng", key), v);
                app.send_range(key, v, false);
            }
        }
    }
    fill(p, track, 6.0, rgba(255, 255, 255, 0.06));
    let (a, b) = if off { (gray(c1), gray(c2)) } else { (c1, c2) };
    if v > 0.002 {
        let fr = Rect::from_min_size(track.min, vec2(track.width() * v, track.height()));
        glow(p, fr, 6.0, 12.0, fade(a, 0.45));
        // the gradient spans the full track and is scaled with the fill (transform: scaleX)
        hgrad(p, fr, 6.0, &[(0.0, a), (1.0, b)]);
    }
    let hk = app.a.v(("knob_h", key), if resp.hovered() || dragging { 1.0 } else { 0.0 }, 0.45, SMOOTH);
    let ks = 0.85 + 0.2 * hk;
    let kc = pos2(track.left() + track.width() * v, track.center().y);
    p.circle_filled(kc, (9.0 + 4.0) * ks, fade(c1, 0.3));
    p.circle_filled(kc, 9.0 * ks, rgb(0xe9, 0xf6, 0xff));
}

fn gray(c: Color32) -> Color32 {
    let l = (0.3 * c.r() as f32 + 0.59 * c.g() as f32 + 0.11 * c.b() as f32) * 0.6;
    Color32::from_rgba_premultiplied(l as u8, l as u8, l as u8, c.a())
}

fn power(app: &mut App, ui: &Ui, p: &Painter, r: Rect) {
    card(p, r);
    let h = card_h(p, r, "Power");
    let plans = app.ctl.plans.clone();
    let active = plans.iter().find(|x| x["active"] == true).and_then(|x| x["name"].as_str()).unwrap_or("--").to_string();
    card_h_right(p, h, &active, DIM);
    let seg = Rect::from_min_size(pos2(r.left() + 14.0, h.bottom() + 8.0 + 10.0), vec2(r.width() - 28.0, 36.0));
    boxed(p, seg, 10.0, rgba(255, 255, 255, 0.04), LINE);
    if plans.is_empty() {
        text(p, seg.center(), Align2::CENTER_CENTER, "No power plans found", font(11.0), DIM, 0.0);
    } else {
        let bw = (seg.width() - 6.0) / plans.len() as f32;
        if let Some(i) = plans.iter().position(|x| x["active"] == true) {
            let x = app.a.v("seg_x", seg.left() + 3.0 + i as f32 * bw, 0.5, SPRING);
            let w = app.a.v("seg_w", bw, 0.5, SPRING);
            let ink = Rect::from_min_size(pos2(x, seg.top() + 3.0), vec2(w, seg.height() - 6.0));
            glow(p, ink, 8.0, 14.0, rgba(62, 243, 255, 0.35));
            hgrad(p, ink, 8.0, &[(0.0, CYAN), (1.0, GREEN)]);
        }
        for (i, pl) in plans.iter().enumerate() {
            let b = Rect::from_min_size(pos2(seg.left() + 3.0 + i as f32 * bw, seg.top() + 3.0), vec2(bw, seg.height() - 6.0));
            let on = pl["active"] == true;
            let name = pl["name"].as_str().unwrap_or("").to_string();
            let resp = hit(app, ui, b, ("plan", i), Sense::click());
            let col = app.a.color(("plan_c", i), if on { INK } else { DIM }, 0.3);
            text_fit(p, b.center(), Align2::CENTER_CENTER, &name, if on { font6(11.0) } else { font(11.0) }, col, 0.0, b.width() - 12.0);
            if resp.on_hover_text(&name).clicked() && !on {
                let guid = pl["guid"].as_str().unwrap_or("").to_string();
                for x in app.ctl.plans.iter_mut() {
                    x["active"] = json!(x["guid"] == guid.as_str());
                }
                call("r:ctl:plan", backend::set_power_plan(guid));
            }
        }
    }
    // actions: sleep / restart / shut down need a second click within 3 s
    let ar = Rect::from_min_max(pos2(r.left() + 14.0, seg.bottom() + 10.0), pos2(r.right() - 14.0, seg.bottom() + 10.0 + 54.0));
    let bw = (ar.width() - 24.0) / 4.0;
    let acts: [(&str, &str, &[Prim], u8); 4] = [("lock", "Lock", icons::LOCK, 0), ("sleep", "Sleep", icons::SLEEP, 0), ("restart", "Restart", icons::RESTART, 1), ("shutdown", "Shut down", icons::SHUTDOWN, 2)];
    for (i, (k, label, ico, kind)) in acts.into_iter().enumerate() {
        let b = Rect::from_min_size(pos2(ar.left() + i as f32 * (bw + 8.0), ar.top()), vec2(bw, 54.0));
        let resp = hit(app, ui, b, ("pw", k), Sense::click());
        let armed = app.ctl.power_arm.get(k).is_some_and(|t| *t > app.now);
        let hk = app.a.v(("pw_h", k), if resp.hovered() { 1.0 } else { 0.0 }, 0.3, SPRING);
        let mut b2 = b.translate(vec2(0.0, -2.0 * hk));
        let (mut bg, mut bd, mut fg) = (rgba(255, 255, 255, 0.025), lerp_c(LINE, LINE2, hk), lerp_c(DIM, TEXT, hk));
        if kind == 1 && hk > 0.0 {
            fg = lerp_c(DIM, AMBER, hk);
            bd = lerp_c(LINE, rgba(255, 181, 71, 0.45), hk);
        }
        if kind == 2 && hk > 0.0 {
            fg = lerp_c(DIM, RED, hk);
            bd = lerp_c(LINE, rgba(255, 77, 109, 0.45), hk);
        }
        if armed {
            let c = if kind == 1 { rgb(0xe8, 0x93, 0x2c) } else { RED };
            bg = c;
            bd = c;
            fg = Color32::WHITE;
            let ph = ((app.now % 1.0) as f32 * 2.0 - 1.0).abs();
            b2 = Rect::from_center_size(b2.center(), b2.size() * (1.0 + 0.04 * EASE_IN_OUT.at(1.0 - ph)));
            glow(p, b2, 12.0, 18.0, if kind == 1 { rgba(255, 181, 71, 0.45) } else { rgba(255, 77, 109, 0.45) });
        }
        boxed(p, b2, 12.0, bg, bd);
        icon(p, Rect::from_min_size(pos2(b2.center().x - 9.0, b2.top() + 9.0), vec2(18.0, 18.0)), ico, 1.9, fg);
        text(p, pos2(b2.center().x, b2.top() + 9.0 + 18.0 + 5.0), Align2::CENTER_TOP, if armed { "Confirm?" } else { label }, font(10.5), fg, 0.0);
        if resp.clicked() {
            if k == "lock" {
                call("r:toast", backend::sys("lock".into(), json!({})));
            } else if !armed {
                app.ctl.power_arm.insert(k.to_string(), app.now + 3.0);
            } else {
                app.ctl.power_arm.remove(k);
                if k == "sleep" {
                    call("r:toast", backend::sys("sleep".into(), json!({})));
                } else {
                    call("r:toast", backend::session(k.to_string()));
                }
            }
        }
    }
}

fn quick(app: &mut App, ui: &Ui, p: &Painter, r: Rect) {
    card(p, r);
    let title = app.os_text("Windows settings");
    let h = card_h(p, r, &title);
    let grid = Rect::from_min_max(pos2(r.left() + 14.0, h.bottom() + 8.0), r.max - vec2(14.0, 12.0));
    let bw = (grid.width() - 16.0) / 3.0;
    let bh = ((grid.height() - 8.0) / 2.0).max(56.0);
    let items: [(&str, &str, &[Prim]); 6] = [("display", "Display", icons::DISPLAY), ("sound", "Sound", icons::SOUND_SMALL), ("devices", "Devices", icons::BT), ("network", "Network", icons::NETWORK), ("nightlight", "Night light", icons::NIGHT), ("notifications", "Notifications", icons::BELL)];
    for (i, (k, label, ico)) in items.into_iter().enumerate() {
        let b = Rect::from_min_size(pos2(grid.left() + (i % 3) as f32 * (bw + 8.0), grid.top() + (i / 3) as f32 * (bh + 8.0)), vec2(bw, bh));
        let resp = hit(app, ui, b, ("qs", k), Sense::click());
        let hk = app.a.v(("qs_h", k), if resp.hovered() { 1.0 } else { 0.0 }, 0.3, SPRING);
        let b = b.translate(vec2(0.0, -2.0 * hk));
        boxed(p, b, 12.0, lerp_c(rgba(255, 255, 255, 0.025), rgba(62, 243, 255, 0.06), hk), lerp_c(LINE, rgba(62, 243, 255, 0.4), hk));
        let fg = lerp_c(DIM, TEXT, hk);
        let total = 18.0 + 6.0 + 13.0;
        let top = b.center().y - total / 2.0;
        icon(p, Rect::from_min_size(pos2(b.center().x - 9.0, top), vec2(18.0, 18.0)), ico, 1.9, fg);
        text(p, pos2(b.center().x, top + 24.0), Align2::CENTER_TOP, label, font(10.5), fg, 0.0);
        if resp.clicked() {
            backend::open_settings(k);
        }
    }
}

// ---------------- drawer ----------------
fn drawer(app: &mut App, ui: &Ui, r: Rect) {
    let p = ui.painter().clone();
    let kind = app.ctl.drawer.unwrap_or("out");
    // block the page underneath
    let _ = ui.interact(r, ui.scope_id().with("drawer-block"), Sense::click());
    vgrad(&p, r, [0.0; 4], &[(0.0, rgba(9, 14, 27, 0.99)), (1.0, rgba(7, 11, 21, 0.99))]);
    let inner = Rect::from_min_max(r.min + vec2(18.0, 14.0), r.max - vec2(18.0, 18.0));
    // header
    let hdr = Rect::from_min_size(inner.min, vec2(inner.width(), 28.0));
    let back = Rect::from_min_size(hdr.min, vec2(28.0, 28.0));
    let bresp = hit(app, ui, back, "dr-back", Sense::click());
    let bh = app.a.v("drb_h", if bresp.hovered() { 1.0 } else { 0.0 }, 0.2, EASE);
    fill(&p, back, 8.0, fade(rgba(255, 255, 255, 0.06), bh));
    icon(&p, Rect::from_center_size(back.center(), vec2(18.0, 18.0)), icons::BACK, 2.0, lerp_c(DIM, TEXT, bh));
    if bresp.on_hover_text("Back").clicked() {
        app.ctl.drawer = None;
    }
    let title = match kind {
        "wifi" => "Wi-Fi networks",
        "bt" => "Bluetooth devices",
        "out" => "Output device",
        _ => "Input device",
    };
    text(&p, pos2(back.right() + 8.0, hdr.center().y), Align2::LEFT_CENTER, title, font6(15.0), TEXT, 0.0);
    let link = format!("{} ↗", app.os_text("Windows settings"));
    let lw = text_w(&p, &link, font(11.0), 0.0) + 20.0;
    let lr = Rect::from_min_size(pos2(hdr.right() - lw, hdr.center().y - 13.0), vec2(lw, 26.0));
    let lresp = hit(app, ui, lr, "dr-settings", Sense::click());
    fill(&p, lr, 8.0, if lresp.hovered() { rgba(62, 243, 255, 0.08) } else { Color32::TRANSPARENT });
    text(&p, lr.center(), Align2::CENTER_CENTER, &link, font(11.0), CYAN, 0.0);
    if lresp.clicked() {
        backend::open_settings(match kind {
            "wifi" => "wifi",
            "bt" => "bluetooth",
            _ => "sound",
        });
    }
    let sep = hdr.bottom() + 8.0;
    p.line_segment([pos2(inner.left(), sep), pos2(inner.right(), sep)], Stroke::new(1.0, LINE));
    let mut y = sep + 10.0;
    let body_clip = Rect::from_min_max(pos2(r.left(), y), r.max).intersect(p.clip_rect());
    let bp = p.clone().with_clip_rect(body_clip);
    let since = app.ctl.drawer_since;
    let mut n = 0usize;
    let mut row = |app: &mut App, y: &mut f32, h: f32| -> (Rect, f32) {
        let r = Rect::from_min_size(pos2(inner.left(), *y), vec2(inner.width(), h));
        *y += h + 4.0;
        let k = app.a.v_in(("dev_in", kind, n, (since * 10.0) as i64), 0.0, 1.0, 0.45, n as f32 * 0.04, SPRING);
        n += 1;
        (r.translate(vec2(14.0 * (1.0 - k), 0.0)), k.clamp(0.0, 1.0))
    };
    match kind {
        "out" | "in" => {
            let list = app.ctl.st["audio"][if kind == "out" { "out" } else { "in" }].as_array().cloned().unwrap_or_default();
            if list.is_empty() {
                text(&bp, pos2(inner.center().x, y + 30.0), Align2::CENTER_TOP, "No devices found", font(12.0), DIM, 0.0);
            }
            for d in list {
                let (rr, k) = row(app, &mut y, 50.0);
                let mut q = bp.clone();
                q.set_opacity(k * p.opacity());
                let sel = d["isDefault"] == true;
                let name = d["name"].as_str().unwrap_or("").to_string();
                let resp = hit(app, ui, rr, ("dev", d["id"].as_str().unwrap_or("")), Sense::click());
                dev_bg(&q, rr, sel, resp.hovered());
                let ic = if kind == "out" { if regex::Regex::new("(?i)head|ear|bud|pods").unwrap().is_match(&name) { icons::HEADPHONES } else { icons::SPEAKER } } else { icons::MIC };
                dev_icon(&q, rr, ic, sel);
                let paren = regex::Regex::new(r"\((.*)\)$").unwrap().captures(&name).map(|c| c[1].to_string()).unwrap_or_default();
                let sub = format!("{paren}{}", if d["muted"] == true { " · muted".to_string() } else { format!(" · {}%", d["volume"].as_f64().unwrap_or(0.0).round()) });
                dev_text(&q, rr, &short_name(&name), &sub, DIM, 30.0);
                // radio
                let rc = pos2(rr.right() - 12.0 - 9.0, rr.center().y);
                q.circle_stroke(rc, 8.0, Stroke::new(2.0, if sel { CYAN } else { LINE2 }));
                if sel {
                    let pk = app.a.v_in(("radio", d["id"].as_str().unwrap_or("")), 0.0, 1.0, 0.4, 0.0, SPRING);
                    radial(&q, rc, 8.0, rgba(62, 243, 255, 0.4));
                    q.circle_filled(rc, 4.0 * pk, CYAN);
                }
                if resp.clicked() && !sel {
                    let id = d["id"].clone();
                    let key = if kind == "out" { "out" } else { "in" };
                    if let Some(l) = app.ctl.st["audio"][key].as_array_mut() {
                        for x in l.iter_mut() {
                            x["isDefault"] = json!(x["id"] == id);
                        }
                    }
                    app.ctl.hold_until = app.now + 2.0;
                    call_ctx("r:ctl:default", json!(name), backend::sys("setDefault".into(), json!({ "id": id })));
                }
            }
        }
        "bt" => {
            let radio_off = app.ctl.radio("Bluetooth").is_some_and(|r| r["state"] != "On");
            if radio_off {
                if note(app, ui, &bp, &mut y, inner, "Bluetooth is off.", "Turn on", true) {
                    app.ctl_toggle("bt");
                }
            }
            let devs = app.ctl.bt.clone();
            for d in devs.clone().unwrap_or_default() {
                let (rr, k) = row(app, &mut y, 50.0);
                let mut q = bp.clone();
                q.set_opacity(k * p.opacity());
                let conn = d["connected"] == true;
                let resp = hit(app, ui, rr, ("bt", d["name"].as_str().unwrap_or("")), Sense::hover());
                dev_bg(&q, rr, false, resp.hovered());
                let ic = match d["kind"].as_str().unwrap_or("") {
                    "AudioVideo" => icons::HEADPHONES,
                    "Phone" => icons::PHONE,
                    "Peripheral" => icons::KEYBOARD,
                    _ => icons::BT,
                };
                dev_icon(&q, rr, ic, conn);
                dev_text(&q, rr, d["name"].as_str().unwrap_or(""), if conn { "Connected" } else { "Paired" }, if conn { GREEN } else { DIM }, 0.0);
            }
            match &devs {
                Some(l) if l.is_empty() => {
                    text(&bp, pos2(inner.center().x, y + 30.0), Align2::CENTER_TOP, "No paired devices", font(12.0), DIM, 0.0);
                    y += 80.0;
                }
                None => {
                    text(&bp, pos2(inner.center().x, y + 30.0), Align2::CENTER_TOP, "Looking for devices...", font(12.0), DIM, 0.0);
                    y += 80.0;
                }
                _ => {}
            }
            if note(app, ui, &bp, &mut y, inner, "Use system settings to connect or pair Bluetooth devices.", "Connect or add a device", false) {
                backend::open_settings("bluetooth");
            }
        }
        _ => {
            let w = app.ctl.wifi.clone();
            let off = app.ctl.radio("WiFi").is_some_and(|r| r["state"] != "On");
            if w.is_null() || w["available"] != true {
                text(&bp, pos2(inner.center().x, y + 30.0), Align2::CENTER_TOP, if w.is_null() { "Looking for networks..." } else { "No Wi-Fi adapter found" }, font(12.0), DIM, 0.0);
            } else if off {
                if note(app, ui, &bp, &mut y, inner, "Wi-Fi is off.", "Turn on", true) {
                    app.ctl_toggle("wifi");
                }
            } else {
                let cur_ssid = w["current"]["ssid"].as_str().unwrap_or("").to_string();
                if w["current"]["state"] == "connected" {
                    let (rr, k) = row(app, &mut y, 50.0);
                    let mut q = bp.clone();
                    q.set_opacity(k * p.opacity());
                    dev_bg(&q, rr, false, false);
                    dev_icon(&q, rr, icons::WIFI, true);
                    dev_text(&q, rr, &cur_ssid, &format!("Connected · {}%", w["current"]["signal"].as_f64().unwrap_or(0.0).round()), GREEN, 110.0);
                    if dev_act(app, ui, &q, rr, "wifi-off", "Disconnect") {
                        call("r:ctl:wifi-off", backend::wifi_disconnect());
                    }
                }
                if w["needsLocation"] == true {
                    let mac = app.is_mac();
                    if note(app, ui, &bp, &mut y, inner, "Location permission is required to list Wi-Fi networks.", if mac { "Grant Wi-Fi access" } else { "Open location settings" }, true) {
                        if mac {
                            call("r:ctl:location", backend::sys("requestLocation".into(), json!({})));
                        } else {
                            backend::open_settings("location");
                        }
                    }
                }
                let nets: Vec<Value> = w["networks"].as_array().cloned().unwrap_or_default().into_iter().filter(|n| n["ssid"].as_str() != Some(cur_ssid.as_str())).collect();
                for nw in &nets {
                    let (rr, k) = row(app, &mut y, 50.0);
                    let mut q = bp.clone();
                    q.set_opacity(k * p.opacity());
                    let ssid = nw["ssid"].as_str().unwrap_or("").to_string();
                    let resp = hit(app, ui, rr, ("net", &ssid), Sense::hover());
                    dev_bg(&q, rr, false, resp.hovered());
                    dev_icon(&q, rr, icons::WIFI, false);
                    let known = nw["known"] == true;
                    let sub = format!("{}{}", if nw["secure"] == true { "Secured" } else { "Open" }, if known { " · Saved" } else { "" });
                    dev_text(&q, rr, &ssid, &sub, DIM, 150.0);
                    // signal bars
                    let bars = (nw["signal"].as_f64().unwrap_or(0.0) / 25.0).ceil() as i32;
                    let label = if app.ctl.connecting.as_deref() == Some(ssid.as_str()) { "Connecting..." } else if known { "Connect" } else { "Set up" };
                    let aw = 24.0 + text_w(&q, &label.to_uppercase(), font(10.5), 0.63);
                    let bx = rr.right() - 12.0 - aw - 12.0 - 18.0;
                    for b in 1..=4 {
                        let bh = 14.0 * [0.3, 0.55, 0.8, 1.0][b - 1];
                        let br = Rect::from_min_max(pos2(bx + (b - 1) as f32 * 5.0, rr.center().y + 7.0 - bh), pos2(bx + (b - 1) as f32 * 5.0 + 3.0, rr.center().y + 7.0));
                        q.rect_filled(br, 1.0, if b as i32 <= bars { CYAN } else { rgba(255, 255, 255, 0.15) });
                    }
                    if dev_act(app, ui, &q, rr, &format!("net-{ssid}"), label) {
                        if known {
                            app.ctl.connecting = Some(ssid.clone());
                            call_ctx("r:ctl:wifi-connect", json!(ssid), backend::wifi_connect(ssid.clone()));
                        } else {
                            backend::open_settings("wifi");
                        }
                    }
                }
                if w["needsLocation"] != true && nets.is_empty() && w["current"]["state"] != "connected" {
                    text(&bp, pos2(inner.center().x, y + 30.0), Align2::CENTER_TOP, "No networks in range", font(12.0), DIM, 0.0);
                }
            }
        }
    }
    let _ = brackets;
}

fn dev_bg(p: &Painter, r: Rect, sel: bool, hov: bool) {
    let bg = if sel { rgba(62, 243, 255, 0.06) } else if hov { rgba(255, 255, 255, 0.035) } else { Color32::TRANSPARENT };
    boxed(p, r, 12.0, bg, if sel { rgba(62, 243, 255, 0.35) } else { Color32::TRANSPARENT });
}
fn dev_icon(p: &Painter, r: Rect, ic: &[Prim], on: bool) {
    let b = Rect::from_min_size(pos2(r.left() + 12.0, r.center().y - 16.0), vec2(32.0, 32.0));
    fill(p, b, 10.0, if on { rgba(62, 243, 255, 0.15) } else { WHITE05 });
    icon(p, Rect::from_center_size(b.center(), vec2(17.0, 17.0)), ic, 1.9, if on { CYAN } else { DIM });
}
fn dev_text(p: &Painter, r: Rect, title: &str, sub: &str, sub_c: Color32, right_reserve: f32) {
    let x = r.left() + 12.0 + 32.0 + 12.0;
    let w = r.right() - 12.0 - right_reserve - x;
    text_fit(p, pos2(x, r.center().y - 1.0), Align2::LEFT_BOTTOM, title, font6(12.5), TEXT, 0.0, w);
    text_fit(p, pos2(x, r.center().y + 1.0), Align2::LEFT_TOP, sub, font(10.5), sub_c, 0.0, w);
}
fn dev_act(app: &mut App, ui: &Ui, p: &Painter, r: Rect, id: &str, label: &str) -> bool {
    let w = 24.0 + text_w(p, &label.to_uppercase(), font(10.5), 0.63);
    let b = Rect::from_min_size(pos2(r.right() - 12.0 - w, r.center().y - 14.0), vec2(w, 28.0));
    let resp = hit(app, ui, b, ("act", id), Sense::click());
    let hk = resp.hovered();
    boxed(p, b, 8.0, if hk { rgba(62, 243, 255, 0.1) } else { Color32::TRANSPARENT }, if hk { rgba(62, 243, 255, 0.45) } else { LINE2 });
    text(p, b.center(), Align2::CENTER_CENTER, &label.to_uppercase(), font(10.5), TEXT, 0.63);
    resp.clicked()
}
/// .dr-note with an underlined action. Returns true when the action is clicked.
fn note(app: &mut App, ui: &Ui, p: &Painter, y: &mut f32, inner: Rect, msg: &str, act: &str, amber: bool) -> bool {
    let (fg, bg, bd) = if amber { (AMBER, rgba(255, 181, 71, 0.07), rgba(255, 181, 71, 0.25)) } else { (DIM, Color32::TRANSPARENT, LINE) };
    let mut j = job(msg, font(11.5), fg, 0.0);
    j.append("  ", 0.0, egui::TextFormat { font_id: font(11.5), color: fg, ..Default::default() });
    j.append(act, 0.0, egui::TextFormat { font_id: font(11.5), color: CYAN, underline: Stroke::new(1.0, CYAN), ..Default::default() });
    j.wrap.max_width = inner.width() - 28.0;
    let g = p.layout_job(j);
    let r = Rect::from_min_size(pos2(inner.left(), *y), vec2(inner.width(), g.size().y + 28.0));
    boxed(p, r, 12.0, bg, bd);
    let gpos = r.min + vec2(14.0, 14.0);
    // the clickable part is the action text at the end
    let act_r = g.rows.last().map(|row| Rect::from_min_max(gpos + vec2(row.rect().right() - text_w(p, act, font(11.5), 0.0), row.rect().top()), gpos + vec2(row.rect().right(), row.rect().bottom()))).unwrap_or(r);
    p.galley(gpos, g, fg);
    *y = r.bottom() + 8.0;
    hit(app, ui, act_r, ("note", act), Sense::click()).clicked()
}
