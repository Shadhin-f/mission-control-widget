// Mission Control panel: tab bar and the Overview, Network, Apps, Cleaner, Settings and Credits pages.
use crate::app::*;
use crate::backend;
use crate::draw::*;
use crate::icons;
use crate::theme::*;
use eframe::egui::{self, pos2, vec2, Align2, Color32, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, UiBuilder};
use serde_json::{json, Value};
use std::hash::Hash;

pub const PAGE_W: f32 = 744.0;

/// Interaction that only counts on the live page while the window takes the mouse.
pub fn hit(app: &App, ui: &Ui, r: Rect, id: impl Hash + std::fmt::Debug, sense: Sense) -> Response {
    ui.interact(r, ui.scope_id().with(id), if app.live && app.interactive { sense } else { Sense::hover() })
}

/// A .card: translucent fill, 1px line, and the two cyan corner brackets.
pub fn card(p: &Painter, r: Rect) {
    boxed(p, r, 14.0, BG_CARD, LINE);
    brackets(p, r);
}
pub fn brackets(p: &Painter, r: Rect) {
    use std::f32::consts::{FRAC_PI_2, PI};
    let c = rgba(62, 243, 255, 0.45);
    p.add(Shape::line(arc_pts(pos2(r.left() + 7.5, r.top() + 7.5), 8.0, PI, PI + FRAC_PI_2), Stroke::new(1.0, c)));
    p.add(Shape::line(arc_pts(pos2(r.right() - 7.5, r.bottom() - 7.5), 8.0, 0.0, FRAC_PI_2), Stroke::new(1.0, c)));
}

/// .card-h: small uppercase heading; returns its rect.
pub fn card_h(p: &Painter, r: Rect, title: &str) -> Rect {
    let h = Rect::from_min_max(pos2(r.left() + 14.0, r.top() + 12.0), pos2(r.right() - 14.0, r.top() + 26.0));
    text(p, pos2(h.left(), h.center().y), Align2::LEFT_CENTER, &title.to_uppercase(), font(10.0), DIM, 1.6);
    h
}

/// The right-hand side of a .card-h (dim text), vertically centred on the heading.
pub fn card_h_right(p: &Painter, h: Rect, s: &str, color: Color32) -> Rect {
    text(p, pos2(h.right(), h.center().y), Align2::RIGHT_CENTER, &s.to_uppercase(), font(10.0), color, 1.6)
}

pub fn status_pill(p: &Painter, right_center: Pos2, status: &str) -> Rect {
    let label = match status {
        "online" => "ONLINE",
        "unstable" => "UNSTABLE",
        "offline" => "OFFLINE",
        _ => "--",
    };
    let (fg, bg) = match status {
        "online" => (GREEN, rgba(68, 255, 168, 0.1)),
        "unstable" => (AMBER, rgba(255, 181, 71, 0.12)),
        "offline" => (RED, rgba(255, 77, 109, 0.14)),
        _ => (DIM, rgba(255, 255, 255, 0.06)),
    };
    let g = galley(p, label, font(9.0), fg, 1.26);
    let r = Rect::from_min_size(pos2(right_center.x - g.size().x - 16.0, right_center.y - g.size().y / 2.0 - 2.0), g.size() + vec2(16.0, 4.0));
    if status == "online" {
        glow(p, r, 9.0, 10.0, rgba(68, 255, 168, 0.15));
    }
    fill(p, r, 9.0, bg);
    p.galley(r.min + vec2(8.0, 2.0), g, fg);
    r
}

// ---------------- the panel ----------------
pub fn draw_panel(app: &mut App, ui: &mut Ui, shell: Rect, hits: &mut Vec<Rect>) {
    let on = app.mode == Mode::Panel;
    let op = app.a.vd("panel_op", if on { 1.0 } else { 0.0 }, if on { 0.35 } else { 0.18 }, if on { 0.16 } else { 0.0 }, EASE);
    let ty = app.a.vd("panel_ty", if on { 0.0 } else { -10.0 }, if on { 0.55 } else { 0.3 }, if on { 0.12 } else { 0.0 }, if on { SPRING } else { SMOOTH });
    if op < 0.005 {
        return;
    }
    let top = BAR_H + ty;
    let left = CX - 390.0;
    let clip = Rect::from_min_max(pos2(shell.left(), BAR_H), shell.max);
    let mut root = ui.new_child(UiBuilder::new().max_rect(Rect::from_min_size(pos2(left, top), vec2(780.0, MAX_PANEL_H - BAR_H))));
    root.set_clip_rect(clip);
    root.set_opacity(op);
    let was_live = app.live;
    app.live = on;
    tabs_nav(app, &mut root, Rect::from_min_size(pos2(left + 18.0, top), vec2(744.0, TABS_H)));
    // pages
    let room = MAX_PANEL_H - BAR_H - TABS_H;
    let page_top = top + TABS_H;
    for pass in 0..2 {
        for t in TABS {
            let active = t == app.tab;
            if (pass == 0) == active {
                continue; // inactive pages first, the active page on top
            }
            if t == Tab::Settings && false {
                continue;
            }
            let target_x = if active { 0.0 } else if app.page_left[t.idx()] { -18.0 } else { 18.0 };
            let po = app.a.vd(("pg_op", t), if active { 1.0 } else { 0.0 }, if active { 0.35 } else { 0.2 }, if active { 0.05 } else { 0.0 }, EASE);
            if po < 0.005 && !active {
                app.a.snap(("pg_x", t), target_x);
                continue;
            }
            let px = app.a.vd(("pg_x", t), target_x, if active { 0.5 } else { 0.35 }, if active { 0.05 } else { 0.0 }, if active { SPRING } else { SMOOTH });
            let rect = Rect::from_min_size(pos2(left + px, page_top), vec2(780.0, room));
            let mut pu = root.new_child(UiBuilder::new().max_rect(rect).id_salt(("page", t)));
            pu.set_clip_rect(clip.intersect(Rect::from_min_max(pos2(shell.left(), page_top), shell.max)));
            pu.multiply_opacity(po);
            app.live = on && active;
            let h = page(app, &mut pu, t, rect, room);
            if active {
                app.page_h.insert(t, h);
            }
        }
    }
    app.live = was_live;
    if on {
        hits.push(shell);
    }
}

fn page(app: &mut App, ui: &mut Ui, t: Tab, rect: Rect, room: f32) -> f32 {
    let inner = Rect::from_min_size(rect.min + vec2(18.0, 14.0), vec2(PAGE_W, room - 32.0));
    let mut cu = ui.new_child(UiBuilder::new().max_rect(inner).layout(egui::Layout::top_down(egui::Align::Min)));
    cu.spacing_mut().item_spacing = vec2(12.0, 12.0);
    let out = egui::ScrollArea::vertical().id_salt(("scroll", t)).max_height(room - 32.0).auto_shrink([false, true]).show(&mut cu, |ui| {
        ui.set_width(PAGE_W);
        match t {
            Tab::Overview => overview(app, ui),
            Tab::Controls => crate::controls::page(app, ui),
            Tab::Network => network(app, ui),
            Tab::Apps => apps(app, ui),
            Tab::Cleaner => cleaner(app, ui, rect),
            Tab::Settings => settings(app, ui),
            Tab::Credits => crate::credits::page(app, ui),
        }
    });
    (out.content_size.y + 32.0).min(room)
}

// ---------------- tabs ----------------
fn tabs_nav(app: &mut App, ui: &mut Ui, r: Rect) {
    let p = ui.painter().clone();
    p.line_segment([pos2(r.left(), r.bottom() - 0.5), pos2(r.right(), r.bottom() - 0.5)], Stroke::new(1.0, LINE));
    // tools (right)
    let ai = app.provider.is_some();
    let mut x = r.right();
    let mut tools: Vec<(&str, &[Prim], &str)> = vec![("collapse", icons::CHEVRON_UP, "Collapse (Esc)")];
    if ai {
        tools.push(("ai", icons::SPARK, "Ask AI (Ctrl+Alt+Space)"));
    }
    tools.push(("pin", icons::PIN, "Keep open when clicking away"));
    for (k, ico, tip) in tools {
        let b = Rect::from_min_size(pos2(x - 28.0, r.center().y - 14.0), vec2(28.0, 28.0));
        x -= 32.0;
        let resp = hit(app, ui, b, ("tool", k), Sense::click());
        let on = k == "pin" && app.pinned;
        let hk = app.a.v(("tool_h", k), if resp.hovered() { 1.0 } else { 0.0 }, 0.2, EASE);
        let bg = if on { rgba(62, 243, 255, 0.1) } else { fade(rgba(255, 255, 255, 0.06), hk) };
        fill(&p, b, 8.0, bg);
        let col = if on { CYAN } else { lerp_c(DIM, TEXT, hk) };
        let ic = Rect::from_center_size(b.center(), vec2(14.0, 14.0));
        if k == "ai" {
            icon(&p, ic, ico, 0.0, col);
        } else {
            icon(&p, ic, ico, 1.9, col);
        }
        if resp.on_hover_text(tip).clicked() {
            match k {
                "collapse" => app.set_mode(Mode::Collapsed),
                "ai" => app.set_mode(Mode::Chat),
                _ => {
                    app.pinned = !app.pinned;
                    let t = if app.pinned { "Pinned open" } else { "Unpinned" };
                    app.toast(t);
                }
            }
        }
    }
    let clock = crate::platform::fmt_clock(chrono::Local::now());
    let cr_ = text(&p, pos2(x - 8.0 + 4.0, r.center().y), Align2::RIGHT_CENTER, &clock, mono(11.0), DIM, 0.44);
    app.next_tick = app.now + (60 - chrono::Local::now().timestamp() % 60) as f64;
    // tabs (left): shrink their padding if all seven don't fit beside the tools
    let widths: Vec<f32> = TABS.iter().map(|t| text_w(&p, t.label(), font(11.0), 1.32)).collect();
    let avail = cr_.left() - 12.0 - r.left();
    let natural: f32 = widths.iter().map(|w| w + 26.0).sum::<f32>() + 2.0 * (TABS.len() as f32 - 1.0);
    let pad = if natural <= avail { 13.0 } else { ((avail - widths.iter().sum::<f32>() - 12.0) / 14.0).max(5.0) };
    let mut tx = r.left();
    let mut ink: Option<(f32, f32)> = None;
    for (t, w) in TABS.into_iter().zip(widths) {
        let tr = Rect::from_min_size(pos2(tx, r.top()), vec2(w + pad * 2.0, r.height()));
        tx += tr.width() + 2.0;
        let resp = hit(app, ui, tr, ("tab", t), Sense::click());
        let active = t == app.tab;
        let hk = app.a.v(("tab_h", t), if resp.hovered() || active { 1.0 } else { 0.0 }, 0.25, EASE);
        let col = if active { app.a.color(("tab_c", t), CYAN, 0.25) } else { app.a.color(("tab_c", t), lerp_c(DIM, TEXT, hk), 0.25) };
        text(&p, tr.center(), Align2::CENTER_CENTER, t.label(), font(11.0), col, 1.32);
        if active {
            ink = Some((tr.left() + 10.0, tr.width() - 20.0));
        }
        if resp.clicked() {
            app.set_tab(t);
        }
    }
    if let Some((ix, iw)) = ink {
        let x = app.a.v("ink_x", ix, 0.5, SPRING);
        let w = app.a.v("ink_w", iw, 0.5, SPRING);
        let ir = Rect::from_min_size(pos2(x, r.bottom() - 1.0), vec2(w, 2.0));
        glow(&p, ir, 1.0, 10.0, fade(CYAN, 0.8));
        fill(&p, ir, 1.0, CYAN);
    }
}

