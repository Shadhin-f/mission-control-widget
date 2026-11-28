// The morphing notch (#shell), the top bar chips, hover quick actions, the pet, the speech bubble and
// the toast. Sizes, colours and timings follow styles.css one to one.
use crate::app::{fmt_rate, App, Mode, BAR_H, CX};
use crate::draw::*;
use crate::icons;
use crate::theme::*;
use eframe::egui::{self, pos2, vec2, Align2, Color32, Mesh, Painter, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2};
use serde_json::Value;

pub fn style(ctx: &egui::Context) {
    ctx.all_styles_mut(|s| {
        s.visuals = egui::Visuals::dark();
        let v = &mut s.visuals;
        v.panel_fill = Color32::TRANSPARENT;
        v.window_fill = rgba(10, 16, 30, 0.97);
        v.window_stroke = Stroke::new(1.0, LINE2);
        v.window_corner_radius = cr(10.0);
        v.menu_corner_radius = cr(8.0);
        v.extreme_bg_color = SELECT_BG;
        v.override_text_color = Some(TEXT);
        v.selection.bg_fill = rgba(62, 243, 255, 0.30);
        v.selection.stroke = Stroke::new(1.0, CYAN);
        v.text_cursor.stroke = Stroke::new(1.5, CYAN);
        v.widgets.inactive.bg_fill = SELECT_BG;
        v.widgets.inactive.weak_bg_fill = SELECT_BG;
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, LINE2);
        v.widgets.hovered.weak_bg_fill = rgba(62, 243, 255, 0.10);
        v.widgets.hovered.bg_fill = rgba(62, 243, 255, 0.10);
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, rgba(62, 243, 255, 0.45));
        v.widgets.active.weak_bg_fill = rgba(62, 243, 255, 0.16);
        v.widgets.active.bg_fill = rgba(62, 243, 255, 0.16);
        for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open, &mut v.widgets.noninteractive] {
            w.corner_radius = cr(8.0);
            w.fg_stroke = Stroke::new(1.0, TEXT);
        }
        v.popup_shadow = egui::epaint::Shadow { offset: [0, 10], blur: 30, spread: 0, color: rgba(0, 0, 0, 0.5) };
        v.window_shadow = v.popup_shadow;
        s.spacing.scroll = egui::style::ScrollStyle { bar_width: 6.0, handle_min_length: 20.0, bar_inner_margin: 0.0, bar_outer_margin: 0.0, floating: false, ..egui::style::ScrollStyle::solid() };
        v.widgets.inactive.bg_fill = SELECT_BG;
        s.interaction.selectable_labels = false;
        s.interaction.tooltip_delay = 0.6;
        s.animation_time = 0.12;
        s.text_styles.insert(egui::TextStyle::Body, font(12.0));
        s.text_styles.insert(egui::TextStyle::Button, font(11.5));
        s.text_styles.insert(egui::TextStyle::Small, font(11.0));
    });
}

// ---------------- pet ----------------
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub enum Mood {
    #[default]
    Calm,
    Think,
    Excited,
    Happy,
    Stressed,
    Sleep,
}

pub struct Pet {
    pub mood: Mood,
    pub mood_since: f64,
    hue_step: i32,
    root_pc: Color32,
    pub net: bool,
    pub alive: bool,
    pub alive_since: f64,
    pub fidgeting: bool,
    pub next_blink: f64,
    pub blink_until: f64,
    pub blink2_at: f64,
    pub next_fidget: f64,
    pub fidget_until: f64,
    pub eye: Vec2,
    pub center: Pos2,
    pub continuous: bool,
    pub wheel_busy: bool,
    pub wheel_acc: i32,
    load_step: i32,
    cand: i32,
    cand_n: u8,
}

impl Default for Pet {
    fn default() -> Self {
        Self {
            mood: Mood::Calm,
            mood_since: 0.0,
            hue_step: -2,
            root_pc: CYAN,
            net: false,
            alive: false,
            alive_since: 0.0,
            fidgeting: false,
            next_blink: 0.0,
            blink_until: 0.0,
            blink2_at: 0.0,
            next_fidget: 0.0,
            fidget_until: 0.0,
            eye: Vec2::ZERO,
            center: pos2(CX, 24.0),
            continuous: false,
            wheel_busy: false,
            wheel_acc: 0,
            load_step: -1,
            cand: -1,
            cand_n: 0,
        }
    }
}

impl Pet {
    /// One CPU sample per stats tick (once a second).
    pub fn sample_cpu(&mut self, cpu: f64) {
        let s = ((cpu / 20.0).floor() as i32).clamp(0, 3);
        if self.load_step < 0 {
            self.load_step = s;
        }
        if s == self.load_step {
            self.cand_n = 0;
            return;
        }
        if s == self.cand {
            self.cand_n += 1;
        } else {
            self.cand = s;
            self.cand_n = 1;
        }
        if self.cand_n >= 3 {
            self.load_step = s;
            self.cand_n = 0;
        }
    }

    pub fn timers(&mut self, now: f64) {
        if now >= self.next_blink {
            if self.mood != Mood::Sleep {
                self.blink_until = now + 0.13;
                if rand::random::<f32>() < 0.25 {
                    self.blink2_at = now + 0.26;
                }
            }
            self.next_blink = now + 2.4 + rand::random::<f64>() * 4.2;
        }
        if self.blink2_at > 0.0 && now >= self.blink2_at {
            self.blink2_at = 0.0;
            self.blink_until = now + 0.11;
        }
        if now >= self.next_fidget {
            // Periodic fidget: one breath/bob cycle every ~15-35 s so the pet feels alive without animating constantly
            self.fidgeting = true;
            self.fidget_until = now + 3.4;
            self.next_fidget = now + 20.0 + rand::random::<f64>() * 15.0;
        }
        if self.fidgeting && now >= self.fidget_until {
            self.fidgeting = false;
        }
    }

    fn pc(&self) -> Color32 {
        match self.mood {
            Mood::Stressed => rgb(0xff, 0x6b, 0x5a),
            Mood::Think => rgb(0xff, 0xa0, 0x71),
            Mood::Excited => rgb(0xc4, 0x8b, 0xff),
            Mood::Calm if self.hue_step >= 0 => hsl(186.0 + self.hue_step as f32 * 22.0, 100.0, 62.0 + self.hue_step as f32 * 2.0),
            _ => self.root_pc,
        }
    }
}

