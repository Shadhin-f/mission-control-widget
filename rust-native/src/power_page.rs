// The Power tab: live power draw, what it costs per day / week / month / year, where the watts go (each
// part marked measured or estimated), and the numbers the user can set or calibrate.
use crate::app::{App, ScrollState};
use crate::draw::*;
use crate::pages::{card, card_h, card_h_right, chart, ghost, thousands, PAGE_W};
use crate::power::{Part, Src};
use crate::theme::*;
use eframe::egui::{self, pos2, vec2, Align2, Color32, Rect, Sense, Ui};
use serde_json::{json, Value};
use std::collections::HashMap;

#[derive(Default)]
pub struct PowerUi {
    scroll: ScrollState,
    seen: u64,
    max: f32,
    /// text being typed into the inputs, by setting key
    buf: HashMap<&'static str, String>,
}

fn src_style(s: Src) -> (&'static str, Color32) {
    match s {
        Src::Measured => ("MEASURED", GREEN),
        Src::Estimated => ("ESTIMATED", AMBER),
        Src::Set => ("YOUR VALUE", VIOLET),
        Src::Included => ("IN CPU", DIM),
    }
}

fn kind_color(p: &Part) -> Color32 {
    match p.kind {
        "cpu" => CYAN,
        "gpu" => GREEN,
        "igpu" => DIM,
        "ram" => VIOLET,
        "charge" => GREEN,
        "rest" => rgb(0x6f, 0x8c, 0xff),
        "psu" => ORANGE,
        "monitor" => PINK,
        _ => DIM,
    }
}

pub fn money(cur: &str, v: f64) -> String {
    if v >= 10000.0 {
        format!("{cur}{}", thousands(v.round() as u64))
    } else if v >= 100.0 {
        format!("{cur}{v:.0}")
    } else if v >= 0.1 || v == 0.0 {
        format!("{cur}{v:.2}")
    } else {
        format!("{cur}{v:.3}")
    }
}

pub fn energy(wh: f64) -> String {
    if wh >= 1_000_000.0 {
        format!("{:.2} MWh", wh / 1_000_000.0)
    } else if wh >= 1000.0 {
        format!("{:.2} kWh", wh / 1000.0)
    } else {
        format!("{wh:.0} Wh")
    }
}