// ---------------- overview ----------------
fn overview(app: &mut App, ui: &mut Ui) {
    let s = app.stats.clone();
    let has = !s.is_null();
    let total = s["mem"]["total"].as_f64().unwrap_or(1.0).max(1.0);
    let used = s["mem"]["used"].as_f64().unwrap_or(0.0);
    let gpu = &s["gpu"];
    let (row, _) = ui.allocate_exact_size(vec2(PAGE_W, 129.0), Sense::hover());
    let p = ui.painter().clone();
    let gw = (PAGE_W - 36.0) / 4.0;
    let cores = app.static_["cores"].as_u64().unwrap_or(s["cores"].as_array().map(|a| a.len() as u64).unwrap_or(0));
    let gb = 1024f64.powi(3);
    let mut g: Vec<(&str, f32, String, Color32, Option<String>)> = vec![
        ("CPU", s["cpu"].as_f64().unwrap_or(0.0) as f32, if has { format!("{cores} threads") } else { "--".into() }, CYAN, None),
        ("RAM", (used / total * 100.0) as f32, if has { format!("{:.1} / {:.1} GB", used / gb, total / gb) } else { "--".into() }, VIOLET, None),
    ];
    if gpu.is_object() {
        let util = gpu["util"].as_f64();
        let mut sub = format!("{}°C{}", gpu["temp"].as_f64().map(|t| format!("{t}")).unwrap_or("--".into()), gpu["power"].as_f64().map(|w| format!(" · {} W", w.round())).unwrap_or_default());
        let name = gpu["name"].as_str().unwrap_or("").to_string();
        if gpu["temp"].is_null() && util.is_some() {
            sub = name.clone();
        }
        if util.is_none() {
            sub = format!("{name} · load unavailable");
        }
        g.push(("GPU", util.unwrap_or(0.0) as f32, sub, GREEN, None));
        let (vu, vt) = (gpu["vramUsed"].as_f64().unwrap_or(0.0), gpu["vramTotal"].as_f64().unwrap_or(0.0));
        let unified = gpu["unified"].as_bool().unwrap_or(false);
        let vsub = if unified { "Unified memory · see RAM".into() } else if vt > 0.0 { format!("{:.1} / {:.0} GB", vu / 1024.0, vt / 1024.0) } else { "Unavailable".into() };
        g.push(("VRAM", if vt > 0.0 { (vu / vt * 100.0) as f32 } else { 0.0 }, vsub, PINK, unified.then(|| "Shared".to_string())));
    } else {
        g.push(("GPU", 0.0, "No NVIDIA data".into(), GREEN, None));
        g.push(("VRAM", 0.0, "--".into(), PINK, None));
    }
    for (i, (name, pct, sub, base, over)) in g.into_iter().enumerate() {
        let r = Rect::from_min_size(pos2(row.left() + i as f32 * (gw + 12.0), row.top()), vec2(gw, 129.0));
        card(&p, r);
        gauge(app, &p, r, name, if has { pct } else { 0.0 }, &sub, base, over, has);
    }

    // load history + threads
    let (row, _) = ui.allocate_exact_size(vec2(PAGE_W, 146.0), Sense::hover());
    let lw = PAGE_W - 250.0 - 12.0;
    let lr = Rect::from_min_size(row.min, vec2(lw, 146.0));
    card(&p, lr);
    let h = card_h(&p, lr, "Load history");
    legend(&p, h, &[(CYAN, "CPU"), (VIOLET, "RAM"), (GREEN, "GPU")]);
    let canvas = Rect::from_min_size(pos2(lr.left() + 14.0, h.bottom() + 8.0), vec2(lw - 28.0, 100.0));
    let frac = ((app.now - app.last_push) as f32).clamp(0.0, 1.0);
    let hist = &app.hist;
    chart(&p, canvas, &[(&hist.cpu, CYAN, rgba(62, 243, 255, 0.16)), (&hist.ram, VIOLET, rgba(155, 123, 255, 0.10)), (&hist.gpu, GREEN, rgba(68, 255, 168, 0.08))], 100.0, frac, None);
    let cr_ = Rect::from_min_size(pos2(lr.right() + 12.0, row.top()), vec2(250.0, 146.0));
    card(&p, cr_);
    let h = card_h(&p, cr_, "Threads");
    card_h_right(&p, h, &if has { format!("{:.2} GHz base", s["cpuMhz"].as_f64().unwrap_or(0.0) / 1000.0) } else { "--".into() }, DIM);
    let grid = Rect::from_min_size(pos2(cr_.left() + 14.0, h.bottom() + 8.0), vec2(250.0 - 28.0, 100.0));
    if let Some(cs) = s["cores"].as_array() {
        let n = cs.len().max(1);
        let rows = n.div_ceil(12);
        let cw = (grid.width() - 4.0 * 11.0) / 12.0;
        let rh = (grid.height() - 4.0 * (rows as f32 - 1.0)) / rows as f32;
        for (i, c) in cs.iter().enumerate() {
            let (rr, cc) = (i / 12, i % 12);
            let cell = Rect::from_min_size(pos2(grid.left() + cc as f32 * (cw + 4.0), grid.top() + rr as f32 * (rh + 4.0)), vec2(cw, rh));
            fill(&p, cell, 3.0, rgba(255, 255, 255, 0.04));
            let v = app.a.v(("core", i), c.as_f64().unwrap_or(0.0) as f32 / 100.0, 0.8, SMOOTH);
            if v > 0.002 {
                let b = Rect::from_min_max(pos2(cell.left(), cell.bottom() - cell.height() * v), cell.max);
                glow(&p, b, 3.0, 8.0, rgba(62, 243, 255, 0.5));
                vgrad(&p, b, [3.0; 4], &[(0.0, CYAN), (1.0, rgba(62, 243, 255, 0.35))]);
            }
        }
    }

    // storage + network
    let disks = s["disks"].as_array().cloned().unwrap_or_default();
    let storage_h = 12.0 + 14.0 + 8.0 + disks.len().max(1) as f32 * 16.0 + (disks.len().max(1) as f32 - 1.0) * 9.0 + 12.0;
    let side_h = 12.0 + 14.0 + 8.0 + 66.0 + 10.0 + 8.0 + 4.0 * 12.6 + 9.0 + 12.0 + 2.0;
    let rh = storage_h.max(side_h);
    let (row, _) = ui.allocate_exact_size(vec2(PAGE_W, rh), Sense::hover());
    let sw = PAGE_W - 300.0 - 12.0;
    let sr = Rect::from_min_size(row.min, vec2(sw, rh));
    card(&p, sr);
    let h = card_h(&p, sr, "Storage");
    let free_total: f64 = disks.iter().map(|d| d["free"].as_f64().unwrap_or(0.0)).sum();
    card_h_right(&p, h, &if has { format!("{} free total", fmt_bytes(free_total, 1)) } else { "--".into() }, DIM);
    let mut y = h.bottom() + 8.0;
    for d in &disks {
        let drive = d["drive"].as_str().unwrap_or("");
        let name = if drive == "/" { "System".to_string() } else { drive.rsplit('/').next().filter(|x| !x.is_empty()).unwrap_or(drive).to_string() };
        let (t, f) = (d["total"].as_f64().unwrap_or(1.0).max(1.0), d["free"].as_f64().unwrap_or(0.0));
        let x0 = sr.left() + 14.0;
        let inner_w = sw - 28.0;
        let label_w = 85.0f32.min(inner_w - 130.0 - 20.0 - 20.0).max(45.0);
        text_fit(&p, pos2(x0, y + 8.0), Align2::LEFT_CENTER, &name, font(13.0), TEXT, 0.0, label_w);
        let bar = Rect::from_min_size(pos2(x0 + label_w + 10.0, y + 4.0), vec2(inner_w - label_w - 130.0 - 20.0, 8.0));
        fill(&p, bar, 4.0, rgba(255, 255, 255, 0.06));
        let used = app.a.v(("disk", drive), (1.0 - f / t) as f32, 1.0, SMOOTH);
        let low = f / t < 0.1;
        let fr = Rect::from_min_size(bar.min, vec2(bar.width() * used, 8.0));
        if low {
            hgrad(&p, fr, 4.0, &[(0.0, AMBER), (1.0, RED)]);
        } else {
            hgrad(&p, fr, 4.0, &[(0.0, CYAN), (1.0, VIOLET)]);
        }
        text(&p, pos2(sr.right() - 14.0, y + 8.0), Align2::RIGHT_CENTER, &format!("{} free of {}", fmt_bytes(f, 1), fmt_bytes(t, 0)), font(11.0), DIM, 0.0);
        y += 16.0 + 9.0;
    }
    let nr = Rect::from_min_size(pos2(sr.right() + 12.0, row.top()), vec2(300.0, rh));
    card(&p, nr);
    let h = card_h(&p, nr, "Network");
    status_pill(&p, pos2(h.right(), h.center().y), app.latency["status"].as_str().unwrap_or(""));
    let l = &app.latency;
    let cells = [
        ("DOWN", if has { fmt_rate(s["net"]["down"].as_f64()) } else { "--".into() }),
        ("UP", if has { fmt_rate(s["net"]["up"].as_f64()) } else { "--".into() }),
        ("PING", if l.is_null() { "--".into() } else { l["last"].as_f64().map(|v| format!("{} ms", v.round())).unwrap_or("lost".into()) }),
        ("LOSS", if l.is_null() { "--".into() } else { format!("{:.1}%", l["loss"].as_f64().unwrap_or(0.0)) }),
    ];
    let cw = (300.0 - 28.0 - 12.0) / 2.0;
    for (i, (k, v)) in cells.iter().enumerate() {
        let x = nr.left() + 14.0 + (i % 2) as f32 * (cw + 12.0);
        let yy = h.bottom() + 8.0 + (i / 2) as f32 * 37.0;
        let kr = text(&p, pos2(x, yy), Align2::LEFT_TOP, k, font(9.0), DIM, 1.26);
        text(&p, pos2(x, kr.bottom() + 1.0), Align2::LEFT_TOP, v, font6(15.0), TEXT, 0.0);
    }
    let sy = h.bottom() + 8.0 + 66.0 + 10.0;
    p.line_segment([pos2(nr.left() + 14.0, sy), pos2(nr.right() - 14.0, sy)], Stroke::new(1.0, LINE));
    let st = &app.static_;
    let rows = [
        ("Uptime", if has { fmt_uptime(s["uptime"].as_f64().unwrap_or(0.0)) } else { "--".into() }),
        ("CPU", st["cpuModel"].as_str().unwrap_or("").to_string()),
        ("GPU", gpu["name"].as_str().unwrap_or("--").to_string()),
        ("Host", format!("{} · {}", st["host"].as_str().unwrap_or(""), st["os"].as_str().unwrap_or(""))),
    ];
    let mut yy = sy + 8.0;
    let kw = rows.iter().map(|(k, _)| text_w(&p, k, font(10.5), 0.0)).fold(0.0, f32::max);
    for (k, v) in rows {
        text(&p, pos2(nr.left() + 14.0, yy), Align2::LEFT_TOP, k, font(10.5), DIM, 0.0);
        let r = text_fit(&p, pos2(nr.left() + 14.0 + kw + 10.0, yy), Align2::LEFT_TOP, &v, font(10.5), TEXT, 0.0, 300.0 - 28.0 - kw - 10.0);
        if app.live && app.interactive && !v.is_empty() {
            ui.interact(r, ui.scope_id().with(("sys", k)), Sense::hover()).on_hover_text(v);
        }
        yy += 12.6 + 3.0;
    }
}

