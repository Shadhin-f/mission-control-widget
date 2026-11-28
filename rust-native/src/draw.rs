// Painting helpers that give egui's painter what the CSS uses: gradients clipped to rounded shapes,
// glows (box-shadow / drop-shadow), letter-spaced text, and SVG icons drawn as vectors.
use crate::theme::*;
use eframe::egui::{
    self, epaint, pos2, text::LayoutJob, vec2, Align2, Color32, CornerRadius, FontId, Galley, Mesh, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, TextFormat, Vec2,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

pub fn cr(r: f32) -> CornerRadius {
    CornerRadius::same(r.round().clamp(0.0, 255.0) as u8)
}
pub fn cr4(tl: f32, tr: f32, br: f32, bl: f32) -> CornerRadius {
    let u = |v: f32| v.round().clamp(0.0, 255.0) as u8;
    CornerRadius { nw: u(tl), ne: u(tr), se: u(br), sw: u(bl) }
}

/// background + border, like a CSS box with `border: 1px solid`.
/// Rounds a rect's edges to physical pixels so 1 px borders stay sharp.
pub fn snap(p: &Painter, r: Rect) -> Rect {
    let k = p.pixels_per_point();
    let s = |v: f32| (v * k).round() / k;
    Rect::from_min_max(pos2(s(r.min.x), s(r.min.y)), pos2(s(r.max.x), s(r.max.y)))
}
pub fn snap_pos(p: &Painter, q: Pos2) -> Pos2 {
    let k = p.pixels_per_point();
    pos2((q.x * k).round() / k, (q.y * k).round() / k)
}

pub fn boxed(p: &Painter, r: Rect, radius: f32, fill: Color32, border: Color32) {
    p.rect(snap(p, r), cr(radius), fill, if border.a() > 0 { Stroke::new(1.0, border) } else { Stroke::NONE }, StrokeKind::Inside);
}
pub fn fill(p: &Painter, r: Rect, radius: f32, fill: Color32) {
    p.rect_filled(snap(p, r), cr(radius), fill);
}

/// box-shadow: offset, blur, spread, colour.
pub fn shadow(p: &Painter, r: Rect, radius: f32, offset: Vec2, blur: f32, spread: f32, color: Color32) {
    if color.a() == 0 {
        return;
    }
    let s = epaint::Shadow { offset: [offset.x.round() as i8, offset.y.round() as i8], blur: blur.round().clamp(0.0, 255.0) as u8, spread: spread.round().clamp(0.0, 255.0) as u8, color };
    p.add(s.as_shape(r, cr(radius)));
}

/// A glow around a rounded box: box-shadow 0 0 blur colour.
pub fn glow(p: &Painter, r: Rect, radius: f32, blur: f32, color: Color32) {
    shadow(p, r, radius, Vec2::ZERO, blur, 0.0, color);
}

// ---------------- gradients clipped to rounded rectangles ----------------

fn corner_y(x: f32, l: f32, r: f32, edge: f32, rl: f32, rr: f32, down: bool) -> f32 {
    // y of the top (down = true) or bottom edge at x, following the corner arcs
    let off = |rad: f32, dx: f32| rad - (rad * rad - dx * dx).max(0.0).sqrt();
    let d = if x < l + rl && rl > 0.0 {
        off(rl, l + rl - x)
    } else if x > r - rr && rr > 0.0 {
        off(rr, x - (r - rr))
    } else {
        0.0
    };
    if down {
        edge + d
    } else {
        edge - d
    }
}

/// Fills a rounded rectangle with any colour function (gradients). Radii: top-left, top-right,
/// bottom-right, bottom-left. `rows` subdivides vertically for vertical/radial gradients.
pub fn fill_with(p: &Painter, rect: Rect, rad: [f32; 4], rows: usize, color: impl Fn(Pos2) -> Color32) {
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }
    let maxr = rect.width().min(rect.height()) / 2.0;
    let [tl, tr, br, bl] = rad.map(|r| r.min(maxr).max(0.0));
    let feather = 1.0 / p.ctx().pixels_per_point();
    let (l, r) = (rect.left(), rect.right());
    let mut xs: Vec<f32> = Vec::new();
    let cols = ((rect.width() / 24.0).ceil() as usize).clamp(2, 48);
    for i in 0..=cols {
        xs.push(l + rect.width() * i as f32 / cols as f32);
    }
    for (rad, left) in [(tl.max(bl), true), (tr.max(br), false)] {
        if rad > 0.0 {
            for i in 1..10 {
                let a = std::f32::consts::FRAC_PI_2 * i as f32 / 10.0;
                let dx = rad * (1.0 - a.cos());
                xs.push(if left { l + dx } else { r - dx });
            }
        }
    }
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    xs.dedup_by(|a, b| (*a - *b).abs() < 0.05);
    let rows = rows.max(1);
    let mut mesh = Mesh::default();
    let mut prev: Option<u32> = None;
    for &x in &xs {
        let top = corner_y(x, l, r, rect.top(), tl, tr, true);
        let bot = corner_y(x, l, r, rect.bottom(), bl, br, false);
        let base = mesh.vertices.len() as u32;
        // feather above, rows+1 inner points, feather below
        let c_top = color(pos2(x, top));
        mesh.colored_vertex(pos2(x, top - feather), Color32::TRANSPARENT);
        for k in 0..=rows {
            let y = top + (bot - top) * k as f32 / rows as f32;
            mesh.colored_vertex(pos2(x, y), if k == 0 { c_top } else { color(pos2(x, y)) });
        }
        let c_bot = color(pos2(x, bot));
        mesh.colored_vertex(pos2(x, bot + feather), fade(c_bot, 0.0));
        let n = rows as u32 + 3;
        if let Some(pb) = prev {
            for k in 0..n - 1 {
                mesh.add_triangle(pb + k, base + k, pb + k + 1);
                mesh.add_triangle(base + k, base + k + 1, pb + k + 1);
            }
        }
        prev = Some(base);
    }
    p.add(Shape::mesh(mesh));
}