/// pet.refresh() from app.js
fn refresh_pet(app: &mut App) {
    let s = &app.stats;
    let ram = if s.is_null() { 0.0 } else { s["mem"]["used"].as_f64().unwrap_or(0.0) / s["mem"]["total"].as_f64().unwrap_or(1.0).max(1.0) * 100.0 };
    let cpu = s["cpu"].as_f64().unwrap_or(0.0);
    let mood = if app.ai_busy {
        Mood::Think
    } else if app.speed_running {
        Mood::Excited
    } else if app.shell_hover || app.mode != Mode::Collapsed {
        Mood::Happy
    } else if !s.is_null() && (cpu > 88.0 || ram > 93.0) {
        Mood::Stressed
    } else if app.user_idle > 180.0 {
        Mood::Sleep
    } else {
        Mood::Calm
    };
    let pet = &mut app.pet;
    if mood != pet.mood {
        pet.mood = mood;
        pet.mood_since = app.now;
    }
    // calm colour drifts cyan -> violet with CPU load, in 4 coarse steps
    // The load step only counts once it has held for three samples, so load hovering on a boundary
    // doesn't flip the colour (and its 0.8 s glow transition) every second
    let step = if !s.is_null() && mood == Mood::Calm { pet.load_step.max(0) } else { -1 };
    if step != pet.hue_step {
        pet.hue_step = step;
        pet.root_pc = pet.pc();
    }
    pet.net = !s.is_null() && s["net"]["down"].as_f64().unwrap_or(0.0) > 2.0 * 1024.0 * 1024.0;
    let busy = (mood != Mood::Calm && mood != Mood::Sleep) || pet.net;
    let alive = busy || pet.fidgeting;
    if alive && !pet.alive {
        pet.alive_since = app.now;
    }
    pet.alive = alive;
}

/// A keyframe pulse: base -> peak at 50% -> base, each half eased in-out.
fn pulse(t: f64, period: f64) -> f32 {
    let p = ((t / period) % 1.0) as f32;
    if p < 0.5 {
        EASE_IN_OUT.at(p / 0.5)
    } else {
        1.0 - EASE_IN_OUT.at((p - 0.5) / 0.5)
    }
}

pub fn sweep_active(app: &App) -> bool {
    let t = (app.now - app.mode_since) % 9.0;
    t >= 6.25
}

// ---------------- the whole widget ----------------
pub fn draw(app: &mut App, ui: &mut Ui, hits: &mut Vec<Rect>) {
    refresh_pet(app);
    let now = app.now;
    let expanded = app.mode != Mode::Collapsed;
    let no_ai = app.no_ai();
    let pet_only = app.pet_only();
    let hover = app.shell_hover && app.mode == Mode::Collapsed;
    let (mut tw, th) = match app.mode {
        Mode::Panel => (780.0, app.panel_target_h()),
        Mode::Chat => (740.0, 600.0),
        Mode::Collapsed if hover => (if no_ai { 440.0 } else { 720.0 }, 92.0),
        Mode::Collapsed => (if no_ai { 420.0 } else { 700.0 }, 46.0),
    };
    let hidden_sides = pet_only && app.mode == Mode::Collapsed && !hover;
    if hidden_sides {
        tw = 64.0;
    }
    let a = &mut app.a;
    let w = a.v("shell_w", tw, 0.62, SPRING);
    let h = a.v("shell_h", th, 0.62, SPRING);
    let rad = a.v("shell_r", if expanded { 26.0 } else { 23.0 }, 0.5, SMOOTH);
    let op = a.v("shell_op", if hidden_sides { 0.0 } else { 1.0 }, 0.2, SMOOTH);
    let shell = Rect::from_min_size(pos2(CX - w / 2.0, 0.0), vec2(w.max(0.0), h.max(1.0)));
    app.shell = shell;
    if !hidden_sides {
        hits.push(shell);
    }
    app.pet.continuous = false;

    let p = ui.painter().clone();
    let sp = p.clone().with_clip_rect(shell.expand(60.0));
    let pc = app.a.color("root_pc", app.pet.root_pc, 0.8);
    if op > 0.001 {
        let sp = { let mut q = sp.clone(); q.set_opacity(op); q };
        // box-shadow: 0 10px 40px rgba(0,0,0,.55)
        shadow(&sp, shell, rad, vec2(0.0, 10.0), 40.0, 0.0, rgba(0, 0, 0, 0.55));
        // background: radial glow over a vertical gradient
        let (sw, sh) = (shell.width(), shell.height());
        let top_c = shell.center_top();
        fill_with(&sp, shell, [0.0, 0.0, rad, rad], 5, |q| {
            let k = ((q.y - shell.top()) / sh).clamp(0.0, 1.0);
            let base = lerp_c(rgba(10, 16, 30, 0.97), rgba(6, 10, 20, 0.97), k);
            let dx = (q.x - top_c.x) / (1.2 * sw);
            let dy = (q.y - top_c.y) / (0.8 * sh);
            let r = (dx * dx + dy * dy).sqrt() / 0.6;
            let ga = (0.09 * (1.0 - r)).max(0.0);
            over(rgba(62, 243, 255, ga), base)
        });
        // border: 1px sides and bottom, no top
        let b = shell.shrink(0.5);
        let mut pts = vec![pos2(b.left(), shell.top())];
        let r = (rad - 0.5).max(0.0).min(b.height()).min(b.width() / 2.0);
        pts.extend(arc_pts(pos2(b.left() + r, b.bottom() - r), r, std::f32::consts::PI, std::f32::consts::FRAC_PI_2));
        pts.extend(arc_pts(pos2(b.right() - r, b.bottom() - r), r, std::f32::consts::FRAC_PI_2, 0.0));
        pts.push(pos2(b.right(), shell.top()));
        sp.add(Shape::line(pts, Stroke::new(1.0, LINE2)));
        // inset 0 -1px 0 rgba(62,243,255,.08)
        sp.line_segment([pos2(shell.left() + rad, shell.bottom() - 1.5), pos2(shell.right() - rad, shell.bottom() - 1.5)], Stroke::new(1.0, rgba(62, 243, 255, 0.08)));
        // glowing bottom rim
        let inset = a_v(app, "rim_in", if expanded { 0.08 } else { 0.18 }, 0.6, SPRING);
        let rim_op = a_v(app, "rim_op", if expanded { 0.35 } else { 0.55 }, 0.4, EASE);
        let rr = Rect::from_min_max(pos2(shell.left() + w * inset, shell.bottom() - 2.0), pos2(shell.right() - w * inset, shell.bottom() - 1.0));
        hgrad(&sp, rr, 0.0, &[(0.0, Color32::TRANSPARENT), (0.5, fade(pc, rim_op)), (1.0, Color32::TRANSPARENT)]);
        // light sweep across the open panel / chat
        if expanded {
            let t = ((now - app.mode_since) % 9.0) as f32;
            if t >= 6.3 {
                let x = shell.left() - 140.0 + (t - 6.3) / 2.7 * 1100.0;
                let band = Rect::from_min_max(pos2(x, shell.top()), pos2(x + 120.0, shell.bottom())).intersect(shell);
                if band.width() > 0.0 {
                    let clip = sp.clone().with_clip_rect(shell);
                    let l = x;
                    fill_with(&clip, band, [0.0; 4], 1, |q| {
                        let u = (q.x - l) / 120.0;
                        fade(rgba(62, 243, 255, 0.07), 1.0 - (u * 2.0 - 1.0).abs())
                    });
                }
            }
        }
    }

    // panel + chat inside the shell
    let inner = shell.shrink2(vec2(1.0, 0.0));
    crate::pages::draw_panel(app, ui, inner, hits);
    crate::chat::draw(app, ui, inner, hits);

    if op > 0.001 {
        draw_bar(app, ui, shell, op, no_ai, hidden_sides);
        draw_quick(app, ui, shell);
    }
    draw_pet(app, ui, no_ai && !pet_only && app.mode == Mode::Collapsed, hover, hits);
    draw_bubble(app, ui);
    draw_toast(app, ui);
    if app.cleaner.confirm {
        hits.push(shell);
    }
}