pub fn legend(p: &Painter, h: Rect, items: &[(Color32, &str)]) {
    // right-aligned: [i] label [i] label
    let mut x = h.right();
    for (c, label) in items.iter().rev() {
        let r = text(p, pos2(x, h.center().y), Align2::RIGHT_CENTER, label, font(10.0), DIM, 0.8);
        let sq = Rect::from_center_size(pos2(r.left() - 6.0 - 4.0, h.center().y), vec2(8.0, 8.0));
        glow(p, sq, 2.0, 6.0, fade(*c, 0.8));
        fill(p, sq, 2.0, *c);
        x = sq.left() - 6.0 - 6.0;
    }
}

fn gauge(app: &mut App, p: &Painter, r: Rect, name: &str, pct: f32, sub: &str, base: Color32, over: Option<String>, has: bool) {
    let pct = pct.clamp(0.0, 100.0);
    let c = if pct >= 90.0 { RED } else if pct >= 75.0 { AMBER } else { base };
    let col = app.a.color(("g_c", name), c, 0.6);
    let center = pos2(r.center().x, r.top() + 12.0 + 44.0);
    let rad = 40.0 * 0.88;
    let w = 7.0 * 0.88;
    p.circle_stroke(center, rad, Stroke::new(w, rgba(255, 255, 255, 0.06)));
    let frac = app.a.v(("g", name), pct / 100.0, 1.0, SMOOTH);
    if frac > 0.002 {
        let a0 = -std::f32::consts::FRAC_PI_2;
        let pts = arc_pts(center, rad, a0, a0 + frac * std::f32::consts::TAU);
        glow_line(p, &pts, w, col, 4.0);
    }
    let shown = crate::anim::num(&mut app.a, ("gnum", name), pct);
    let vs = match over {
        Some(o) => o,
        None if has => format!("{}%", shown.round()),
        None => "--".into(),
    };
    let br = text(p, pos2(center.x, center.y - 3.0), Align2::CENTER_CENTER, &vs, font6(22.0), TEXT, -0.22);
    text(p, pos2(center.x, br.bottom() + 1.0), Align2::CENTER_TOP, name, font(9.0), DIM, 1.8);
    text_fit(p, pos2(r.center().x, r.top() + 12.0 + 88.0 + 6.0), Align2::CENTER_TOP, sub, font(10.5), DIM, 0.0, r.width() - 16.0);
}

/// The scrolling line chart (canvas Chart class): smooth bezier lines with a glow and a gradient fill.
pub fn chart(p: &Painter, r: Rect, series: &[(&std::collections::VecDeque<f32>, Color32, Color32)], max: f32, frac: f32, label: Option<String>) {
    let cp = p.clone().with_clip_rect(r.intersect(p.clip_rect()));
    for i in 1..4 {
        let y = (r.top() + r.height() * i as f32 / 4.0).round() + 0.5;
        cp.line_segment([pos2(r.left(), y), pos2(r.right(), y)], Stroke::new(1.0, rgba(110, 200, 255, 0.07)));
    }
    let (w, h) = (r.width(), r.height());
    let step = w / (HIST as f32 - 2.0);
    for (d, color, fillc) in series {
        if d.len() < 2 {
            continue;
        }
        let n = d.len();
        let pts: Vec<Pos2> = d.iter().enumerate().map(|(i, v)| {
            let x = w + (1.0 - frac) * step - (n - 1 - i) as f32 * step;
            let y = h - 4.0 - (v.min(max * 1.5) / max) * (h - 10.0);
            pos2(r.left() + x, r.top() + y)
        }).collect();
        let mut curve = vec![pts[0]];
        for i in 1..n {
            let (p0, p1) = (pts[i - 1], pts[i]);
            let mx = (p0.x + p1.x) / 2.0;
            let (c1, c2) = (pos2(mx, p0.y), pos2(mx, p1.y));
            for s in 1..=6 {
                let t = s as f32 / 6.0;
                let u = 1.0 - t;
                let q = p0.to_vec2() * (u * u * u) + c1.to_vec2() * (3.0 * u * u * t) + c2.to_vec2() * (3.0 * u * t * t) + p1.to_vec2() * (t * t * t);
                curve.push(q.to_pos2());
            }
        }
        // fill: gradient from `fillc` at the top of the canvas to transparent at the bottom
        let mut mesh = egui::Mesh::default();
        for (i, q) in curve.iter().enumerate() {
            let k = 1.0 - ((q.y - r.top()) / h).clamp(0.0, 1.0);
            mesh.colored_vertex(*q, fade(*fillc, k));
            mesh.colored_vertex(pos2(q.x, r.bottom()), Color32::TRANSPARENT);
            if i > 0 {
                let b = (i as u32) * 2;
                mesh.add_triangle(b - 2, b, b - 1);
                mesh.add_triangle(b, b + 1, b - 1);
            }
        }
        cp.add(Shape::mesh(mesh));
        glow_line(&cp, &curve, 1.8, *color, 8.0);
    }
    if let Some(l) = label {
        text(&cp, pos2(r.left() + 6.0, r.top() + 12.0), Align2::LEFT_BOTTOM, &l, font(10.0), rgba(125, 146, 175, 0.8), 0.0);
    }
}