/// Multi-stop gradient helper: stops are (offset 0..1, colour).
pub fn stops_at(stops: &[(f32, Color32)], t: f32) -> Color32 {
    if t <= stops[0].0 {
        return stops[0].1;
    }
    for w in stops.windows(2) {
        if t <= w[1].0 {
            let k = (t - w[0].0) / (w[1].0 - w[0].0).max(1e-6);
            return lerp_c(w[0].1, w[1].1, k);
        }
    }
    stops[stops.len() - 1].1
}

pub fn hgrad(p: &Painter, rect: Rect, radius: f32, stops: &[(f32, Color32)]) {
    let (l, w) = (rect.left(), rect.width().max(1.0));
    fill_with(p, rect, [radius; 4], 1, |q| stops_at(stops, (q.x - l) / w));
}
pub fn vgrad(p: &Painter, rect: Rect, rad: [f32; 4], stops: &[(f32, Color32)]) {
    let (t, h) = (rect.top(), rect.height().max(1.0));
    fill_with(p, rect, rad, 6, |q| stops_at(stops, (q.y - t) / h));
}
/// linear-gradient(135deg, a, b)
pub fn diag(p: &Painter, rect: Rect, radius: f32, a: Color32, b: Color32) {
    let (o, s) = (rect.min, rect.size());
    let len = s.x + s.y;
    fill_with(p, rect, [radius; 4], 4, |q| lerp_c(a, b, ((q.x - o.x) + (q.y - o.y)) / len.max(1.0)));
}

/// A radial "glow" disc: colour at the centre fading to transparent at radius `r` (stop at 0 and r).
pub fn radial(p: &Painter, c: Pos2, r: f32, color: Color32) {
    radial_stops(p, c, r, &[(0.0, color), (1.0, Color32::TRANSPARENT)]);
}
pub fn radial_stops(p: &Painter, c: Pos2, r: f32, stops: &[(f32, Color32)]) {
    radial_ellipse(p, c, vec2(r, r), stops);
}
pub fn radial_ellipse(p: &Painter, c: Pos2, rad: Vec2, stops: &[(f32, Color32)]) {
    let segs = 48;
    let rings = 12;
    let mut mesh = Mesh::default();
    mesh.colored_vertex(c, stops_at(stops, 0.0));
    for k in 1..=rings {
        let t = k as f32 / rings as f32;
        let col = stops_at(stops, t);
        for i in 0..segs {
            let a = std::f32::consts::TAU * i as f32 / segs as f32;
            mesh.colored_vertex(c + vec2(a.cos() * rad.x * t, a.sin() * rad.y * t), col);
        }
    }
    for i in 0..segs as u32 {
        let j = (i + 1) % segs as u32;
        mesh.add_triangle(0, 1 + i, 1 + j);
    }
    for k in 1..rings as u32 {
        let (a, b) = (1 + (k - 1) * segs as u32, 1 + k * segs as u32);
        for i in 0..segs as u32 {
            let j = (i + 1) % segs as u32;
            mesh.add_triangle(a + i, b + i, a + j);
            mesh.add_triangle(b + i, b + j, a + j);
        }
    }
    p.add(Shape::mesh(mesh));
}