fn a_v(app: &mut App, k: &str, t: f32, d: f32, e: Ease) -> f32 {
    app.a.v(k, t, d, e)
}

/// Source-over for two premultiplied colours.
pub fn over(top: Color32, bot: Color32) -> Color32 {
    let ta = top.a() as f32 / 255.0;
    let f = |t: u8, b: u8| (t as f32 + b as f32 * (1.0 - ta)).round().min(255.0) as u8;
    Color32::from_rgba_premultiplied(f(top.r(), bot.r()), f(top.g(), bot.g()), f(top.b(), bot.b()), f(top.a(), bot.a()))
}

// ---------------- top bar ----------------
fn draw_bar(app: &mut App, ui: &mut Ui, shell: Rect, op: f32, no_ai: bool, hidden_sides: bool) {
    let expanded = app.mode != Mode::Collapsed;
    let pad = if expanded { 22.0 } else { 14.0 };
    let bar = Rect::from_min_max(pos2(shell.left() + pad, 0.0), pos2(shell.right() - pad, BAR_H));
    let two_col = no_ai && !expanded && !app.pet_only();
    let col = if two_col { bar.width() - 62.0 } else { (bar.width() - 62.0) / 2.0 };
    if col < 20.0 || hidden_sides && op < 0.05 {
        return;
    }
    let mut p = ui.painter().clone().with_clip_rect(shell);
    p.set_opacity(op);
    let cy = BAR_H / 2.0;
    let left = Rect::from_min_max(pos2(bar.left(), cy - 13.0), pos2(bar.left() + col, cy + 13.0));
    let s = app.stats.clone();
    let has = !s.is_null();
    let total = s["mem"]["total"].as_f64().unwrap_or(1.0).max(1.0);
    let cpu = has.then(|| s["cpu"].as_f64().unwrap_or(0.0) as f32);
    let ram = has.then(|| (s["mem"]["used"].as_f64().unwrap_or(0.0) / total * 100.0) as f32);
    let gpu = if has { s["gpu"]["util"].as_f64().map(|v| v as f32) } else { None };
    let cw = (left.width() - 12.0) / 3.0;
    let chips: [(&str, &[crate::draw::Prim], Option<f32>, Color32, &str); 3] =
        [("cpu", icons::CPU, cpu, CYAN, "CPU load"), ("gpu", icons::GPU, gpu, GREEN, "GPU load"), ("ram", icons::RAM, ram, VIOLET, "Memory in use")];
    for (i, (k, ico, v, c, tip)) in chips.into_iter().enumerate() {
        let r = Rect::from_min_size(pos2(left.left() + i as f32 * (cw + 6.0), left.top()), vec2(cw, 26.0));
        stat_chip(app, &p, r, k, ico, v, c);
        if app.interactive {
            ui.interact(r, ui.scope_id().with(("chip", k)), Sense::hover()).on_hover_text(tip);
        }
    }
    if no_ai {
        return;
    }
    let right = Rect::from_min_max(pos2(bar.right() - col, cy - 13.0), pos2(bar.right(), cy + 13.0));
    let avail = right.width() - 6.0;
    let net_w = avail * 1.15 / 2.15;
    let net = Rect::from_min_size(right.min, vec2(net_w, 26.0));
    let lim = Rect::from_min_max(pos2(net.right() + 6.0, right.top()), right.max);
    // net chip
    boxed(&p, net, 13.0, rgba(255, 255, 255, 0.035), LINE);
    let (dn, up) = (if has { s["net"]["down"].as_f64() } else { None }, if has { s["net"]["up"].as_f64() } else { None });
    let f = font6(11.5);
    let x0 = net.left() + 8.0;
    let ar = text(&p, pos2(x0, cy), Align2::LEFT_CENTER, "↓", font6(11.0), CYAN, 0.0);
    text_fit(&p, pos2(ar.right() + 3.0, cy), Align2::LEFT_CENTER, &fmt_rate(dn), f.clone(), TEXT, 0.0, 56.0);
    let upw = text_w(&p, "↑", font6(11.0), 0.0);
    let ux = net.right() - 8.0 - 56.0 - 3.0 - upw;
    let ar = text(&p, pos2(ux, cy), Align2::LEFT_CENTER, "↑", font6(11.0), PINK, 0.0);
    text_fit(&p, pos2(ar.right() + 3.0, cy), Align2::LEFT_CENTER, &fmt_rate(up), f, TEXT, 0.0, 56.0);
    if app.interactive {
        ui.interact(net, ui.scope_id().with("netchip"), Sense::hover()).on_hover_text("Live download / upload");
    }
    // limits chip
    let resp = if app.interactive { Some(ui.interact(lim, ui.scope_id().with("limchip"), Sense::click())) } else { None };
    let hov = resp.as_ref().is_some_and(|r| r.hovered());
    let hk = app.a.v("lim_hover", if hov { 1.0 } else { 0.0 }, 0.2, EASE);
    let bg = lerp_c(rgba(255, 255, 255, 0.035), rgba(255, 140, 90, 0.1), hk);
    let bd = lerp_c(LINE, rgba(255, 150, 100, 0.4), hk);
    boxed(&p, lim, 13.0, bg, bd);
    let clip = p.clone().with_clip_rect(lim.intersect(p.clip_rect()));
    let provs = app.prov_list();
    let loading = app.lim_loading_until > app.now;
    for prov in ["claude", "codex"] {
        if !provs.contains(&prov) {
            continue;
        }
        let shown = app.lim_shown == Some(prov);
        let leaving = app.lim_prev.is_some_and(|(pp, t)| pp == prov && app.now - t < 0.6) && !shown;
        // .lim-row: hidden below (110%), shown, or leaving upwards (-110%)
        let target = if shown { 0.0 } else if leaving { -1.1 } else { 1.1 };
        let ty = app.a.v(("lim_ty", prov), target, if shown || leaving { 0.55 } else { 0.0 }, SPRING);
        let o = app.a.v(("lim_op", prov), if shown { 1.0 } else { 0.0 }, 0.35, EASE);
        if o < 0.01 {
            continue;
        }
        let mut rp = clip.clone();
        rp.set_opacity(op * o);
        let off = ty * 26.0;
        let row = lim.translate(vec2(0.0, off));
        let ic = Rect::from_center_size(pos2(row.left() + 9.0 + 7.5, row.center().y), vec2(15.0, 15.0));
        if loading {
            // spin 1s linear: rotate the icon by drawing it around its centre
            spin_icon(&rp, ic, prov, (app.now % 1.0) as f32);
        } else if prov == "claude" {
            icon_glow(&rp, ic, icons::SPARK, 0.0, ORANGE, 3.0);
        } else {
            icon_glow(&rp, ic, icons::HEX, 1.7, GREEN, 3.0);
        }
        let wins = limit_windows(app, prov);
        let mut x = ic.right() + 7.0;
        for (label, pct) in wins {
            let col = match pct {
                Some(v) if v >= 90.0 => RED,
                Some(v) if v >= 75.0 => AMBER,
                _ => TEXT,
            };
            let lr = text(&rp, pos2(x, row.center().y + 1.0), Align2::LEFT_CENTER, &label, font(9.0), DIM, 0.54);
            let vs = pct.map(|v| format!("{}%", v.round())).unwrap_or("--".into());
            let vr = text(&rp, pos2(lr.right() + 3.0, row.center().y), Align2::LEFT_CENTER, &vs, font6(11.5), col, 0.0);
            x = lr.right() + 3.0 + vr.width().max(28.0) + 8.0;
        }
    }
    if let Some(r) = resp {
        let tip = limits_tooltip(app);
        let r = r.on_hover_text(tip);
        if r.clicked() {
            app.lim_loading_until = app.now + 25.0;
            app.be.refresh_limits();
        }
    }
}