// ---------------- network ----------------
fn network(app: &mut App, ui: &mut Ui) {
    let s = app.stats.clone();
    let has = !s.is_null();
    let (r, _) = ui.allocate_exact_size(vec2(PAGE_W, 12.0 + 14.0 + 8.0 + 230.0 + 12.0 + 2.0), Sense::hover());
    let p = ui.painter().clone();
    card(&p, r);
    let h = card_h(&p, r, "Live transfer");
    // legend with values: [i] ↓ rate [i] ↑ rate (values at least 70 px wide)
    let up = format!("↑ {}", if has { fmt_rate(s["net"]["up"].as_f64()) } else { "--".into() });
    let dn = format!("↓ {}", if has { fmt_rate(s["net"]["down"].as_f64()) } else { "--".into() });
    let mut x = h.right();
    for (c, v) in [(PINK, up), (CYAN, dn)] {
        let v = v.to_uppercase();
        let w = text_w(&p, &v, font6(10.0), 0.8).max(70.0);
        text(&p, pos2(x - w, h.center().y), Align2::LEFT_CENTER, &v, font6(10.0), TEXT, 0.8);
        let sq = Rect::from_center_size(pos2(x - w - 6.0 - 4.0, h.center().y), vec2(8.0, 8.0));
        glow(&p, sq, 2.0, 6.0, fade(c, 0.8));
        fill(&p, sq, 2.0, c);
        x = sq.left() - 12.0;
    }
    let canvas = Rect::from_min_size(pos2(r.left() + 14.0, h.bottom() + 8.0), vec2(r.width() - 28.0, 230.0));
    // auto scale, eased towards 1.25x the peak
    let peak = app.hist.down.iter().chain(app.hist.up.iter()).fold(1.0f32, |a, b| a.max(*b));
    let target = peak * 1.25;
    let cur = app.net_max.max(1.0);
    app.net_max = cur + (target - cur) * 0.08;
    if (app.net_max - target).abs() / target > 0.002 {
        app.pet.continuous = true;
    }
    let frac = ((app.now - app.last_push) as f32).clamp(0.0, 1.0);
    let label = format!("peak scale {}", fmt_rate(Some(app.net_max as f64)));
    chart(&p, canvas, &[(&app.hist.down, CYAN, rgba(62, 243, 255, 0.2)), (&app.hist.up, PINK, rgba(255, 79, 216, 0.12))], app.net_max, frac, Some(label));

    // speed test + connection
    let hist = app.settings["speedHistory"].as_array().cloned().unwrap_or_default();
    let hist_h = if hist.is_empty() { 13.0 } else { (hist.len() as f32 * 17.0 - 4.0).min(92.0) };
    let lat_h = 2.0 + 12.0 + 14.0 + 8.0 + 29.0 + 10.0 + 46.0 + 8.0 + 13.0 + 10.0 + 12.0 + 6.0 + hist_h + 12.0;
    let speed_h = 2.0 + 12.0 + 14.0 + 8.0 + 120.0 + 34.0 + 12.0;
    let rh = lat_h.max(speed_h);
    let (row, _) = ui.allocate_exact_size(vec2(PAGE_W, rh), Sense::hover());
    let sr = Rect::from_min_size(row.min, vec2(400.0, rh));
    card(&p, sr);
    let h = card_h(&p, sr, "Speed test");
    let res = app.speed.result.clone();
    card_h_right(&p, h, res["server"].as_str().unwrap_or("Cloudflare"), DIM);
    let dial = Rect::from_min_size(pos2(sr.left() + 14.0, h.bottom() + 8.0), vec2(190.0, 120.0));
    let k = 0.95;
    let c = pos2(dial.left() + 100.0 * k, dial.top() + 110.0 * k);
    let rad = 80.0 * k;
    let pts = arc_pts(c, rad, std::f32::consts::PI, std::f32::consts::TAU);
    p.add(Shape::line(pts.clone(), Stroke::new(12.0 * k, rgba(255, 255, 255, 0.06))));
    for q in [pts[0], pts[pts.len() - 1]] {
        p.circle_filled(q, 6.0 * k, rgba(255, 255, 255, 0.06));
    }
    let frac = app.a.v("dial", app.speed.arc, 0.35, SMOOTH);
    if frac > 0.003 {
        let n = ((pts.len() as f32 - 1.0) * frac).max(1.0) as usize;
        let seg: Vec<Pos2> = pts[..=n.min(pts.len() - 1)].to_vec();
        let (x0, x1) = (c.x - rad, c.x + rad);
        let cols: Vec<Color32> = seg.iter().map(|q| stops_at(&[(0.0, CYAN), (0.6, VIOLET), (1.0, PINK)], (q.x - x0) / (x1 - x0))).collect();
        for (extra, a) in [(9.0, 0.06), (5.0, 0.12), (2.0, 0.2)] {
            p.add(Shape::line(seg.clone(), Stroke::new(12.0 * k + extra, rgba(62, 243, 255, 0.5 * a / 0.2 * 0.4))));
        }
        stroke_colored(&p, &seg, &cols, 12.0 * k);
    }
    let shown = crate::anim::num(&mut app.a, "dial_num", app.speed.num);
    let num = if app.speed.num_int { format!("{}", shown.round()) } else { fmt_mbps(Some(shown as f64)) };
    let nr = text(&p, pos2(dial.center().x, dial.top() + 46.0), Align2::CENTER_TOP, &num, font6(30.0), TEXT, 0.0);
    let ur = text(&p, pos2(dial.center().x, nr.bottom() - 2.0), Align2::CENTER_TOP, &app.speed.unit.clone(), font(9.0), DIM, 1.8);
    text(&p, pos2(dial.center().x, ur.bottom() + 4.0), Align2::CENTER_TOP, &app.speed.phase.to_uppercase(), font(9.5), CYAN, 1.52);
    let resx = dial.right() + 12.0;
    let resw = sr.right() - 14.0 - resx;
    let cells = [
        ("DOWNLOAD", res["down"].as_f64().map(|v| format!("{} Mbps", fmt_mbps(Some(v))))),
        ("UPLOAD", res["up"].as_f64().map(|v| format!("{} Mbps", fmt_mbps(Some(v))))),
        ("PING", res["ping"].as_f64().map(|v| format!("{} ms", v.round()))),
        ("JITTER", res.get("jitter").map(|v| format!("{:.1} ms", v.as_f64().unwrap_or(0.0)))),
        ("PACKET LOSS", res.get("loss").map(|v| format!("{:.1}%", v.as_f64().unwrap_or(0.0)))),
        ("ISP", res["isp"].as_str().filter(|s| !s.is_empty()).map(|s| s.to_string())),
    ];
    let cw = (resw - 8.0) / 2.0;
    let top = dial.center().y - (3.0 * 28.0 + 16.0) / 2.0;
    for (i, (k2, v)) in cells.into_iter().enumerate() {
        let x = resx + (i % 2) as f32 * (cw + 8.0);
        let y = top + (i / 2) as f32 * 36.0;
        let v = if app.speed_running && i < 5 { None } else { v };
        let kr = text(&p, pos2(x, y), Align2::LEFT_TOP, k2, font(9.0), DIM, 1.26);
        let vs = v.unwrap_or("--".into());
        if i == 5 {
            text_fit(&p, pos2(x, kr.bottom() + 2.0), Align2::LEFT_TOP, &vs, font(10.5), TEXT, 0.0, cw);
        } else {
            text(&p, pos2(x, kr.bottom() + 1.0), Align2::LEFT_TOP, &vs, font6(14.0), TEXT, 0.0);
        }
    }
    let btn = Rect::from_min_max(pos2(sr.left() + 14.0, sr.bottom() - 12.0 - 34.0), pos2(sr.right() - 14.0, sr.bottom() - 12.0));
    let label = app.speed.label.clone();
    if cta(app, ui, &p, btn, "sp-run", &label, !app.speed_running, false) {
        app.run_speed();
    }

    // connection card
    let lr = Rect::from_min_max(pos2(sr.right() + 12.0, row.top()), row.max);
    card(&p, lr);
    let h = card_h(&p, lr, "Connection");
    let l = app.latency.clone();
    status_pill(&p, pos2(h.right(), h.center().y), l["status"].as_str().unwrap_or(""));
    let cells = [
        ("PING", l["avg"].as_f64().map(|v| format!("{} ms", v.round())).unwrap_or("--".into())),
        ("JITTER", if l.is_null() { "--".into() } else { format!("{:.1} ms", l["jitter"].as_f64().unwrap_or(0.0)) }),
        ("LOSS (2 MIN)", if l.is_null() { "--".into() } else { format!("{:.1}%", l["loss"].as_f64().unwrap_or(0.0)) }),
    ];
    let cw = (lr.width() - 28.0 - 16.0) / 3.0;
    for (i, (k2, v)) in cells.iter().enumerate() {
        let x = lr.left() + 14.0 + i as f32 * (cw + 8.0);
        let kr = text(&p, pos2(x, h.bottom() + 8.0), Align2::LEFT_TOP, k2, font(9.0), DIM, 1.26);
        text(&p, pos2(x, kr.bottom() + 1.0), Align2::LEFT_TOP, v, font6(15.0), TEXT, 0.0);
    }
    // latency bars
    let bars = Rect::from_min_size(pos2(lr.left() + 14.0, h.bottom() + 8.0 + 29.0 + 10.0), vec2(lr.width() - 28.0, 46.0));
    let n = 40;
    let bw = (bars.width() - 2.0 * (n as f32 - 1.0)) / n as f32;
    let maxv = app.lat_hist.iter().flatten().fold(30.0f64, |a, b| a.max(*b));
    let pad = n - app.lat_hist.len();
    for i in 0..n {
        if i < pad {
            continue;
        }
        let v = app.lat_hist[i - pad];
        let x = bars.left() + i as f32 * (bw + 2.0);
        let (hgt, col, op) = match v {
            None => (bars.height(), RED, 0.55),
            Some(v) => ((((v / maxv) * 100.0).max(4.0) / 100.0 * bars.height() as f64) as f32, if v > 100.0 { AMBER } else { CYAN }, 0.75),
        };
        let hh = app.a.v(("lat", i), hgt, 0.5, SMOOTH);
        fill(&p, Rect::from_min_max(pos2(x, bars.bottom() - hh.max(2.0)), pos2(x + bw, bars.bottom())), 1.0, fade(col, op));
    }
    let a = &s["net"]["adapter"];
    let adapter = if a.is_object() {
        let bps = a["linkBps"].as_f64().unwrap_or(0.0);
        let link = if bps >= 1e9 { format!("{:.1} Gbps", bps / 1e9) } else if bps > 0.0 { format!("{} Mbps", (bps / 1e6).round()) } else { a["name"].as_str().unwrap_or("").to_string() };
        format!("{} · {} · link {}", if a["wifi"].as_bool().unwrap_or(false) { "Wi-Fi" } else { "Wired" }, a["name"].as_str().unwrap_or(""), link)
    } else {
        "--".into()
    };
    let ar = text_fit(&p, pos2(lr.left() + 14.0, bars.bottom() + 8.0), Align2::LEFT_TOP, &adapter, font(10.5), DIM, 0.0, lr.width() - 28.0);
    let sub = text(&p, pos2(lr.left() + 14.0, ar.bottom() + 10.0), Align2::LEFT_TOP, "RECENT TESTS", font(10.0), DIM, 1.6);
    let mut y = sub.bottom() + 6.0;
    let hp = p.clone().with_clip_rect(Rect::from_min_max(pos2(lr.left(), y), pos2(lr.right(), (y + 92.0).min(lr.bottom() - 8.0))).intersect(p.clip_rect()));
    if hist.is_empty() {
        text(&hp, pos2(lr.left() + 14.0, y), Align2::LEFT_TOP, "No tests yet", font(10.5), DIM, 0.0);
    }
    for r in hist.iter() {
        let t = r["t"].as_f64().unwrap_or(0.0);
        let when = chrono::TimeZone::timestamp_millis_opt(&chrono::Local, t as i64).single().map(crate::platform::fmt_short).unwrap_or_default();
        let x = lr.left() + 14.0;
        let iw = lr.width() - 28.0;
        let colw = (iw - 74.0 - 56.0 - 18.0) / 2.0;
        text_fit(&hp, pos2(x, y), Align2::LEFT_TOP, &when, font(10.5), DIM, 0.0, 74.0);
        rich(&hp, pos2(x + 74.0 + 6.0, y), &[("↓ ", DIM), (&fmt_mbps(r["down"].as_f64()), TEXT)]);
        rich(&hp, pos2(x + 74.0 + 12.0 + colw, y), &[("↑ ", DIM), (&fmt_mbps(r["up"].as_f64()), TEXT)]);
        rich(&hp, pos2(x + 74.0 + 18.0 + colw * 2.0, y), &[(&format!("{}", r["ping"].as_f64().unwrap_or(0.0).round()), TEXT), (" ms", DIM)]);
        y += 17.0;
    }
}