// ---------------- lines ----------------

/// A polyline with round caps.
pub fn line(p: &Painter, pts: Vec<Pos2>, w: f32, color: Color32) {
    if pts.len() < 2 || color.a() == 0 {
        return;
    }
    let (a, b) = (pts[0], pts[pts.len() - 1]);
    p.add(Shape::line(pts, Stroke::new(w, color)));
    if w >= 1.2 {
        p.circle_filled(a, w / 2.0, color);
        p.circle_filled(b, w / 2.0, color);
    }
}

/// A stroke with a soft glow behind it (drop-shadow / shadowBlur).
pub fn glow_line(p: &Painter, pts: &[Pos2], w: f32, color: Color32, blur: f32) {
    if blur > 0.0 {
        for (extra, a) in [(blur * 1.6, 0.07), (blur * 0.9, 0.13), (blur * 0.4, 0.22)] {
            p.add(Shape::line(pts.to_vec(), Stroke::new(w + extra, fade(color, a))));
        }
    }
    line(p, pts.to_vec(), w, color);
}

pub fn arc_pts(c: Pos2, r: f32, a0: f32, a1: f32) -> Vec<Pos2> {
    let n = (((a1 - a0).abs() * r / 3.0).ceil() as usize).clamp(2, 200);
    (0..=n).map(|i| {
        let a = a0 + (a1 - a0) * i as f32 / n as f32;
        c + vec2(a.cos(), a.sin()) * r
    }).collect()
}

/// A thick stroke whose colour varies along its length (SVG stroke with a gradient).
pub fn stroke_colored(p: &Painter, pts: &[Pos2], cols: &[Color32], w: f32) {
    if pts.len() < 2 {
        return;
    }
    let feather = 1.0 / p.ctx().pixels_per_point();
    let mut mesh = Mesh::default();
    let n = pts.len();
    for i in 0..n {
        let d = if i == 0 { pts[1] - pts[0] } else if i == n - 1 { pts[n - 1] - pts[n - 2] } else { pts[i + 1] - pts[i - 1] };
        let nrm = vec2(-d.y, d.x).normalized();
        let h = w / 2.0;
        let c = cols[i];
        mesh.colored_vertex(pts[i] + nrm * (h + feather), Color32::TRANSPARENT);
        mesh.colored_vertex(pts[i] + nrm * h, c);
        mesh.colored_vertex(pts[i] - nrm * h, c);
        mesh.colored_vertex(pts[i] - nrm * (h + feather), Color32::TRANSPARENT);
        if i > 0 {
            let (a, b) = ((i as u32 - 1) * 4, i as u32 * 4);
            for k in 0..3 {
                mesh.add_triangle(a + k, b + k, a + k + 1);
                mesh.add_triangle(b + k, b + k + 1, a + k + 1);
            }
        }
    }
    p.add(Shape::mesh(mesh));
    p.circle_filled(pts[0], w / 2.0, cols[0]);
    p.circle_filled(pts[n - 1], w / 2.0, cols[n - 1]);
}

// ---------------- text ----------------

pub fn job(text: &str, font: FontId, color: Color32, spacing: f32) -> LayoutJob {
    let mut j = LayoutJob::default();
    j.append(text, 0.0, TextFormat { font_id: font, color, extra_letter_spacing: spacing, ..Default::default() });
    j
}

pub fn galley(p: &Painter, text: &str, font: FontId, color: Color32, spacing: f32) -> Arc<Galley> {
    p.layout_job(job(text, font, color, spacing))
}