fn spin_icon(p: &Painter, ic: Rect, prov: &str, t: f32) {
    // Rotation of a vector icon: approximate by drawing it on a rotated copy of its points
    let ang = t * std::f32::consts::TAU;
    let c = ic.center();
    let (s, co) = (ang.sin(), ang.cos());
    let rot = |q: Pos2| {
        let d = q - c;
        c + vec2(d.x * co - d.y * s, d.x * s + d.y * co)
    };
    let prims = if prov == "claude" { icons::SPARK } else { icons::HEX };
    let k = ic.width() / 24.0;
    for pr in prims {
        let (d, filled) = match *pr {
            crate::draw::Prim::P(d) => (d, false),
            crate::draw::Prim::F(d) => (d, true),
            _ => continue,
        };
        for (pts, _) in parse_path(d).iter() {
            let pts: Vec<Pos2> = pts.iter().map(|q| rot(ic.min + vec2(q.x * k, q.y * k))).collect();
            if filled {
                fill_star(p, &pts, ORANGE);
            } else {
                line(p, pts, 1.7 * k, GREEN);
            }
        }
    }
}

/// The two windows shown on the chip (5h / 7d), with expired windows back at 0%.
pub fn limit_windows(app: &App, prov: &str) -> Vec<(String, Option<f64>)> {
    let now = crate::util::now_ms();
    let l = &app.limits[prov];
    let wins: Vec<Value> = match l["windows"].as_array() {
        Some(w) if !w.is_empty() => w.clone(),
        _ => vec![serde_json::json!({"label": "5h"}), serde_json::json!({"label": "7d"})],
    };
    wins.iter()
        .take(2)
        .map(|w| {
            let pct = w["pct"].as_f64().map(|p| {
                let r = w["resetsAt"].as_f64().unwrap_or(0.0);
                if r > 0.0 && r < now {
                    0.0
                } else {
                    p
                }
            });
            (w["label"].as_str().unwrap_or("").to_string(), pct)
        })
        .collect()
}

fn limits_tooltip(app: &App) -> String {
    let now = crate::util::now_ms();
    let mut lines = vec![];
    for p in app.prov_list() {
        let l = &app.limits[p];
        let Some(wins) = l["windows"].as_array() else { continue };
        let name = if p == "codex" { "Codex" } else { "Claude Code" };
        lines.push(match l["plan"].as_str() {
            Some(plan) => format!("{name} ({plan} plan)"),
            None => name.to_string(),
        });
        for w in wins {
            let r = w["resetsAt"].as_f64().unwrap_or(0.0);
            let pct = if r > 0.0 && r < now { 0.0 } else { w["pct"].as_f64().unwrap_or(0.0) };
            let reset = if r > now { format!(" · resets {}", crate::app::fmt_reset(r)) } else { String::new() };
            lines.push(format!("   {}: {}% used{reset}", w["label"].as_str().unwrap_or(""), pct.round()));
        }
    }
    format!("AI usage\n{}\nClick to refresh", if lines.is_empty() { "No data yet".to_string() } else { lines.join("\n") })
}