fn rich(p: &Painter, pos: Pos2, parts: &[(&str, Color32)]) {
    let mut x = pos.x;
    for (s, c) in parts {
        let f = if *c == TEXT { font6(10.5) } else { font(10.5) };
        let r = text(p, pos2(x, pos.y), Align2::LEFT_TOP, s, f, *c, 0.0);
        x = r.right();
    }
}

/// .cta button (gradient, uppercase). Returns true when clicked.
pub fn cta(app: &mut App, ui: &Ui, p: &Painter, r: Rect, id: &str, label: &str, enabled: bool, danger: bool) -> bool {
    let resp = hit(app, ui, r, ("cta", id), if enabled { Sense::click() } else { Sense::hover() });
    let hk = app.a.v(("cta_h", id), if resp.hovered() && enabled { 1.0 } else { 0.0 }, 0.2, SPRING);
    let op = app.a.v(("cta_op", id), if enabled { 1.0 } else { 0.45 }, 0.2, EASE);
    let r = r.translate(vec2(0.0, -hk));
    let mut q = p.clone();
    q.set_opacity(op * p.opacity());
    let (a, b, g, fg) = if danger { (rgb(0xff, 0x7a, 0x59), RED, rgba(255, 77, 109, 0.25), rgb(0x1a, 0x05, 0x08)) } else { (CYAN, VIOLET, rgba(62, 243, 255, 0.25), INK) };
    let bright = |c: Color32| lerp_c(c, Color32::WHITE, 0.12 * hk);
    glow(&q, r, 10.0, 18.0, g);
    hgrad(&q, r, 10.0, &[(0.0, bright(a)), (1.0, bright(b))]);
    text(&q, r.center(), Align2::CENTER_CENTER, &label.to_uppercase(), font7(11.0), fg, 1.76);
    enabled && resp.clicked()
}

/// .ghost button. Returns true when clicked.
pub fn ghost(app: &mut App, ui: &Ui, p: &Painter, r: Rect, id: &str, label: &str, enabled: bool, danger: bool) -> bool {
    let resp = hit(app, ui, r, ("ghost", id), if enabled { Sense::click() } else { Sense::hover() });
    let hk = app.a.v(("ghost_h", id), if resp.hovered() && enabled { 1.0 } else { 0.0 }, 0.2, EASE);
    let mut q = p.clone();
    q.set_opacity(if enabled { 1.0 } else { 0.45 } * p.opacity());
    let bd = if danger { rgba(255, 77, 109, 0.35) } else { LINE2 };
    boxed(&q, r, 10.0, fade(rgba(62, 243, 255, 0.08), hk), lerp_c(bd, rgba(62, 243, 255, 0.45), hk));
    text(&q, r.center(), Align2::CENTER_CENTER, &label.to_uppercase(), font(11.0), if danger { RED } else { TEXT }, 1.32);
    enabled && resp.clicked()
}

impl App {
    pub fn run_speed(&mut self) {
        if self.speed_running {
            return;
        }
        self.speed_running = true;
        self.speed.label = "Testing...".into();
        for k in ["down", "up", "ping", "jitter", "loss"] {
            if let Some(o) = self.speed.result.as_object_mut() {
                o.remove(k);
            }
        }
        let net = self.be.net.clone();
        backend::call("r:speed", async move { net.speed_test(&tauri::AppHandle).await });
    }

    pub fn on_speed(&mut self, p: Value) {
        match p["phase"].as_str() {
            Some("ping") => {
                self.speed.phase = "Measuring ping".into();
                self.speed.unit = "ms".into();
                if let Some(v) = p["value"].as_f64() {
                    self.speed.num = v as f32;
                    self.speed.num_int = true;
                }
                self.speed.arc = p["pct"].as_f64().unwrap_or(0.0) as f32 * 0.15;
            }
            Some(ph @ ("download" | "upload")) => {
                self.speed.phase = if ph == "download" { "↓ Download" } else { "↑ Upload" }.into();
                self.speed.unit = "Mbps".into();
                let m = p["mbps"].as_f64().unwrap_or(0.0).max(0.0);
                self.speed.num = m as f32;
                self.speed.num_int = false;
                self.speed.arc = (m / 1000.0).sqrt().min(1.0) as f32;
            }
            Some("done") => self.speed.result = p["result"].clone(),
            _ => {}
        }
    }

    pub fn on_speed_done(&mut self, v: Value) {
        self.speed_running = false;
        self.speed.label = "Run again".into();
        if v["ok"] == true {
            let r = v["v"].clone();
            self.be.speed_done(&r);
            let s = self.be.public_settings();
            self.settings = s;
            let down = r["down"].as_f64().unwrap_or(0.0);
            self.speed.num = down as f32;
            self.speed.num_int = false;
            self.speed.arc = (down / 1000.0).sqrt().min(1.0) as f32;
            self.speed.phase = "Download result".into();
            self.speed.result = r.clone();
            self.say(format!("Speed test: ↓ {} / ↑ {} Mbps · {} ms", fmt_mbps(r["down"].as_f64()), fmt_mbps(r["up"].as_f64()), r["ping"].as_f64().unwrap_or(0.0).round()), None, 6000.0, 0.0);
        } else {
            self.speed.phase = "Failed".into();
            self.toast(format!("Speed test failed: {}", v["err"].as_str().unwrap_or("error")));
        }
    }
}

// ---------------- apps ----------------
fn apps(app: &mut App, ui: &mut Ui) {
    let (head, _) = ui.allocate_exact_size(vec2(PAGE_W, 32.0), Sense::hover());
    let p = ui.painter().clone();
    // filter input
    let fr = Rect::from_min_size(head.min, vec2(220.0, 32.0));
    let te_id = ui.scope_id().with("app-filter");
    let focused = ui.memory(|m| m.has_focus(te_id));
    let fk = app.a.v("filter_f", if focused { 1.0 } else { 0.0 }, 0.2, EASE);
    boxed(&p, fr, 10.0, rgba(255, 255, 255, 0.03), lerp_c(LINE2, rgba(62, 243, 255, 0.5), fk));
    if app.live {
        let te = egui::TextEdit::singleline(&mut app.apps.filter).id(te_id).frame(egui::Frame::NONE).font(font(12.0)).text_color(TEXT).hint_text(egui::RichText::new("Filter processes...").color(DIM).font(font(12.0))).desired_width(196.0);
        ui.put(fr.shrink2(vec2(12.0, 8.0)), te);
    } else {
        let t = if app.apps.filter.is_empty() { ("Filter processes...", DIM) } else { (app.apps.filter.as_str(), TEXT) };
        text(&p, pos2(fr.left() + 12.0, fr.center().y), Align2::LEFT_CENTER, t.0, font(12.0), t.1, 0.0);
    }
    let data = app.apps.data.clone();
    let total = app.stats["mem"]["total"].as_f64().unwrap_or(1.0).max(1.0);
    let used = app.stats["mem"]["used"].as_f64().unwrap_or(0.0);
    let all: Vec<Value> = data["apps"].as_array().into_iter().flatten().chain(data["background"].as_array().into_iter().flatten()).cloned().collect();
    // memory summary bar: top 5 + rest
    let ms = Rect::from_min_max(pos2(fr.right() + 12.0, head.top()), head.max);
    if !data.is_null() && !app.stats.is_null() {
        let label = format!("{} / {} · {} processes", fmt_bytes(used, 1), fmt_bytes(total, 1), data["total"].as_u64().unwrap_or(0));
        let lr = text(&p, pos2(ms.right(), ms.center().y), Align2::RIGHT_CENTER, &label, font(11.0), DIM, 0.0);
        let bar = Rect::from_min_max(pos2(ms.left(), ms.center().y - 4.0), pos2(lr.left() - 10.0, ms.center().y + 4.0));
        fill(&p, bar, 4.0, rgba(255, 255, 255, 0.06));
        let bp = p.clone().with_clip_rect(bar.intersect(p.clip_rect()));
        let mut top = all.clone();
        top.sort_by(|a, b| b["mem"].as_f64().unwrap_or(0.0).partial_cmp(&a["mem"].as_f64().unwrap_or(0.0)).unwrap());
        top.truncate(5);
        let colors = [CYAN, VIOLET, PINK, GREEN, AMBER];
        let mut x = bar.left();
        let mut rest = used;
        let mut segs: Vec<(f64, Color32)> = top.iter().enumerate().map(|(i, g)| {
            let m = g["mem"].as_f64().unwrap_or(0.0);
            rest -= m;
            (m, colors[i])
        }).collect();
        segs.push((rest.max(0.0), rgba(255, 255, 255, 0.18)));
        for (i, (m, c)) in segs.into_iter().enumerate() {
            let w = app.a.v(("membar", i), (m / total) as f32 * bar.width(), 0.8, SMOOTH);
            bp.rect_filled(Rect::from_min_size(pos2(x, bar.top()), vec2(w, 8.0)), 0.0, c);
            x += w;
        }
        // round the bar's ends
        p.rect_stroke(bar, cr(4.0), Stroke::new(1.0, Color32::TRANSPARENT), egui::StrokeKind::Inside);
    } else {
        text(&p, pos2(ms.right(), ms.center().y), Align2::RIGHT_CENTER, "--", font(11.0), DIM, 0.0);
    }
    // list (max 476 px, scrolls)
    let f = app.apps.filter.trim().to_lowercase();
    let matches = |g: &&Value| f.is_empty() || g["name"].as_str().unwrap_or("").to_lowercase().contains(&f) || g["title"].as_str().unwrap_or("").to_lowercase().contains(&f);
    let apps_: Vec<Value> = data["apps"].as_array().into_iter().flatten().filter(matches).cloned().collect();
    let bg: Vec<Value> = data["background"].as_array().into_iter().flatten().filter(matches).cloned().collect();
    let max_mem = all.iter().map(|g| g["mem"].as_f64().unwrap_or(0.0)).fold(1.0, f64::max);
    let list_h = (2.0 * 25.0 + (apps_.len() + bg.len()) as f32 * 44.0).min(476.0);
    let (lr, _) = ui.allocate_exact_size(vec2(PAGE_W, if data.is_null() { 60.0 } else { list_h }), Sense::hover());
    if data.is_null() {
        return;
    }
    let mut lu = ui.new_child(UiBuilder::new().max_rect(Rect::from_min_max(lr.min, lr.max + vec2(8.0, 0.0))));
    egui::ScrollArea::vertical().id_salt("app-list").max_height(476.0).auto_shrink([false, false]).show(&mut lu, |ui| {
        ui.set_width(PAGE_W);
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        let clip = ui.clip_rect();
        let mut heads: Vec<(Rect, String)> = vec![];
        for (sec, list) in [("Apps", &apps_), ("Background", &bg)] {
            let (sr, _) = ui.allocate_exact_size(vec2(PAGE_W, 25.0), Sense::hover());
            heads.push((sr, format!("{} · {}", sec.to_uppercase(), list.len())));
            for g in list.iter() {
                let (r, _) = ui.allocate_exact_size(vec2(PAGE_W, 44.0), Sense::hover());
                if !clip.intersects(r) {
                    continue;
                }
                proc_row(app, ui, r, g, max_mem);
            }
        }
        // position: sticky; top: 0 — each heading sticks until the next one pushes it up
        let p = ui.painter().clone();
        for i in 0..heads.len() {
            let (r, label) = &heads[i];
            let mut y = r.top().max(clip.top());
            if let Some((next, _)) = heads.get(i + 1) {
                y = y.min(next.top() - r.height());
            }
            let band = Rect::from_min_size(pos2(r.left(), y), r.size());
            if !clip.intersects(band) {
                continue;
            }
            vgrad(&p, band, [0.0; 4], &[(0.0, rgba(8, 13, 25, 0.98)), (0.7, rgba(8, 13, 25, 0.98)), (1.0, Color32::TRANSPARENT)]);
            text(&p, pos2(band.left() + 2.0, band.center().y), Align2::LEFT_CENTER, label, font(9.5), DIM, 1.71);
        }
    });
}