/// Single-line text, cut with an ellipsis to `max_w` when given.
pub fn galley_fit(p: &Painter, text: &str, font: FontId, color: Color32, spacing: f32, max_w: f32) -> Arc<Galley> {
    let mut j = job(text, font, color, spacing);
    j.wrap.max_width = max_w.max(1.0);
    j.wrap.max_rows = 1;
    j.wrap.break_anywhere = true;
    j.wrap.overflow_character = Some('…');
    p.layout_job(j)
}

pub fn text_w(p: &Painter, text: &str, font: FontId, spacing: f32) -> f32 {
    galley(p, text, font, Color32::WHITE, spacing).size().x
}

/// Places text by an anchor; letter-spacing like CSS (the last letter's spacing is kept, as browsers do).
pub fn text(p: &Painter, pos: Pos2, anchor: Align2, s: &str, font: FontId, color: Color32, spacing: f32) -> Rect {
    let g = galley(p, s, font, color, spacing);
    put(p, pos, anchor, g, color)
}

pub fn text_fit(p: &Painter, pos: Pos2, anchor: Align2, s: &str, font: FontId, color: Color32, spacing: f32, max_w: f32) -> Rect {
    let g = galley_fit(p, s, font, color, spacing, max_w);
    put(p, pos, anchor, g, color)
}

pub fn put(p: &Painter, pos: Pos2, anchor: Align2, g: Arc<Galley>, color: Color32) -> Rect {
    let r = anchor.anchor_size(pos, g.size());
    let at = snap_pos(p, r.min);
    p.galley(at, g, color);
    Rect::from_min_size(at, r.size())
}

// ---------------- SVG icons ----------------
// The icons are the renderer's inline SVGs (24x24 view box), stroked as vectors at any scale.

#[derive(Clone, Copy)]
pub enum Prim {
    P(&'static str),
    /// x, y, w, h, rx
    R(f32, f32, f32, f32, f32),
    /// cx, cy, r
    C(f32, f32, f32),
    /// filled circle (class="dot")
    Dot(f32, f32, f32),
    /// filled path
    F(&'static str),
}

thread_local! {
    static PATHS: RefCell<HashMap<usize, Arc<Vec<(Vec<Pos2>, bool)>>>> = RefCell::new(HashMap::new());
}

/// Parses SVG path data into polylines (M L H V A C Z, absolute and relative). Returns (points, closed).
pub fn parse_path(d: &'static str) -> Arc<Vec<(Vec<Pos2>, bool)>> {
    let k = d.as_ptr() as usize ^ d.len();
    if let Some(v) = PATHS.with(|m| m.borrow().get(&k).cloned()) {
        return v;
    }
    let v = Arc::new(parse_path_uncached(d));
    PATHS.with(|m| m.borrow_mut().insert(k, v.clone()));
    v
}

fn parse_path_uncached(d: &str) -> Vec<(Vec<Pos2>, bool)> {
    let mut toks: Vec<Tok> = Vec::new();
    let b = d.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i] as char;
        if c.is_ascii_alphabetic() {
            toks.push(Tok::C(c));
            i += 1;
        } else if c == '-' || c == '.' || c.is_ascii_digit() {
            let s = i;
            let mut seen_dot = c == '.';
            i += 1;
            while i < b.len() {
                let ch = b[i] as char;
                if ch.is_ascii_digit() {
                    i += 1;
                } else if ch == '.' && !seen_dot {
                    seen_dot = true;
                    i += 1;
                } else {
                    break;
                }
            }
            toks.push(Tok::N(d[s..i].parse().unwrap_or(0.0)));
        } else {
            i += 1;
        }
    }
    let mut out: Vec<(Vec<Pos2>, bool)> = Vec::new();
    let mut cur = pos2(0.0, 0.0);
    let mut start = cur;
    let mut cmd = 'M';
    let mut poly: Vec<Pos2> = Vec::new();
    let mut idx = 0;
    let num = |idx: &mut usize| -> Option<f32> {
        if let Some(Tok::N(n)) = toks.get(*idx) {
            *idx += 1;
            Some(*n)
        } else {
            None
        }
    };
    while idx < toks.len() {
        if let Tok::C(c) = toks[idx] {
            cmd = c;
            idx += 1;
            if c == 'z' || c == 'Z' {
                if !poly.is_empty() {
                    poly.push(start);
                    out.push((std::mem::take(&mut poly), true));
                }
                cur = start;
                continue;
            }
        }
        let rel = cmd.is_ascii_lowercase();
        let base = if rel { cur } else { pos2(0.0, 0.0) };
        match cmd.to_ascii_uppercase() {
            'M' => {
                let (Some(x), Some(y)) = (num(&mut idx), num(&mut idx)) else { break };
                if poly.len() > 1 {
                    out.push((std::mem::take(&mut poly), false));
                }
                poly.clear();
                cur = pos2(base.x + x, base.y + y);
                start = cur;
                poly.push(cur);
                cmd = if rel { 'l' } else { 'L' };
            }
            'L' => {
                let (Some(x), Some(y)) = (num(&mut idx), num(&mut idx)) else { break };
                cur = pos2(base.x + x, base.y + y);
                poly.push(cur);
            }
            'H' => {
                let Some(x) = num(&mut idx) else { break };
                cur = pos2(if rel { cur.x + x } else { x }, cur.y);
                poly.push(cur);
            }
            'V' => {
                let Some(y) = num(&mut idx) else { break };
                cur = pos2(cur.x, if rel { cur.y + y } else { y });
                poly.push(cur);
            }
            'C' => {
                let v: Vec<f32> = (0..6).filter_map(|_| num(&mut idx)).collect();
                if v.len() < 6 {
                    break;
                }
                let (p1, p2, p3) = (pos2(base.x + v[0], base.y + v[1]), pos2(base.x + v[2], base.y + v[3]), pos2(base.x + v[4], base.y + v[5]));
                let p0 = cur;
                for s in 1..=12 {
                    let t = s as f32 / 12.0;
                    let u = 1.0 - t;
                    let q = p0.to_vec2() * (u * u * u) + p1.to_vec2() * (3.0 * u * u * t) + p2.to_vec2() * (3.0 * u * t * t) + p3.to_vec2() * (t * t * t);
                    poly.push(q.to_pos2());
                }
                cur = p3;
            }
            'A' => {
                let v: Vec<f32> = (0..7).filter_map(|_| num(&mut idx)).collect();
                if v.len() < 7 {
                    break;
                }
                let end = pos2(base.x + v[5], base.y + v[6]);
                arc_to(&mut poly, cur, v[0], v[1], v[2], v[3] != 0.0, v[4] != 0.0, end);
                cur = end;
            }
            _ => {
                idx += 1;
            }
        }
    }
    if poly.len() > 1 {
        out.push((poly, false));
    } else if poly.len() == 1 {
        // "M12 19.2h.01"-style dots
        out.push((vec![poly[0], poly[0] + vec2(0.01, 0.0)], false));
    }
    out
}