fn stat_chip(app: &mut App, p: &Painter, r: Rect, k: &str, ico: &[crate::draw::Prim], v: Option<f32>, base: Color32) {
    boxed(p, r, 13.0, rgba(255, 255, 255, 0.035), LINE);
    let pct = v.unwrap_or(0.0);
    let (warn, crit) = (v.is_some_and(|x| (75.0..90.0).contains(&x)), v.is_some_and(|x| x >= 90.0));
    let c = if crit { RED } else if warn { AMBER } else { base };
    let vcol = if crit { RED } else if warn { AMBER } else { TEXT };
    let inner = r.shrink2(vec2(8.0, 0.0));
    let cy = r.center().y;
    let vs = v.map(|x| format!("{}%", x.round())).unwrap_or("--".into());
    let vw = text_w(p, &vs, font6(12.0), 0.0).max(30.0);
    // icon, value, meter (flex 1, 14..40 px) laid out with space-between
    let meter_w = (inner.width() - 15.0 - vw - 12.0).clamp(14.0, 40.0);
    let free = (inner.width() - 15.0 - vw - meter_w - 12.0).max(0.0);
    let ic = Rect::from_center_size(pos2(inner.left() + 7.5, cy), vec2(15.0, 15.0));
    icon_glow(p, ic, ico, 1.7, base, 3.0);
    let vx = ic.right() + 6.0 + free / 2.0 + vw;
    text(p, pos2(vx, cy), Align2::RIGHT_CENTER, &vs, font6(12.0), vcol, 0.0);
    let m = Rect::from_min_size(pos2(inner.right() - meter_w, cy - 2.0), vec2(meter_w, 4.0));
    fill(p, m, 2.0, rgba(255, 255, 255, 0.08));
    // Changes under a pixel are applied without a transition (nothing visible would move)
    let target = (pct.clamp(0.0, 100.0)).round() / 100.0;
    let (kk, mc) = app.a.low(|a| {
        let prev = a.v(("meter", k), target, 0.35, SMOOTH);
        if ((target - prev) * meter_w).abs() < 0.75 {
            a.snap(("meter", k), target);
        }
        (a.v(("meter", k), target, 0.35, SMOOTH), a.color(("meter_c", k), c, 0.6))
    });
    if kk > 0.001 {
        let f = Rect::from_min_size(m.min, vec2(m.width() * kk, 4.0));
        glow(p, f, 2.0, 6.0, fade(mc, 0.8));
        fill(p, f, 2.0, mc);
    }
}

// ---------------- hover quick actions ----------------
fn draw_quick(app: &mut App, ui: &mut Ui, shell: Rect) {
    let on = app.shell_hover && app.mode == Mode::Collapsed;
    let o = app.a.vd("quick_op", if on { 1.0 } else { 0.0 }, 0.25, if on { 0.08 } else { 0.0 }, EASE);
    let ty = app.a.vd("quick_ty", if on { 0.0 } else { -8.0 }, 0.4, if on { 0.08 } else { 0.0 }, SPRING);
    if o < 0.01 {
        return;
    }
    let mut p = ui.painter().clone().with_clip_rect(shell);
    p.set_opacity(o);
    let ai = app.provider.is_some();
    let ask = format!("Ask {}", if app.provider == Some("codex") { "Codex" } else { "Claude" });
    let items: Vec<(&str, String, bool)> = if ai { vec![("panel", "Mission Control".into(), false), ("chat", ask, true)] } else { vec![("panel", "Mission Control".into(), false)] };
    let widths: Vec<f32> = items.iter().map(|(_, t, _)| 14.0 + 12.0 + 7.0 + text_w(&p, t, font(11.0), 0.44) + 14.0).collect();
    let total: f32 = widths.iter().sum::<f32>() + 10.0 * (items.len() as f32 - 1.0);
    let mut x = CX - total / 2.0;
    for ((k, t, claude), w) in items.into_iter().zip(widths) {
        let r = Rect::from_min_size(pos2(x, 52.0 + ty), vec2(w, 28.0));
        x += w + 10.0;
        let resp = if on && app.interactive { Some(ui.interact(r, ui.scope_id().with(("qbtn", k)), Sense::click())) } else { None };
        let hov = resp.as_ref().is_some_and(|r| r.hovered());
        let hk = app.a.v(("qh", k), if hov { 1.0 } else { 0.0 }, 0.2, SPRING);
        let r = r.translate(vec2(0.0, -hk));
        let (bg, bd) = if claude {
            (lerp_c(rgba(62, 243, 255, 0.06), rgba(255, 140, 90, 0.12), hk), lerp_c(LINE2, rgba(255, 150, 100, 0.5), hk))
        } else {
            (lerp_c(rgba(62, 243, 255, 0.06), rgba(62, 243, 255, 0.14), hk), lerp_c(LINE2, rgba(62, 243, 255, 0.5), hk))
        };
        boxed(&p, r, 14.0, bg, bd);
        let ic = Rect::from_center_size(pos2(r.left() + 14.0 + 6.0, r.center().y), vec2(12.0, 12.0));
        if claude {
            icon(&p, ic, icons::SPARK, 0.0, ORANGE);
        } else {
            icon(&p, ic, icons::DIAMOND, 1.8, CYAN);
        }
        text(&p, pos2(ic.right() + 7.0, r.center().y), Align2::LEFT_CENTER, &t, font(11.0), TEXT, 0.44);
        if resp.is_some_and(|r| r.clicked()) {
            app.set_mode(if k == "chat" { Mode::Chat } else { Mode::Panel });
        }
    }
}