fn proc_row(app: &mut App, ui: &Ui, r: Rect, g: &Value, max_mem: f64) {
    let p = ui.painter().clone();
    let name = g["name"].as_str().unwrap_or("").to_string();
    let row = hit(app, ui, r, ("proc", &name), Sense::hover());
    let hk = app.a.v(("proc_h", &name), if row.hovered() { 1.0 } else { 0.0 }, 0.2, EASE);
    fill(&p, r, 10.0, fade(rgba(255, 255, 255, 0.035), hk));
    let label = name.trim_end_matches(".exe").trim_end_matches(".EXE").to_string();
    let hue = hue_of(&name);
    let ic = Rect::from_center_size(pos2(r.left() + 8.0 + 15.0, r.center().y), vec2(28.0, 28.0));
    let h = |s: f32, l: f32| hsl(hue, s, l);
    boxed(&p, ic, 8.0, fade(h(80.0, 60.0), 0.14), fade(h(80.0, 60.0), 0.3));
    let letter = label.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or("?".into());
    text(&p, ic.center(), Align2::CENTER_CENTER, &letter, font7(12.0), h(90.0, 72.0), 0.0);
    // name column: grid 30px 1fr 150px 76px 30px, gap 10
    let x0 = ic.right() + 1.0 + 10.0;
    let kill_l = r.right() - 8.0 - 30.0;
    let mem_l = kill_l - 10.0 - 76.0;
    let bar_l = mem_l - 10.0 - 150.0;
    let nw = bar_l - 10.0 - x0;
    let count = g["count"].as_u64().unwrap_or(1);
    let title = g["title"].as_str().unwrap_or("");
    let sub = if !title.is_empty() {
        if count > 1 { format!("{title} · {count} processes") } else { title.to_string() }
    } else if count > 1 {
        format!("{count} processes")
    } else {
        format!("PID {}", g["pids"][0].as_u64().unwrap_or(0))
    };
    text_fit(&p, pos2(x0, r.center().y - 1.0), Align2::LEFT_BOTTOM, &label, font6(12.0), TEXT, 0.0, nw);
    text_fit(&p, pos2(x0, r.center().y + 1.0), Align2::LEFT_TOP, &sub, font(10.0), DIM, 0.0, nw);
    let bar = Rect::from_min_size(pos2(bar_l, r.center().y - 2.5), vec2(150.0, 5.0));
    fill(&p, bar, 3.0, rgba(255, 255, 255, 0.06));
    let mem = g["mem"].as_f64().unwrap_or(0.0);
    let w = app.a.v(("proc_w", &name), (mem / max_mem) as f32, 0.8, SMOOTH);
    if w > 0.001 {
        hgrad(&p, Rect::from_min_size(bar.min, vec2(150.0 * w, 5.0)), 3.0, &[(0.0, VIOLET), (1.0, PINK)]);
    }
    text(&p, pos2(mem_l + 76.0, r.center().y), Align2::RIGHT_CENTER, &fmt_bytes(mem, 1), font(12.0), TEXT, 0.0);
    // kill: first click arms ("END?"), second within 3 s ends the task
    let protected = g["protected"].as_bool().unwrap_or(false);
    let armed = app.apps.kill_arm.contains_key(&name);
    let kw = app.a.v(("kill_w", &name), if armed { 52.0 } else { 26.0 }, 0.2, SPRING);
    let kr = Rect::from_min_size(pos2(kill_l + 2.0 + 26.0 - kw, r.center().y - 13.0), vec2(kw, 26.0));
    let resp = hit(app, ui, kr, ("kill", &name), if protected { Sense::hover() } else { Sense::click() });
    let resp = resp.on_hover_text(if protected { "Protected system process" } else { "End task" });
    let kh = resp.hovered() && !protected;
    let mut q = p.clone();
    if protected {
        q.set_opacity(0.2 * p.opacity());
    }
    if armed {
        fill(&q, kr, 8.0, RED);
        text(&q, kr.center(), Align2::CENTER_CENTER, "END?", font7(10.0), Color32::WHITE, 0.6);
    } else {
        fill(&q, kr, 8.0, if kh { rgba(255, 77, 109, 0.12) } else { Color32::TRANSPARENT });
        icon(&q, Rect::from_center_size(kr.center(), vec2(13.0, 13.0)), icons::CROSS, 2.2, if kh { RED } else { DIM });
    }
    if resp.clicked() && !protected {
        if !armed {
            app.apps.kill_arm.insert(name.clone(), app.now + 3.0);
        } else {
            app.apps.kill_arm.remove(&name);
            let pids: Vec<u32> = g["pids"].as_array().into_iter().flatten().filter_map(|v| v.as_u64().map(|x| x as u32)).collect();
            backend::call_ctx("r:kill", json!(name), async move { crate::processes::kill(pids).await });
        }
    }
}

// ---------------- cleaner ----------------
const DEFAULT_ON: [&str; 9] = ["usertemp", "chrome", "edge", "brave", "firefox", "shader", "inet", "crash", "thumbs"];

impl App {
    pub fn scan_cache(&mut self) {
        self.cleaner.busy = true;
        self.cleaner.now = "...".into();
        backend::call("r:scan", async {
            tokio::task::spawn_blocking(|| {
                let running = crate::processes::running_names();
                Value::Array(crate::cleaner::scan(&running, |p| backend::emit("cleaner:progress", p)))
            })
            .await
            .map_err(|e| e.to_string())
        });
    }

    pub fn on_scan(&mut self, v: Value) {
        self.cleaner.busy = false;
        if v["ok"] == true {
            self.cleaner.items = v["v"].as_array().cloned().unwrap_or_default();
            self.cleaner.scanned = true;
            self.cleaner.render_gen += 1;
            if self.cleaner.selected.is_empty() {
                for i in &self.cleaner.items {
                    let id = i["id"].as_str().unwrap_or("");
                    if DEFAULT_ON.contains(&id) && i["size"].as_f64().unwrap_or(0.0) > 0.0 {
                        self.cleaner.selected.insert(id.to_string());
                    }
                }
            }
        }
    }

    pub fn on_clean(&mut self, v: Value) {
        self.cleaner.busy = false;
        self.cleaner.cleaning = false;
        if v["ok"] == true {
            let mut freed = 0.0;
            for r in v["v"].as_array().into_iter().flatten() {
                let id = r["id"].as_str().unwrap_or("").to_string();
                let f = r["freed"].as_f64().unwrap_or(0.0);
                freed += f;
                self.cleaner.freed.insert(id.clone(), f);
                if let Some(it) = self.cleaner.items.iter_mut().find(|i| i["id"] == id.as_str()) {
                    it["size"] = r["left"].clone();
                }
            }
            self.cleaner.selected.clear();
            self.cleaner.render_gen += 1;
            self.toast(format!("Freed {}", fmt_bytes(freed, 1)));
            self.say(format!("Cleaned up {}. Nice and tidy.", fmt_bytes(freed, 1)), None, 4000.0, 0.0);
        } else {
            self.toast(format!("Cleaning failed: {}", v["err"].as_str().unwrap_or("error")));
        }
    }

    fn selected_bytes(&self) -> f64 {
        self.cleaner.items.iter().filter(|i| self.cleaner.selected.contains(i["id"].as_str().unwrap_or(""))).map(|i| i["size"].as_f64().unwrap_or(0.0)).sum()
    }
}