enum Tok {
    C(char),
    N(f32),
}

/// SVG elliptical arc (endpoint parameterisation) to points.
#[allow(clippy::too_many_arguments)]
fn arc_to(poly: &mut Vec<Pos2>, p0: Pos2, rx: f32, ry: f32, rot: f32, large: bool, sweep: bool, p1: Pos2) {
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    if rx < 1e-4 || ry < 1e-4 {
        poly.push(p1);
        return;
    }
    let phi = rot.to_radians();
    let (cp, sp) = (phi.cos(), phi.sin());
    let dx = (p0.x - p1.x) / 2.0;
    let dy = (p0.y - p1.y) / 2.0;
    let x1 = cp * dx + sp * dy;
    let y1 = -sp * dx + cp * dy;
    let lam = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
    if lam > 1.0 {
        let s = lam.sqrt();
        rx *= s;
        ry *= s;
    }
    let num = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
    let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let mut co = (num / den).max(0.0).sqrt();
    if large == sweep {
        co = -co;
    }
    let cxp = co * rx * y1 / ry;
    let cyp = -co * ry * x1 / rx;
    let cx = cp * cxp - sp * cyp + (p0.x + p1.x) / 2.0;
    let cy = sp * cxp + cp * cyp + (p0.y + p1.y) / 2.0;
    let ang = |ux: f32, uy: f32, vx: f32, vy: f32| {
        let a = (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
        a
    };
    let t1 = ang(1.0, 0.0, (x1 - cxp) / rx, (y1 - cyp) / ry);
    let mut dt = ang((x1 - cxp) / rx, (y1 - cyp) / ry, (-x1 - cxp) / rx, (-y1 - cyp) / ry);
    if !sweep && dt > 0.0 {
        dt -= std::f32::consts::TAU;
    } else if sweep && dt < 0.0 {
        dt += std::f32::consts::TAU;
    }
    let n = ((dt.abs() * rx.max(ry)) / 1.2).ceil().clamp(4.0, 64.0) as usize;
    for i in 1..=n {
        let t = t1 + dt * i as f32 / n as f32;
        let (x, y) = (rx * t.cos(), ry * t.sin());
        poly.push(pos2(cp * x - sp * y + cx, sp * x + cp * y + cy));
    }
}

/// Draws an icon (24-unit view box) into `rect`. Stroke width is in view-box units, like SVG.
pub fn icon(p: &Painter, rect: Rect, prims: &[Prim], sw: f32, color: Color32) {
    let s = rect.width() / 24.0;
    let o = rect.min.to_vec2();
    let tf = |q: Pos2| pos2(q.x * s, q.y * s) + o;
    let w = sw * s;
    for pr in prims {
        match *pr {
            Prim::P(d) => {
                for (pts, closed) in parse_path(d).iter() {
                    let pts: Vec<Pos2> = pts.iter().map(|q| tf(*q)).collect();
                    if *closed {
                        p.add(Shape::closed_line(pts, Stroke::new(w, color)));
                    } else {
                        line(p, pts, w, color);
                    }
                }
            }
            Prim::F(d) => {
                for (pts, _) in parse_path(d).iter() {
                    let pts: Vec<Pos2> = pts.iter().map(|q| tf(*q)).collect();
                    fill_star(p, &pts, color);
                }
            }
            Prim::R(x, y, rw, rh, rx) => {
                let r = Rect::from_min_size(tf(pos2(x, y)), vec2(rw * s, rh * s));
                p.rect_stroke(r, cr(rx * s), Stroke::new(w, color), StrokeKind::Middle);
            }
            Prim::C(cx, cy, r) => {
                p.circle_stroke(tf(pos2(cx, cy)), r * s, Stroke::new(w, color));
            }
            Prim::Dot(cx, cy, r) => {
                p.circle_filled(tf(pos2(cx, cy)), r * s, color);
            }
        }
    }
}

/// Fills a polygon that is star-shaped around its centroid (the spark, play and send glyphs), with
/// an anti-aliased rim.
pub fn fill_star(p: &Painter, pts: &[Pos2], color: Color32) {
    let mut pts = pts.to_vec();
    if pts.len() > 2 && pts[0].distance(pts[pts.len() - 1]) < 0.01 {
        pts.pop();
    }
    if pts.len() < 3 {
        return;
    }
    let c = pts.iter().fold(Vec2::ZERO, |a, q| a + q.to_vec2()) / pts.len() as f32;
    let c = c.to_pos2();
    let feather = 0.8 / p.ctx().pixels_per_point();
    let mut mesh = Mesh::default();
    mesh.colored_vertex(c, color);
    let n = pts.len() as u32;
    for q in &pts {
        let d = (*q - c).normalized();
        mesh.colored_vertex(*q - d * feather * 0.5, color);
        mesh.colored_vertex(*q + d * feather, Color32::TRANSPARENT);
    }
    for i in 0..n {
        let j = (i + 1) % n;
        let (a, b, a2, b2) = (1 + i * 2, 1 + j * 2, 2 + i * 2, 2 + j * 2);
        mesh.add_triangle(0, a, b);
        mesh.add_triangle(a, a2, b);
        mesh.add_triangle(a2, b2, b);
    }
    p.add(Shape::mesh(mesh));
}

/// icon + filter: drop-shadow(0 0 blur colour)
pub fn icon_glow(p: &Painter, rect: Rect, prims: &[Prim], sw: f32, color: Color32, blur: f32) {
    let s = rect.width() / 24.0;
    let spread = blur / s.max(0.01);
    icon(p, rect, prims, sw + spread * 1.2, fade(color, 0.10));
    icon(p, rect, prims, sw + spread * 0.55, fade(color, 0.18));
    icon(p, rect, prims, sw, color);
}

/// Mouse-wheel movement this frame (positive = up), read from the raw events so scroll areas can't eat it.
pub fn wheel(ui: &egui::Ui) -> f32 {
    ui.input(|i| i.raw.events.iter().filter_map(|e| if let egui::Event::MouseWheel { delta, .. } = e { Some(delta.y) } else { None }).sum())
}