// ---------------- the pet ----------------
fn draw_pet(app: &mut App, ui: &mut Ui, shifted: bool, hover: bool, hits: &mut Vec<Rect>) {
    let now = app.now;
    let tx = app.a.v("pet_tx", if shifted { if hover { 175.0 } else { 165.0 } } else { 0.0 }, 0.62, SPRING);
    let base = pos2(CX - 23.0 + tx, 2.0);
    let rect = Rect::from_min_size(base, vec2(46.0, 44.0));
    hits.push(rect);
    app.pet.center = rect.center();
    let resp = if app.interactive { Some(ui.interact(rect, ui.scope_id().with("pet"), Sense::click())) } else { None };
    let pressed = resp.as_ref().is_some_and(|r| r.is_pointer_button_down_on());
    if let Some(r) = &resp {
        let tip = format!("Click: Mission Control  -  Right-click: Ask {}  -  Scroll: volume", if app.provider == Some("codex") { "Codex" } else { "Claude" });
        if r.clicked() {
            app.set_mode(if app.mode == Mode::Collapsed { Mode::Panel } else { Mode::Collapsed });
        } else if r.secondary_clicked() {
            app.set_mode(if app.mode == Mode::Chat { Mode::Collapsed } else { Mode::Chat });
        }
        if r.hovered() {
            let dy = wheel(ui);
            if dy != 0.0 {
                app.pet.wheel_acc += if dy > 0.0 { 2 } else { -2 };
            }
            r.clone().on_hover_text(tip);
        }
    }
    if app.pet.wheel_acc != 0 && !app.pet.wheel_busy {
        let d = std::mem::take(&mut app.pet.wheel_acc);
        app.pet.wheel_busy = true;
        crate::backend::call("r:vol", crate::backend::sys("stepVolume".into(), serde_json::json!({ "delta": d })));
    }
    let scale = app.a.v("pet_press", if pressed { 0.92 } else { 1.0 }, 0.5, SPRING);
    let pet = &app.pet;
    let mood = pet.mood;
    let alive = pet.alive;
    let at = now - pet.alive_since;
    let pc_t = pet.pc();
    let pc = app.a.color("pet_pc", pc_t, 0.8);
    let eye_c = app.a.color("eye_pc", pc_t, 0.6);
    let p = ui.painter().clone();
    let c = rect.center();
    let tf = |q: Pos2| c + (q - c) * scale;
    let s = scale;

    // halo
    let halo_period = if mood == Mood::Sleep { 6.0 } else if pet.net { 1.2 } else { 3.4 };
    let hk = if alive { pulse(at, halo_period) } else { 0.0 };
    let halo_base = if mood == Mood::Sleep { 0.10 } else { 0.22 };
    let halo_op = halo_base + (0.34 - halo_base) * hk;
    let halo_r = 30.7 * (1.0 + 0.18 * hk) * s;
    radial(&p, c, halo_r, fade(pc, halo_op));
    // orbit (think / excited)
    let orbit_on = matches!(mood, Mood::Think | Mood::Excited);
    let oo = app.a.v("orbit_op", if orbit_on { 1.0 } else { 0.0 }, 0.4, EASE);
    let os = app.a.v("orbit_s", if orbit_on { 1.0 } else { 0.7 }, 0.5, SPRING);
    if oo > 0.01 {
        let per = if mood == Mood::Excited { 0.9 } else { 1.6 };
        let ang = if orbit_on { ((now - app.pet.mood_since) / per % 1.0) as f32 * std::f32::consts::TAU } else { 0.0 };
        let r = 27.0 * os * s;
        let n = 36;
        for i in 0..n {
            if i % 2 == 1 {
                continue;
            }
            let a0 = ang + i as f32 / n as f32 * std::f32::consts::TAU;
            let a1 = ang + (i as f32 + 1.0) / n as f32 * std::f32::consts::TAU;
            p.line_segment([c + vec2(a0.cos(), a0.sin()) * r, c + vec2(a1.cos(), a1.sin()) * r], Stroke::new(1.0, fade(pc, 0.55 * oo)));
        }
        let dot = c + vec2((ang - std::f32::consts::FRAC_PI_2).cos(), (ang - std::f32::consts::FRAC_PI_2).sin()) * r;
        radial(&p, dot, 7.0, fade(pc, 0.5 * oo));
        p.circle_filled(dot, 3.0 * s, fade(pc, oo));
        app.pet.continuous |= orbit_on;
    }
    // float group: bob / shake
    let mut off = Vec2::ZERO;
    if mood == Mood::Stressed {
        let ph = ((at / 0.5) % 1.0) as f32;
        let k = if ph < 0.25 { -EASE_IN_OUT.at(ph / 0.25) } else if ph < 0.75 { -1.0 + 2.0 * EASE_IN_OUT.at((ph - 0.25) / 0.5) } else { 1.0 - EASE_IN_OUT.at((ph - 0.75) / 0.25) };
        off.x = 0.8 * k;
    } else if alive {
        let per = if mood == Mood::Sleep { 6.0 } else { 3.4 };
        off.y = 1.6 * pulse(at, per);
    }
    let o = |x: f32, y: f32| tf(base + off + vec2(x, y));

    // fins
    let happy = mood == Mood::Happy;
    let fin_k = app.a.v("fin", if happy { 1.0 } else { 0.0 }, 0.4, SPRING);
    let wig = if happy && now - app.pet.mood_since < 1.0 {
        let ph = (((now - app.pet.mood_since) / 0.5) % 1.0) as f32;
        pulse(ph as f64, 1.0)
    } else {
        0.0
    };
    let fin_ang = 18f32.to_radians() * fin_k * (1.0 - wig);
    draw_fin(&p, o(1.0, 15.0), o(9.0, 27.0), true, fin_ang, s);
    draw_fin(&p, o(37.0, 15.0), o(45.0, 27.0), false, -fin_ang, s);

    // body
    let bc = o(23.0, 21.0);
    let br = 17.0 * s;
    let body_rect = Rect::from_center_size(bc, vec2(br * 2.0, br * 2.0));
    shadow(&p, body_rect, br, Vec2::ZERO, 14.0, 0.0, fade(pc, 0.45));
    shadow(&p, body_rect, br, vec2(0.0, 3.0), 10.0, 0.0, rgba(0, 0, 0, 0.5));
    let focal = o(6.0 + 10.88, 4.0 + 8.84);
    let stops = [(0.0, rgb(0xff, 0xff, 0xff)), (0.22, rgb(0xd6, 0xe3, 0xf2)), (0.62, rgb(0x8c, 0x9c, 0xb6)), (1.0, rgb(0x4a, 0x58, 0x73))];
    let hole = bc + vec2(-3.0, -4.0) * s;
    disc(&p, bc, br, |q| {
        let base = stops_at(&stops, q.distance(focal) / (34.17 * s));
        // inset -3px -4px 8px rgba(10,16,30,.55)
        let d = q.distance(hole);
        let sh = ((d - (br - 2.0 * s)) / (8.0 * s) + 0.35).clamp(0.0, 1.0);
        over(rgba(10, 16, 30, 0.55 * sh), base)
    });
    p.circle_stroke(bc, br + 0.5, Stroke::new(1.0, rgba(255, 255, 255, 0.12)));

    // visor
    let visor = Rect::from_min_max(o(10.0, 13.0), o(36.0, 28.0));
    vgrad(&p, visor, [7.5 * s; 4], &[(0.0, rgb(0x05, 0x09, 0x12)), (1.0, rgb(0x0f, 0x1a, 0x2e))]);
    p.rect_stroke(visor.expand(0.5), cr(8.0 * s), Stroke::new(1.0, rgba(0, 0, 0, 0.35)), egui::StrokeKind::Middle);
    fill(&p, Rect::from_min_size(visor.min + vec2(3.0, 1.0) * s, vec2(10.0, 3.0) * s), 2.0, rgba(255, 255, 255, 0.12));

    // eyes (follow the pointer)
    let target = { app.ptr.lock().unwrap().eye_target };
    let target = if mood == Mood::Think { vec2(2.5, -1.8) } else { target };
    let dt = ui.input(|i| i.stable_dt).min(0.1);
    let k = 1.0 - (1.0 - 0.28f32).powf(dt * 60.0);
    let e = &mut app.pet.eye;
    *e += (target - *e) * k;
    if (target - *e).length() < 0.08 {
        *e = target;
    } else {
        app.pet.continuous = true;
    }
    let eye_off = app.pet.eye;
    let blink = app.pet.blink_until > now && mood != Mood::Sleep;
    let (ew_t, eh_t) = match mood {
        Mood::Happy => (7.0, 5.0),
        Mood::Excited => (6.0, 10.0),
        Mood::Sleep => (7.0, 2.0),
        Mood::Stressed => (5.0, 6.0),
        _ => (5.0, 8.0),
    };
    let ew = app.a.v("eye_w", ew_t, 0.3, SPRING);
    let eh = if blink { app.a.v("eye_h", 1.0, 0.09, EASE) } else { app.a.v("eye_h", eh_t, 0.09, EASE) };
    let rot_t = if mood == Mood::Stressed { 18f32 } else { 0.0 };
    let rot = app.a.v("eye_rot", rot_t, 0.3, SPRING).to_radians();
    let happy_k = app.a.v("eye_happy", if happy { 1.0 } else { 0.0 }, 0.3, EASE);
    let vp = p.clone().with_clip_rect(visor);
    let ec = visor.center() + eye_off * s;
    let eye_op = if mood == Mood::Sleep { 0.6 } else { 1.0 };
    for (i, sign) in [(-1.0f32), 1.0].into_iter().enumerate() {
        let cx = ec.x + sign * (7.0 / 2.0 + ew / 2.0) * s;
        let center = pos2(cx, ec.y);
        if happy_k > 0.5 {
            // ^ ^ : 2px border on top and sides, rounded 6 6 0 0
            let (w2, h2) = (ew * s / 2.0, eh * s);
            let mut pts = vec![pos2(cx - w2 + 1.0, ec.y + h2 / 2.0)];
            pts.extend(arc_pts(pos2(cx, ec.y + h2 / 2.0), w2 - 1.0, std::f32::consts::PI, std::f32::consts::TAU));
            glow_line(&vp, &pts, 2.0 * s, eye_c, 3.0);
        } else {
            let r = Rect::from_center_size(center, vec2(ew * s, eh * s));
            let ang = if i == 0 { rot } else { -rot };
            if mood != Mood::Sleep {
                glow(&vp, r, 3.0, 6.0, fade(eye_c, 0.9 * eye_op));
            }
            if ang.abs() > 0.01 {
                rot_rect(&vp, r, ang, fade(eye_c, eye_op));
            } else {
                fill(&vp, r, 3.0_f32.min(r.height() / 2.0), fade(eye_c, eye_op));
                if r.height() > 3.0 && mood != Mood::Sleep {
                    vp.rect_stroke(r.shrink(0.5), cr(2.5), Stroke::new(0.8, rgba(255, 255, 255, 0.35)), egui::StrokeKind::Middle);
                }
            }
        }
    }
    // cheeks
    let ck = app.a.v("cheek", if happy { 0.8 } else { 0.0 }, 0.4, EASE);
    if ck > 0.01 {
        for x in [12.0, 29.0] {
            let r = Rect::from_min_max(o(x, 29.0), o(x + 5.0, 32.0));
            p.add(egui::epaint::EllipseShape::filled(r.center(), r.size() / 2.0, fade(rgb(0xff, 0x7f, 0xb5), ck)));
        }
    }
    // sweat drop (stressed)
    if mood == Mood::Stressed {
        let ph = (((now - app.pet.mood_since) / 2.2) % 1.0) as f32;
        let e = EASE_IN.at(ph);
        let (oa, y) = if ph < 0.2 { (0.9 * (ph / 0.2), -3.0 + 15.0 * e) } else { (0.9 * (1.0 - (ph - 0.2) / 0.8), -3.0 + 15.0 * e) };
        let d = o(39.5, 9.5 + y);
        p.add(egui::epaint::EllipseShape::filled(d, vec2(2.5, 3.5) * s, fade(rgb(0x7f, 0xd8, 0xff), oa)));
        app.pet.continuous = true;
    }
    // zzz
    let zo = app.a.v("zzz", if mood == Mood::Sleep { 1.0 } else { 0.0 }, 0.6, EASE);
    if zo > 0.01 && alive {
        for (i, size) in [(0, 9.0), (1, 11.0), (2, 13.0)] {
            let t = ((at - i as f64) / 3.0).rem_euclid(1.0) as f32;
            if at < i as f64 {
                continue;
            }
            let k = EASE_OUT.at(t);
            let a = if t < 0.3 { t / 0.3 } else { 1.0 - (t - 0.3) / 0.7 };
            let pos = base + vec2(60.0 + 10.0 * k, -2.0 + 14.0 - 20.0 * k);
            text(&p, pos, Align2::LEFT_TOP, "z", font7(size), fade(DIM, a * zo), 0.0);
        }
    }
    app.pet.continuous |= alive || blink || app.pet.fidgeting;
}