fn cleaner(app: &mut App, ui: &mut Ui, page_rect: Rect) {
    let (head, _) = ui.allocate_exact_size(vec2(PAGE_W, 38.0), Sense::hover());
    let p = ui.painter().clone();
    let c = &app.cleaner;
    let (title, sub) = if c.scanned {
        let all: f64 = c.items.iter().map(|i| i["size"].as_f64().unwrap_or(0.0)).sum();
        (format!("{} reclaimable", fmt_bytes(all, 1)), format!("{} selected in {} locations · in-use files are skipped", fmt_bytes(app.selected_bytes(), 1), c.selected.len()))
    } else {
        ("Find reclaimable space".into(), "Scan temp files, browser caches, shader caches and more.".into())
    };
    let clean_w = 18.0 * 2.0 + text_w(&p, "CLEAN SELECTED", font7(11.0), 1.76);
    let scan_label = if c.scanned { "Rescan" } else { "Scan" };
    let scan_w = 32.0 + text_w(&p, &scan_label.to_uppercase(), font(11.0), 1.32);
    let clean_r = Rect::from_min_size(pos2(head.right() - clean_w, head.center().y - 17.0), vec2(clean_w, 34.0));
    let scan_r = Rect::from_min_size(pos2(clean_r.left() - 12.0 - scan_w, head.center().y - 16.0), vec2(scan_w, 32.0));
    let tr = text(&p, pos2(head.left(), head.top()), Align2::LEFT_TOP, &title, font6(18.0), TEXT, 0.0);
    text_fit(&p, pos2(head.left(), tr.bottom() + 2.0), Align2::LEFT_TOP, &sub, font(11.0), DIM, 0.0, scan_r.left() - 12.0 - head.left());
    let busy = c.busy || c.cleaning;
    if ghost(app, ui, &p, scan_r, "cl-scan", scan_label, !busy, false) {
        app.cleaner.freed.clear();
        app.scan_cache();
    }
    let clean_label = if app.cleaner.cleaning { "Cleaning..." } else { "Clean selected" };
    let can = !busy && !app.cleaner.selected.is_empty() && app.cleaner.scanned;
    if cta(app, ui, &p, clean_r, "cl-clean", clean_label, can, true) {
        let sel: Vec<String> = app.cleaner.items.iter().filter(|i| app.cleaner.selected.contains(i["id"].as_str().unwrap_or(""))).map(|i| i["name"].as_str().unwrap_or("").to_string()).collect();
        app.cleaner.confirm_text = (format!("Delete {}?", fmt_bytes(app.selected_bytes(), 1)), sel.join(", "));
        app.cleaner.confirm = true;
    }
    // list
    if app.cleaner.busy && !app.cleaner.scanned || (app.cleaner.busy && app.cleaner.items.is_empty()) {
        let (r, _) = ui.allocate_exact_size(vec2(PAGE_W, 100.0), Sense::hover());
        let s = text(&p, pos2(r.center().x, r.top() + 40.0), Align2::CENTER_TOP, "SCANNING", font(12.0), DIM, 1.2);
        text(&p, pos2(r.center().x, s.bottom() + 6.0), Align2::CENTER_TOP, &app.cleaner.now.clone(), font(12.0), CYAN, 0.0);
    } else {
        let items = app.cleaner.items.clone();
        let max = items.iter().map(|i| i["size"].as_f64().unwrap_or(0.0)).fold(1.0, f64::max);
        let list_h = (items.len() as f32 * 50.0).min(476.0);
        let (lr, _) = ui.allocate_exact_size(vec2(PAGE_W, list_h), Sense::hover());
        let mut lu = ui.new_child(UiBuilder::new().max_rect(Rect::from_min_max(lr.min, lr.max + vec2(8.0, 0.0))));
        egui::ScrollArea::vertical().id_salt("cl-list").max_height(476.0).auto_shrink([false, false]).show(&mut lu, |ui| {
            ui.set_width(PAGE_W - 4.0);
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            for it in items.iter() {
                let (r, _) = ui.allocate_exact_size(vec2(PAGE_W - 4.0, 50.0), Sense::hover());
                citem(app, ui, r, it, max);
            }
        });
    }
    // confirm overlay
    let show = app.cleaner.confirm;
    let o = app.a.v("cf_op", if show { 1.0 } else { 0.0 }, 0.25, EASE);
    let k = app.a.v("cf_k", if show { 1.0 } else { 0.0 }, 0.4, SPRING);
    if o > 0.01 && app.live {
        let area = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, ui.scope_id().with("cf")));
        let mut fp = area.with_clip_rect(app.shell);
        fp.set_opacity(o);
        let cover = Rect::from_min_max(pos2(page_rect.left(), page_rect.top()), pos2(page_rect.right(), app.shell.bottom()));
        fp.rect_filled(cover, cr4(0.0, 0.0, 26.0, 26.0), rgba(4, 7, 14, 0.7));
        let (title, names) = app.cleaner.confirm_text.clone();
        let s = 0.94 + 0.06 * k;
        let bw = 380.0 * s;
        let body = format!("This permanently deletes files from: ");
        let tail = "They do not go to the Recycle Bin and cannot be restored. Caches rebuild themselves; open browsers keep their in-use files.";
        let mut j = egui::text::LayoutJob::default();
        let fmt = |c: Color32, f| egui::TextFormat { font_id: f, color: c, line_height: Some(18.0), ..Default::default() };
        j.append(&body, 0.0, fmt(DIM, font(12.0)));
        j.append("\n", 0.0, fmt(DIM, font(12.0)));
        j.append(&names, 0.0, fmt(TEXT, font7(12.0)));
        j.append(".\n", 0.0, fmt(TEXT, font7(12.0)));
        j.append(tail, 0.0, fmt(DIM, font(12.0)));
        j.wrap.max_width = 340.0;
        let g = fp.layout_job(j);
        let bh = (20.0 + 20.0 + 8.0 + g.size().y + 16.0 + 34.0 + 20.0) * s;
        let br = Rect::from_center_size(cover.center(), vec2(bw, bh));
        boxed(&fp, br, 16.0, rgb(0x0d, 0x15, 0x27), rgba(255, 77, 109, 0.35));
        let tr = text(&fp, br.min + vec2(20.0, 20.0), Align2::LEFT_TOP, &title, font6(16.0), TEXT, 0.0);
        fp.galley(pos2(br.left() + 20.0, tr.bottom() + 8.0), g, DIM);
        let ok_w = 32.0 + text_w(&fp, "DELETE PERMANENTLY", font7(11.0), 1.76);
        let ok = Rect::from_min_size(pos2(br.right() - 20.0 - ok_w, br.bottom() - 20.0 - 34.0), vec2(ok_w, 34.0));
        let cw = 32.0 + text_w(&fp, "CANCEL", font(11.0), 1.32);
        let cancel = Rect::from_min_size(pos2(ok.left() - 8.0 - cw, ok.top() + 1.0), vec2(cw, 32.0));
        let mut fu = ui.new_child(UiBuilder::new().layer_id(egui::LayerId::new(egui::Order::Foreground, ui.scope_id().with("cfu"))).max_rect(cover));
        fu.set_clip_rect(app.shell);
        let _ = fu.interact(cover, fu.scope_id().with("cover"), Sense::click());
        if ghost(app, &fu, &fp, cancel, "cf-cancel", "Cancel", show, false) {
            app.cleaner.confirm = false;
        }
        if cta(app, &fu, &fp, ok, "cf-ok", "Delete permanently", show, true) {
            app.cleaner.confirm = false;
            let ids: Vec<String> = app.cleaner.selected.iter().cloned().collect();
            app.cleaner.cleaning = true;
            app.cleaner.busy = true;
            backend::call("r:clean", async move {
                tokio::task::spawn_blocking(move || Value::Array(crate::cleaner::clean(&ids, |p| backend::emit("cleaner:progress", p)))).await.map_err(|e| e.to_string())
            });
        }
    }
}

fn citem(app: &mut App, ui: &Ui, r: Rect, it: &Value, max: f64) {
    let p = ui.painter().clone();
    let id = it["id"].as_str().unwrap_or("").to_string();
    let on = app.cleaner.selected.contains(&id);
    let resp = hit(app, ui, r, ("citem", &id), Sense::click());
    let hk = app.a.v(("ci_h", &id), if resp.hovered() { 1.0 } else { 0.0 }, 0.2, EASE);
    let busy = app.cleaner.busy && app.cleaner.busy_id == id;
    let pulse_a = if busy { 0.06 * (1.0 - ((app.now * std::f64::consts::PI).cos() as f32 + 1.0) / 2.0 * 0.0 - (((app.now % 1.0) as f32 - 0.5).abs() * 2.0)) .max(0.0) } else { 0.0 };
    fill(&p, r, 12.0, over_c(rgba(62, 243, 255, pulse_a), fade(rgba(255, 255, 255, 0.035), hk)));
    // checkbox
    let ck = Rect::from_min_size(pos2(r.left() + 10.0 + 4.0, r.center().y - 9.0), vec2(18.0, 18.0));
    let ok = app.a.v(("ck", &id), if on { 1.0 } else { 0.0 }, 0.25, SPRING);
    if ok > 0.01 {
        glow(&p, ck, 6.0, 10.0, rgba(62, 243, 255, 0.45 * ok.min(1.0)));
    }
    p.rect(ck, cr(6.0), fade(CYAN, ok.clamp(0.0, 1.0)), Stroke::new(1.5, lerp_c(LINE2, CYAN, ok)), egui::StrokeKind::Inside);
    if ok > 0.3 {
        let c = ck.center() + vec2(0.0, -1.0);
        line(&p, vec![c + vec2(-4.0, 0.0), c + vec2(-1.2, 2.8), c + vec2(4.0, -2.4)], 2.0, fade(INK, ok.min(1.0)));
    }
    // name, tags and description
    let x0 = r.left() + 10.0 + 26.0 + 10.0;
    let bar_l = r.right() - 10.0 - 80.0 - 10.0 - 170.0;
    let nw = bar_l - 10.0 - x0;
    let nr = text(&p, pos2(x0, r.center().y - 1.0), Align2::LEFT_BOTTOM, it["name"].as_str().unwrap_or(""), font6(12.5), TEXT, 0.0);
    let mut tx = nr.right() + 6.0;
    for (flag, label) in [("admin", "ADMIN"), ("running", "BROWSER OPEN")] {
        if it[flag].as_bool().unwrap_or(false) {
            let g = galley(&p, label, font(9.0), AMBER, 0.72);
            let tr = Rect::from_min_size(pos2(tx, nr.center().y - g.size().y / 2.0 - 1.0), g.size() + vec2(12.0, 2.0));
            fill(&p, tr, 6.0, rgba(255, 181, 71, 0.14));
            p.galley(tr.min + vec2(6.0, 1.0), g, AMBER);
            tx = tr.right() + 6.0;
        }
    }
    let files = it["files"].as_f64().unwrap_or(0.0) as u64;
    let unit = if id == "recycle" { "items" } else { "files" };
    let desc = format!("{} · {} {unit}", it["desc"].as_str().unwrap_or(""), thousands(files));
    text_fit(&p, pos2(x0, r.center().y + 1.0), Align2::LEFT_TOP, &desc, font(10.0), DIM, 0.0, nw);
    let bar = Rect::from_min_size(pos2(bar_l, r.center().y - 2.5), vec2(170.0, 5.0));
    fill(&p, bar, 3.0, rgba(255, 255, 255, 0.06));
    let size = it["size"].as_f64().unwrap_or(0.0);
    // bars grow from 0 each time the list is rendered (renderCleaner)
    let w = app.a.v_in(("ci_w", &id, app.cleaner.render_gen), 0.0, (size / max) as f32, 1.0, 0.0, SMOOTH);
    if w > 0.001 {
        hgrad(&p, Rect::from_min_size(bar.min, vec2(170.0 * w, 5.0)), 3.0, &[(0.0, CYAN), (1.0, GREEN)]);
    }
    let freed = app.cleaner.freed.get(&id).copied().filter(|f| *f > 0.0);
    let (sz, col) = match freed {
        Some(f) => (format!("-{}", fmt_bytes(f, 1)), GREEN),
        None => (fmt_bytes(size, 1), TEXT),
    };
    text(&p, pos2(r.right() - 10.0, r.center().y), Align2::RIGHT_CENTER, &sz, font6(12.5), col, 0.0);
    if resp.clicked() && !app.cleaner.busy {
        if on {
            app.cleaner.selected.remove(&id);
        } else {
            app.cleaner.selected.insert(id);
        }
    }
}