pub fn page(app: &mut App, ui: &mut Ui) {
    let s = app.power.snap.lock().unwrap().clone();
    // scroll: one step per new sample (just catch up after being away)
    if s.count > app.pw.seen {
        let n = s.count - app.pw.seen;
        if n > 3 {
            app.pw.scroll.count += n - 1;
            app.pw.scroll.push(app.now);
        } else {
            for _ in 0..n {
                app.pw.scroll.push(app.now);
            }
        }
        app.pw.seen = s.count;
    }
    let price = app.settings["powerPrice"].as_f64().filter(|v| *v >= 0.0);
    let cur = app.settings["powerCurrency"].as_str().map(str::trim).filter(|c| !c.is_empty()).unwrap_or(&s.currency).to_string();

    // ---------------- live ----------------
    let (r, _) = ui.allocate_exact_size(vec2(PAGE_W, 12.0 + 14.0 + 10.0 + 170.0 + 14.0), Sense::hover());
    let p = ui.painter().clone();
    card(&p, r);
    let h = card_h(&p, r, "Live power");
    if s.ready {
        let pct = (s.measured * 100.0).round();
        let c = if pct >= 70.0 { GREEN } else if pct >= 40.0 { AMBER } else { ORANGE };
        let badge = if s.dc_measured && cfg!(target_os = "macos") { "DC measured · wall estimated".to_string() } else { format!("{pct:.0}% measured") };
        let lr = card_h_right(&p, h, &badge, c);
        let d = pos2(lr.left() - 9.0, h.center().y);
        let ph = (app.now as f32 / 1.6).fract();
        p.circle_filled(d, 3.0, c);
        p.circle_stroke(d, 3.0 + ph * 5.0, egui::Stroke::new(1.0, fade(c, 0.6 * (1.0 - ph))));
    }
    let left = Rect::from_min_size(pos2(r.left() + 16.0, h.bottom() + 10.0), vec2(214.0, 170.0));
    if !s.ready {
        text(&p, left.left_top() + vec2(0.0, 20.0), Align2::LEFT_TOP, "Reading sensors...", font(12.0), DIM, 0.0);
    } else {
        let main_w = if s.on_battery { s.battery_w.unwrap_or(s.dc_w) } else { s.wall_w };
        let shown = crate::anim::num(&mut app.a, "pw_main", main_w);
        let nr = text(&p, left.left_top(), Align2::LEFT_TOP, &format!("{:.0}", shown), font6(46.0), TEXT, -0.4);
        text(&p, pos2(nr.right() + 6.0, nr.bottom() - 10.0), Align2::LEFT_BOTTOM, "W", font6(18.0), DIM, 0.0);
        let sub = if s.on_battery { "FROM THE BATTERY" } else if cfg!(target_os = "macos") { "ESTIMATED WALL POWER" } else { "FROM THE WALL" };
        let sr = text(&p, pos2(left.left(), nr.bottom() + 2.0), Align2::LEFT_TOP, sub, font(9.5), CYAN, 1.6);
        let mut y = sr.bottom() + 12.0;
        let line = |y: &mut f32, s: &str, c: Color32| {
            let r = text_fit(&p, pos2(left.left(), *y), Align2::LEFT_TOP, s, font(11.5), c, 0.0, left.width());
            *y = r.bottom() + 5.0;
        };
        if s.on_battery {
            line(&mut y, "Paid for when it recharges", DIM);
        } else if let Some(pr) = price {
            let per_h = s.wall_w as f64 / 1000.0 * pr;
            line(&mut y, &format!("{} per hour", money(&cur, per_h)), AMBER);
            line(&mut y, &format!("{} for 24 h at this rate", money(&cur, per_h * 24.0)), DIM);
        } else {
            line(&mut y, "Set your electricity price below", AMBER);
            line(&mut y, "to see costs", DIM);
        }
        let loss = s.parts.iter().find(|p| p.kind == "psu").map(|p| p.watts).unwrap_or(0.0);
        line(&mut y, &format!("System {:.0} W · loss {:.0} W", s.dc_w, loss), DIM);
        if !s.source_note.is_empty() { line(&mut y, &s.source_note, DIM2); }
        // measured vs estimated bar
        let bar = Rect::from_min_size(pos2(left.left(), left.bottom() - 22.0), vec2(left.width() - 8.0, 6.0));
        fill(&p, bar, 3.0, rgba(255, 181, 71, 0.22));
        let m = app.a.v("pw_meas", s.measured, 0.5, EASE);
        if m > 0.01 {
            let mr = Rect::from_min_size(bar.min, vec2(bar.width() * m, bar.height()));
            glow(&p, mr, 3.0, 8.0, fade(GREEN, 0.5));
            fill(&p, mr, 3.0, GREEN);
        }
        let ly = bar.bottom() + 9.0;
        text(&p, pos2(bar.left(), ly), Align2::LEFT_CENTER, "MEASURED", font(8.5), GREEN, 1.2);
        text(&p, pos2(bar.right(), ly), Align2::RIGHT_CENTER, "ESTIMATED", font(8.5), AMBER, 1.2);
    }
    // chart
    let canvas = Rect::from_min_max(pos2(left.right() + 12.0, left.top()), pos2(r.right() - 14.0, left.bottom()));
    let peak = s.hist.iter().fold(20.0f32, |a, b| a.max(*b));
    let target = (peak * 1.25).max(25.0);
    let curm = if app.pw.max <= 0.0 { target } else { app.pw.max };
    app.pw.max = curm + (target - curm) * 0.08;
    if (app.pw.max - target).abs() / target > 0.002 {
        app.pet.continuous = true;
    }
    let sc = app.pw.scroll.advance(app.now);
    chart(&p, canvas, &[(&s.hist, AMBER, rgba(255, 181, 71, 0.16))], app.pw.max, sc, 1, Some(format!("scale {:.0} W", app.pw.max)));

    // ---------------- costs ----------------
    let (row, _) = ui.allocate_exact_size(vec2(PAGE_W, 112.0), Sense::hover());
    let sm = &s.summary;
    let gw = (PAGE_W - 36.0) / 4.0;
    let periods: [(&str, f64, f64, &str); 4] = [
        ("Today", sm.today.wh, sm.daily_wh.max(sm.today.wh), "so far"),
        ("Week", sm.last7, sm.daily_wh * 7.0, "last 7 days"),
        ("Month", sm.last30, sm.daily_wh * 30.44, "last 30 days"),
        ("Year", sm.last365, sm.daily_wh * 365.0, "last 365 days"),
    ];
    for (i, (name, actual, projected, span)) in periods.into_iter().enumerate() {
        let c = Rect::from_min_size(pos2(row.left() + i as f32 * (gw + 12.0), row.top()), vec2(gw, 112.0));
        card(&p, c);
        let hh = card_h(&p, c, name);
        if i > 0 {
            card_h_right(&p, hh, "projected", DIM2);
        }
        let x = c.left() + 14.0;
        let big = if i == 0 { actual } else { projected };
        let (big_s, big_c) = match price {
            Some(pr) => (money(&cur, big / 1000.0 * pr), if i == 0 { CYAN } else { AMBER }),
            None => (energy(big), if i == 0 { CYAN } else { AMBER }),
        };
        let br = text_fit(&p, pos2(x, hh.bottom() + 8.0), Align2::LEFT_TOP, &big_s, font6(24.0), big_c, -0.2, gw - 28.0);
        let l1 = if price.is_some() { energy(big) } else { "set a price for cost".into() };
        let r1 = text_fit(&p, pos2(x, br.bottom() + 3.0), Align2::LEFT_TOP, &l1, font(10.5), DIM, 0.0, gw - 28.0);
        let l2 = if i == 0 {
            match price {
                Some(pr) => format!("about {} for the day", money(&cur, projected / 1000.0 * pr)),
                None => format!("about {} for the day", energy(projected)),
            }
        } else {
            let a = match price {
                Some(pr) => money(&cur, actual / 1000.0 * pr),
                None => energy(actual),
            };
            format!("tracked {a} · {span}")
        };
        text_fit(&p, pos2(x, r1.bottom() + 4.0), Align2::LEFT_TOP, &l2, font(10.0), DIM2, 0.0, gw - 28.0);
    }
    if !s.summary.basis.is_empty() {
        let (nr, _) = ui.allocate_exact_size(vec2(PAGE_W, 12.0), Sense::hover());
        let days = s.summary.days_tracked;
        let msg = format!("Projections: {} · {} day{} tracked so far", s.summary.basis, days, if days == 1 { "" } else { "s" });
        text(&p, pos2(nr.left() + 4.0, nr.center().y), Align2::LEFT_CENTER, &msg, font(10.0), DIM2, 0.0);
    }

    // ---------------- breakdown ----------------
    let n = s.parts.len().max(1) as f32;
    let row_h = 38.0;
    let (r, _) = ui.allocate_exact_size(vec2(PAGE_W, 12.0 + 14.0 + 8.0 + n * row_h + 10.0), Sense::hover());
    card(&p, r);
    let h = card_h(&p, r, "Where it goes");
    card_h_right(&p, h, &format!("{} detected", s.parts.iter().filter(|p| matches!(p.kind, "cpu" | "gpu" | "igpu")).count()), DIM);
    let total = s.parts.iter().map(|p| p.watts).sum::<f32>().max(1.0);
    let mut y = h.bottom() + 8.0;
    for (i, part) in s.parts.iter().enumerate() {
        let rr = Rect::from_min_size(pos2(r.left() + 14.0, y), vec2(r.width() - 28.0, row_h));
        y += row_h;
        if i > 0 {
            p.line_segment([pos2(rr.left(), rr.top() + 0.5), pos2(rr.right(), rr.top() + 0.5)], egui::Stroke::new(1.0, rgba(110, 200, 255, 0.06)));
        }
        let kc = kind_color(part);
        let dot = pos2(rr.left() + 5.0, rr.top() + 13.0);
        p.circle_filled(dot, 3.5, kc);
        if part.src == Src::Measured {
            radial(&p, dot, 8.0, fade(kc, 0.35));
        }
        // badge (right), watts, bar
        let (bl, bc) = src_style(part.src);
        let g = galley(&p, bl, font(8.5), bc, 1.1);
        let badge = Rect::from_min_size(pos2(rr.right() - g.size().x - 14.0, rr.top() + 13.0 - g.size().y / 2.0 - 2.0), g.size() + vec2(14.0, 4.0));
        fill(&p, badge, 8.0, fade(bc, 0.12));
        p.galley(badge.min + vec2(7.0, 2.0), g, bc);
        let wtxt = if part.src == Src::Included { "--".to_string() } else { format!("{:.1} W", crate::anim::num(&mut app.a, ("pw_part", i), part.watts)) };
        let wx = badge.left() - 12.0;
        text(&p, pos2(wx, rr.top() + 13.0), Align2::RIGHT_CENTER, &wtxt, font6(13.0), TEXT, 0.0);
        let bar = Rect::from_min_max(pos2(rr.left() + 330.0, rr.top() + 11.0), pos2(wx - 72.0, rr.top() + 15.0));
        fill(&p, bar, 2.0, rgba(255, 255, 255, 0.05));
        let k = app.a.v(("pw_bar", i), (part.watts / total).clamp(0.0, 1.0), 0.5, EASE);
        if k > 0.003 {
            let br = Rect::from_min_size(bar.min, vec2((bar.width() * k).max(2.0), bar.height()));
            glow(&p, br, 2.0, 6.0, fade(kc, 0.5));
            fill(&p, br, 2.0, kc);
        }
        // name, load, how
        let mut name = part.name.clone();
        if let Some(l) = part.load {
            name = format!("{name}  ·  {l:.0}%");
        }
        text_fit(&p, pos2(rr.left() + 16.0, rr.top() + 13.0), Align2::LEFT_CENTER, &name, font(12.0), TEXT, 0.0, 300.0);
        text_fit(&p, pos2(rr.left() + 16.0, rr.top() + 28.0), Align2::LEFT_CENTER, &part.how, font(10.0), DIM2, 0.0, bar.right() - rr.left() - 16.0);
    }

    // ---------------- settings ----------------
    let (r, _) = ui.allocate_exact_size(vec2(PAGE_W, 12.0 + 14.0 + 10.0 + 4.0 * 40.0 + 6.0 + 34.0 + 12.0), Sense::hover());
    card(&p, r);
    let h = card_h(&p, r, "Your numbers");
    card_h_right(&p, h, "empty = automatic", DIM2);
    let col_w = (r.width() - 28.0 - 24.0) / 2.0;
    let field = |c: usize, rw: usize| Rect::from_min_size(pos2(r.left() + 14.0 + c as f32 * (col_w + 24.0), h.bottom() + 10.0 + rw as f32 * 40.0), vec2(col_w, 32.0));
    let rest_hint = format!("auto {:.0}", s.rest_auto_w);
    let eff_hint = format!("auto {:.0}", s.eff_auto * 100.0);
    let cur_hint = s.currency.clone();
    let hours_hint = match (sm.hours, app.settings["powerHours"].is_number()) {
        (Some(h), false) => format!("learned {h:.1}"),
        _ => "auto".to_string(),
    };
    let rows: [(usize, usize, &str, &'static str, &str, &str, f32); 7] = [
        (0, 0, "Electricity price per kWh", "powerPrice", "e.g. 0.15", "", 90.0),
        (1, 0, "Currency symbol", "powerCurrency", &cur_hint, "", 70.0),
        (0, 1, "Rest of the system", "powerRestW", &rest_hint, "W", 80.0),
        (1, 1, "Power supply efficiency", "powerPsuEff", &eff_hint, "%", 80.0),
        (0, 2, "Monitor(s)", "powerMonitorW", "0", "W", 80.0),
        (1, 2, "Hours on per day", "powerHours", &hours_hint, "h", 80.0),
        (0, 3, "A wall meter reads", "powerWall", "watts", "W", 80.0),
    ];
    for (c, rw, label, key, hint, unit, w) in rows {
        let fr = field(c, rw);
        text(&p, pos2(fr.left(), fr.center().y), Align2::LEFT_CENTER, label, font(11.5), DIM, 0.0);
        let ur = Rect::from_min_size(pos2(fr.right() - (if unit.is_empty() { 0.0 } else { 18.0 }) - w, fr.top()), vec2(w, fr.height()));
        if !unit.is_empty() {
            text(&p, pos2(fr.right(), fr.center().y), Align2::RIGHT_CENTER, unit, font(11.5), DIM, 0.0);
        }
        if key == "powerWall" {
            // calibrate: Apply sets "rest of the system" so the total matches the meter
            let ab = Rect::from_min_size(pos2(ur.left() - 68.0, fr.top() + 2.0), vec2(60.0, 28.0));
            input(app, ui, &p, ur, key, hint, true);
            if ghost(app, ui, &p, ab, "pw-cal", "Apply", s.ready && !s.on_battery, false) {
                calibrate(app, &s);
            }
        } else {
            input(app, ui, &p, ur, key, hint, key != "powerCurrency");
        }
    }
    let note_y = field(0, 3).bottom() + 10.0;
    let mut j = job(
        "Counts while Mission Control is running; sleep and shutdown use very little. On a laptop on battery, energy is counted when it recharges. Got a plug-in power meter? Enter its reading and press Apply to calibrate the estimate.",
        font(10.5),
        DIM2,
        0.0,
    );
    j.wrap.max_width = r.width() - 28.0;
    let g = p.layout_job(j);
    p.galley(pos2(r.left() + 14.0, note_y), g, DIM2);
}

/// A small text field bound to a setting (numbers, or the currency symbol). Saves when it loses focus or on
/// Enter; empty means automatic.
fn input(app: &mut App, ui: &mut Ui, p: &egui::Painter, r: Rect, key: &'static str, hint: &str, numeric: bool) {
    let id = ui.scope_id().with(("pw-in", key));
    let focused = ui.memory(|m| m.has_focus(id));
    let saved = match &app.settings[key] {
        Value::Number(n) => {
            let v = n.as_f64().unwrap_or(0.0);
            if v.fract() == 0.0 { format!("{v:.0}") } else { format!("{v}") }
        }
        Value::String(s) => s.clone(),
        _ => String::new(),
    };
    let mut buf = app.pw.buf.remove(key).unwrap_or_else(|| saved.clone());
    if !focused && key != "powerWall" {
        buf = saved.clone();
    }
    let fk = app.a.v(("pw_f", key), if focused { 1.0 } else { 0.0 }, 0.2, EASE);
    boxed(p, r, 8.0, rgba(255, 255, 255, 0.03), lerp_c(LINE2, rgba(62, 243, 255, 0.5), fk));
    let inner = r.shrink2(vec2(9.0, 8.0));
    if app.live {
        let te = egui::TextEdit::singleline(&mut buf)
            .id(id)
            .frame(egui::Frame::NONE)
            .font(font(12.0))
            .text_color(TEXT)
            .hint_text(egui::RichText::new(hint).color(DIM2).font(font(12.0)))
            .desired_width(inner.width());
        let resp = ui.put(inner, te);
        if resp.lost_focus() && key != "powerWall" {
            let t = buf.trim().replace(',', ".");
            let v = if t.is_empty() {
                Some(Value::Null)
            } else if !numeric {
                Some(json!(buf.trim().chars().take(6).collect::<String>()))
            } else {
                t.parse::<f64>().ok().filter(|v| v.is_finite() && *v >= 0.0).map(|v| json!(v))
            };
            match v {
                Some(v) if v != app.settings[key] => app.set_setting(key, v),
                Some(_) => {}
                None => {
                    app.toast("Enter a number, or leave it empty for automatic");
                    buf = saved;
                }
            }
        }
    } else {
        let (t, c) = if buf.is_empty() { (hint.to_string(), DIM2) } else { (buf.clone(), TEXT) };
        text(p, pos2(inner.left(), inner.center().y), Align2::LEFT_CENTER, &t, font(12.0), c, 0.0);
    }
    app.pw.buf.insert(key, buf);
}

/// Set "rest of the system" so the estimated wall power matches a meter reading.
fn calibrate(app: &mut App, s: &crate::power::Snapshot) {
    let t = app.pw.buf.get("powerWall").cloned().unwrap_or_default();
    let Some(wall) = t.trim().replace(',', ".").parse::<f32>().ok().filter(|v| *v > 0.0 && *v < 5000.0) else {
        app.toast("Enter the watts your meter shows first");
        return;
    };
    let monitor = app.settings["powerMonitorW"].as_f64().unwrap_or(0.0) as f32;
    // the efficiency in use right now (automatic or set)
    let eff = if s.wall_w - monitor > 1.0 { (s.dc_w / (s.wall_w - monitor)).clamp(0.5, 1.0) } else { s.eff_auto };
    if s.dc_measured && cfg!(target_os = "macos") {
        let efficiency = s.dc_w / (wall - monitor);
        if !efficiency.is_finite() || !(0.5..=1.0).contains(&efficiency) {
            app.toast("The wall reading must exceed the Mac's measured DC input; check the meter and monitor value");
            return;
        }
        app.set_setting("powerPsuEff", json!(efficiency * 100.0));
        app.pw.buf.insert("powerWall", String::new());
        app.toast(format!("Calibrated charger efficiency to {:.0}%", efficiency * 100.0));
        return;
    }
    let rest = s.parts.iter().find(|p| p.kind == "rest").map(|p| p.watts).unwrap_or(s.rest_auto_w);
    let dc_needed = (wall - monitor) * eff;
    let new_rest = ((rest + dc_needed - s.dc_w) * 10.0).round() / 10.0;
    if new_rest < 0.0 {
        app.toast(format!("The meter reads less than the measured parts alone ({:.0} W). Check the monitor and efficiency settings.", s.wall_w - rest / eff));
        return;
    }
    app.set_setting("powerRestW", json!(new_rest));
    app.pw.buf.insert("powerWall", String::new());
    app.toast(format!("Calibrated: rest of the system is now {new_rest:.0} W"));
}