fn disc(p: &Painter, c: Pos2, r: f32, color: impl Fn(Pos2) -> Color32) {
    let (segs, rings) = (48u32, 8u32);
    let feather = 1.0 / p.ctx().pixels_per_point();
    let mut mesh = Mesh::default();
    mesh.colored_vertex(c, color(c));
    for k in 1..=rings {
        let rr = r * k as f32 / rings as f32;
        for i in 0..segs {
            let a = std::f32::consts::TAU * i as f32 / segs as f32;
            let q = c + vec2(a.cos(), a.sin()) * rr;
            mesh.colored_vertex(q, color(q));
        }
    }
    for i in 0..segs {
        let a = std::f32::consts::TAU * i as f32 / segs as f32;
        mesh.colored_vertex(c + vec2(a.cos(), a.sin()) * (r + feather), Color32::TRANSPARENT);
    }
    for i in 0..segs {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % segs);
    }
    for k in 1..=rings {
        let (a, b) = (1 + (k - 1) * segs, 1 + k * segs);
        for i in 0..segs {
            let j = (i + 1) % segs;
            mesh.add_triangle(a + i, b + i, a + j);
            mesh.add_triangle(b + i, b + j, a + j);
        }
    }
    p.add(Shape::mesh(mesh));
}

fn rounded_pts(r: Rect, rad: [f32; 4]) -> Vec<Pos2> {
    use std::f32::consts::{FRAC_PI_2, PI};
    let [tl, tr, br, bl] = rad.map(|x| x.min(r.width() / 2.0).min(r.height() / 2.0));
    let mut v = Vec::new();
    v.extend(arc_pts(pos2(r.left() + tl, r.top() + tl), tl.max(0.01), PI, PI * 1.5));
    v.extend(arc_pts(pos2(r.right() - tr, r.top() + tr), tr.max(0.01), PI * 1.5, PI * 2.0));
    v.extend(arc_pts(pos2(r.right() - br, r.bottom() - br), br.max(0.01), 0.0, FRAC_PI_2));
    v.extend(arc_pts(pos2(r.left() + bl, r.bottom() - bl), bl.max(0.01), FRAC_PI_2, PI));
    v
}