fn over_c(top: Color32, bot: Color32) -> Color32 {
    crate::notch::over(top, bot)
}

pub fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

// ---------------- settings ----------------
fn settings(app: &mut App, ui: &mut Ui) {
    ui.spacing_mut().item_spacing.y = 6.0;
    let ai = !app.no_ai();
    let mut rows: Vec<(&str, String, String)> = vec![("openAtLogin", "Launch at startup".into(), app.os_text("Start Mission Control when you sign in to Windows."))];
    if app.is_win() {
        rows.push(("extremeMode", "Extreme mode".into(), "Theme the Windows taskbar and Start menu to match the widget. Turning it off restores your colours.".into()));
    }
    rows.push(("hideSidesUntilHover", "Show sides only on hover".into(), "Keep only the pet visible when collapsed. Hover to reveal both sides; click the pet to open Mission Control.".into()));
    rows.push(("idleOnFullscreen", "Idle during fullscreen".into(), "Hide and stop all sampling while a game or video is fullscreen.".into()));
    rows.push(("collapseOnBlur", "Collapse when clicking away".into(), "Fold back into the notch when another window gets focus.".into()));
    if app.has_ai("claude") {
        rows.push(("autoLimits", "Auto-refresh Claude limits".into(), "Every 10 min while a 5-hour window is open, using a tiny Haiku request. Chat replies always update them.".into()));
    }
    let editor = app.settings["codexEditorAvailable"].as_bool().unwrap_or(false) && (!app.has_ai("codex") || app.set_bool("codexFromEditor"));
    if editor {
        rows.push(("codexFromEditor", "Use Codex from your code editor".into(), "No Codex CLI is installed, but the OpenAI extension in VS Code / Cursor bundles one. Turn on to use it for chat and limits.".into()));
    }
    for (k, title, sub) in rows {
        let on = app.set_bool(k);
        let r = set_row(app, ui, k, &title, &sub, 38.0);
        let resp = hit(app, ui, r, ("set", k), Sense::click());
        let p = ui.painter().clone();
        switch(app, &p, Rect::from_min_size(pos2(r.right() - 14.0 - 38.0, r.center().y - 11.0), vec2(38.0, 22.0)), k, on);
        if resp.clicked() {
            app.set_setting(k, json!(!on));
        }
    }
    if ai {
        let mut sub = String::new();
        if app.has_ai("claude") {
            sub.push_str("Claude: Auto runs safe actions itself and blocks risky ones. ");
        }
        if app.has_ai("codex") {
            sub.push_str("Codex: Auto and Accept edits use its workspace-write sandbox. ");
        }
        sub.push_str("Plan is read-only.");
        let r = set_row(app, ui, "chatMode", "AI permission mode", &sub, 150.0);
        let sel = Rect::from_min_size(pos2(r.right() - 14.0 - 130.0, r.center().y - 15.0), vec2(130.0, 30.0));
        let cur = app.settings["chatMode"].as_str().unwrap_or("auto").to_string();
        if let Some(v) = select(app, ui, sel, "set-mode", &cur, &[("auto", "Auto"), ("acceptEdits", "Accept edits"), ("plan", "Plan (read-only)")], 11.5) {
            app.set_setting("chatMode", json!(v));
        }
    }
    // shortcuts
    let r = set_row(app, ui, "shortcuts", "Shortcuts", "", 0.0);
    let p = ui.painter().clone();
    let mut x = r.left() + 14.0;
    let y = r.bottom() - 8.0 - 8.0;
    let mut keys: Vec<(&[&str], &str)> = vec![(&["Ctrl", "Alt", "M"], "Mission Control")];
    if ai {
        keys.push((&["Ctrl", "Alt", "Space"], "Ask AI"));
    }
    keys.push((&["Esc"], "collapse"));
    for (ks, label) in keys {
        for k in ks {
            let g = galley(&p, k, mono(10.0), TEXT, 0.0);
            let kr = Rect::from_min_size(pos2(x + 1.0, y - g.size().y / 2.0 - 1.0), g.size() + vec2(10.0, 2.0));
            p.rect_stroke(kr, cr(4.0), Stroke::new(1.0, LINE2), egui::StrokeKind::Inside);
            p.galley(kr.min + vec2(5.0, 1.0), g, TEXT);
            x = kr.right() + 1.0;
        }
        let lr = text(&p, pos2(x + 4.0, y), Align2::LEFT_CENTER, label, font(11.0), DIM, 0.0);
        x = lr.right() + 12.0;
    }
    // quit
    let r = set_row(app, ui, "quit", "Quit Mission Control", "You can also quit from the tray icon.", 80.0);
    let b = Rect::from_min_size(pos2(r.right() - 14.0 - 64.0, r.center().y - 16.0), vec2(64.0, 32.0));
    let p = ui.painter().clone();
    if ghost(app, ui, &p, b, "quit", "Quit", true, true) {
        app.quit();
    }
}

/// A .set row; returns its rect. `ctrl_w` reserves room for the control on the right.
fn set_row(app: &App, ui: &mut Ui, k: &str, title: &str, sub: &str, ctrl_w: f32) -> Rect {
    let p = ui.painter().clone();
    let text_w_ = PAGE_W - 28.0 - if ctrl_w > 0.0 { ctrl_w + 16.0 } else { 0.0 };
    let tg = galley(&p, title, font6(13.0), TEXT, 0.0);
    let sg = if k == "shortcuts" {
        None
    } else {
        let mut j = job(sub, font(11.0), DIM, 0.0);
        j.wrap.max_width = text_w_;
        Some(p.layout_job(j))
    };
    let content_h = tg.size().y + 2.0 + sg.as_ref().map(|g| g.size().y).unwrap_or(16.0);
    let h = (content_h + 16.0).max(if ctrl_w > 0.0 { 38.0 } else { 0.0 });
    let (r, _) = ui.allocate_exact_size(vec2(PAGE_W, h), Sense::hover());
    boxed(&p, r, 12.0, BG_CARD, LINE);
    let ty = r.center().y - content_h / 2.0;
    let tgh = tg.size().y;
    p.galley(pos2(r.left() + 14.0, ty), tg, TEXT);
    if let Some(g) = sg {
        p.galley(pos2(r.left() + 14.0, ty + tgh + 2.0), g, DIM);
    }
    let _ = app;
    r
}

pub fn switch(app: &mut App, p: &Painter, r: Rect, k: &str, on: bool) {
    let t = app.a.v(("sw", k), if on { 1.0 } else { 0.0 }, 0.4, SPRING);
    let bg = app.a.color(("sw_bg", k), if on { rgba(62, 243, 255, 0.35) } else { rgba(255, 255, 255, 0.1) }, 0.3);
    fill(p, r, 11.0, bg);
    let kc = app.a.color(("sw_k", k), if on { CYAN } else { rgb(0x9f, 0xb3, 0xcc) }, 0.3);
    let c = pos2(r.left() + 3.0 + 8.0 + 16.0 * t, r.center().y);
    if on {
        p.add(egui::epaint::Shadow { offset: [0, 0], blur: 10, spread: 0, color: fade(CYAN, 0.8) }.as_shape(Rect::from_center_size(c, vec2(16.0, 16.0)), cr(8.0)));
    }
    p.circle_filled(c, 8.0, kc);
}

/// A <select>: the closed control plus a popup list. Returns the newly chosen value.
pub fn select(app: &mut App, ui: &Ui, r: Rect, id: &str, cur: &str, opts: &[(&str, &str)], size: f32) -> Option<String> {
    let p = ui.painter().clone();
    let resp = hit(app, ui, r, ("select", id), Sense::click());
    boxed(&p, r, 8.0, SELECT_BG, if resp.hovered() { rgba(62, 243, 255, 0.45) } else { LINE2 });
    let label = opts.iter().find(|(v, _)| *v == cur).map(|(_, l)| *l).unwrap_or(cur);
    text_fit(&p, pos2(r.left() + 8.0, r.center().y), Align2::LEFT_CENTER, label, font(size), TEXT, 0.0, r.width() - 28.0);
    icon(&p, Rect::from_center_size(pos2(r.right() - 11.0, r.center().y), vec2(12.0, 12.0)), icons::CARET_DOWN, 0.0, TEXT);
    let mut out = None;
    egui::Popup::menu(&resp).width(r.width()).show(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        for (v, l) in opts {
            let sel = *v == cur;
            if ui.add(egui::Button::selectable(sel, egui::RichText::new(*l).font(font(size)).color(if sel { CYAN } else { TEXT })).min_size(vec2(r.width() - 12.0, 24.0))).clicked() {
                out = Some(v.to_string());
                ui.close();
            }
        }
    });
    out
}