fn rot_rect(p: &Painter, r: Rect, ang: f32, color: Color32) {
    let c = r.center();
    let (s, co) = (ang.sin(), ang.cos());
    let pts: Vec<Pos2> = rounded_pts(r, [3.0; 4]).into_iter().map(|q| {
        let d = q - c;
        c + vec2(d.x * co - d.y * s, d.x * s + d.y * co)
    }).collect();
    fill_star(p, &pts, color);
}

fn draw_fin(p: &Painter, a: Pos2, b: Pos2, left: bool, ang: f32, _s: f32) {
    let r = Rect::from_two_pos(a, b);
    let rad = if left { [8.0, 2.0, 2.0, 8.0] } else { [2.0, 8.0, 8.0, 2.0] };
    // rotate about the edge that touches the body
    let origin = if left { pos2(r.right(), r.center().y) } else { pos2(r.left(), r.center().y) };
    let (s, co) = (ang.sin(), ang.cos());
    let pts: Vec<Pos2> = rounded_pts(r, rad).into_iter().map(|q| {
        let d = q - origin;
        origin + vec2(d.x * co - d.y * s, d.x * s + d.y * co)
    }).collect();
    let top = rgb(0xa8, 0xb8, 0xcf);
    let bot = rgb(0x56, 0x64, 0x7e);
    // vertical gradient in the fin's own space
    let c = pts.iter().fold(Vec2::ZERO, |acc, q| acc + q.to_vec2()) / pts.len() as f32;
    let mut mesh = Mesh::default();
    let col = |q: Pos2| {
        let d = q - origin;
        let y = origin.y + (-d.x * s + d.y * co); // un-rotate
        lerp_c(top, bot, (y - r.top()) / r.height())
    };
    mesh.colored_vertex(c.to_pos2(), col(c.to_pos2()));
    for q in &pts {
        mesh.colored_vertex(*q, col(*q));
    }
    let n = pts.len() as u32;
    for i in 0..n {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    p.add(Shape::mesh(mesh));
}

// ---------------- bubble + toast ----------------
fn draw_bubble(app: &mut App, ui: &mut Ui) {
    let show = app.bubble.as_ref().is_some_and(|b| b.until > app.now) && app.mode == Mode::Collapsed;
    let o = app.a.v("bub_op", if show { 1.0 } else { 0.0 }, 0.25, EASE);
    let k = app.a.v("bub_k", if show { 1.0 } else { 0.0 }, 0.45, SPRING);
    let Some(b) = &app.bubble else { return };
    if o < 0.01 {
        if !show {
            app.bubble = None;
        }
        return;
    }
    let mut p = ui.painter().clone();
    p.set_opacity(o);
    // shrink-to-fit up to max-width 360, centred lines
    let natural = galley(&p, &b.text, font(11.5), TEXT, 0.0).size().x;
    let mut j = job(&b.text, font(11.5), TEXT, 0.0);
    j.wrap.max_width = natural.min(360.0 - 28.0) + 0.5;
    j.halign = egui::Align::Center;
    let g = p.layout_job(j);
    let sz = g.rect.size() + vec2(28.0, 16.0);
    let scale = 0.92 + 0.08 * k;
    let top = 56.0 - 10.0 * (1.0 - k);
    let r = Rect::from_min_size(pos2(CX - sz.x * scale / 2.0, top), sz * scale);
    shadow(&p, r, 12.0, vec2(0.0, 10.0), 30.0, 0.0, rgba(0, 0, 0, 0.5));
    // arrow
    let ac = pos2(CX, r.top());
    let d = 4.5 * scale;
    let arrow = vec![ac + vec2(-d - 1.0, 1.0), ac + vec2(0.0, -d), ac + vec2(d + 1.0, 1.0)];
    p.add(Shape::convex_polygon(arrow.clone(), rgba(10, 16, 30, 0.96), Stroke::NONE));
    boxed(&p, r, 12.0 * scale, rgba(10, 16, 30, 0.96), LINE2);
    // re-cover the border under the arrow, then stroke the arrow's two sides
    p.line_segment([ac + vec2(-d + 0.5, 0.5), ac + vec2(d - 0.5, 0.5)], Stroke::new(1.5, rgba(10, 16, 30, 0.96)));
    p.add(Shape::line(arrow, Stroke::new(1.0, LINE2)));
    let off = g.rect.center().to_vec2();
    p.galley(r.center() - off, g, TEXT);
}

fn draw_toast(app: &mut App, ui: &mut Ui) {
    let show = app.toast.as_ref().is_some_and(|b| b.until > app.now);
    let o = app.a.v("toast_op", if show { 1.0 } else { 0.0 }, 0.25, EASE);
    let k = app.a.v("toast_k", if show { 1.0 } else { 0.0 }, 0.4, SPRING);
    let Some(t) = &app.toast else { return };
    if o < 0.01 {
        if !show {
            app.toast = None;
        }
        return;
    }
    let mut p = ui.painter().clone();
    p.set_opacity(o);
    let g = galley(&p, &t.text, font(11.5), TEXT, 0.0);
    let sz = g.size() + vec2(32.0, 16.0);
    let top = if app.mode == Mode::Collapsed { 104.0 } else { app.shell.height() - 52.0 };
    let r = Rect::from_min_size(pos2(CX - sz.x / 2.0, top + 10.0 * (1.0 - k)), sz);
    boxed(&p, r, 10.0, rgba(10, 16, 30, 0.97), LINE2);
    p.galley(r.center() - g.size() / 2.0, g, TEXT);
}
