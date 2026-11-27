// The AI chat (Claude Code / Codex): streamed replies with light markdown, tool rows, typing dots,
// provider switch and permission mode, and the composer.
use crate::app::*;
use crate::draw::*;
use crate::icons;
use crate::pages::{hit, select};
use crate::theme::*;
use eframe::egui::{self, pos2, text::LayoutJob, vec2, Align2, Color32, Rect, Sense, Stroke, TextFormat, Ui, UiBuilder};
use serde_json::{json, Value};

pub enum Part {
    Text(String),
    Tool { id: String, name: String, detail: String, state: u8 },
    Denied(String),
    Meta(String),
}

#[derive(PartialEq)]
pub enum Kind {
    User,
    Bot,
    Err,
}

pub struct Msg {
    pub kind: Kind,
    pub parts: Vec<Part>,
    pub typing: bool,
    pub id: u64,
    streamed: bool,
}

#[derive(Default)]
pub struct Chat {
    pub msgs: Vec<Msg>,
    pub input: String,
    pub model: String,
    pub focus_at: Option<f64>,
    cur: Option<usize>,
    empty_text: Option<String>,
    next_id: u64,
    scroll_down: bool,
}

const SUGS: [&str; 4] = ["What is using most of my RAM?", "Open my Downloads folder", "Switch Windows to dark mode", "Show my IP address and Wi-Fi details"];

impl Chat {
    fn push(&mut self, kind: Kind, parts: Vec<Part>, typing: bool) -> usize {
        self.next_id += 1;
        self.msgs.push(Msg { kind, parts, typing, id: self.next_id, streamed: false });
        self.scroll_down = true;
        self.msgs.len() - 1
    }
}

impl App {
    fn ai_name(&self) -> &'static str {
        if self.provider == Some("codex") {
            "Codex"
        } else {
            "Claude"
        }
    }

    pub fn send_chat(&mut self, text: &str) {
        let text = text.trim().to_string();
        if text.is_empty() || self.ai_busy {
            return;
        }
        let Some(prov) = self.provider else { return };
        self.chat.push(Kind::User, vec![Part::Text(text.clone())], false);
        self.chat.input.clear();
        let i = self.chat.push(Kind::Bot, vec![], true);
        self.chat.cur = Some(i);
        self.ai_busy = true;
        let mode = self.settings["chatMode"].as_str().unwrap_or("auto").to_string();
        self.be.ai_send(prov, text, mode);
    }

    fn new_chat(&mut self, text: String) {
        self.be.claude.reset();
        self.be.codex.reset();
        self.chat.msgs.clear();
        self.chat.cur = None;
        self.chat.empty_text = Some(text);
        self.ai_busy = false;
    }

    pub fn on_ai(&mut self, e: Value) {
        let kind = e["kind"].as_str().unwrap_or("");
        if kind == "init" {
            self.chat.model = e["model"].as_str().unwrap_or("").to_string();
            return;
        }
        let Some(ci) = self.chat.cur else {
            return;
        };
        self.chat.scroll_down = true;
        match kind {
            "segment" => {
                let m = &mut self.chat.msgs[ci];
                if matches!(m.parts.last(), Some(Part::Text(t)) if !t.trim().is_empty()) {
                    m.parts.push(Part::Text(String::new()));
                }
            }
            "delta" => {
                let m = &mut self.chat.msgs[ci];
                m.typing = false;
                m.streamed = true;
                let t = e["text"].as_str().unwrap_or("");
                match m.parts.last_mut() {
                    Some(Part::Text(s)) => s.push_str(t),
                    _ => m.parts.push(Part::Text(t.to_string())),
                }
            }
            "tool" => {
                let m = &mut self.chat.msgs[ci];
                m.typing = false;
                m.parts.push(Part::Tool { id: e["id"].as_str().unwrap_or("").to_string(), name: e["name"].as_str().unwrap_or("").to_string(), detail: e["detail"].as_str().unwrap_or("").to_string(), state: 0 });
            }
            "tool_result" => {
                let id = e["id"].as_str().unwrap_or("");
                let err = e["error"].as_bool().unwrap_or(false);
                for p in self.chat.msgs[ci].parts.iter_mut() {
                    if let Part::Tool { id: tid, state, .. } = p {
                        if tid == id {
                            *state = if err { 2 } else { 1 };
                        }
                    }
                }
            }
            "result" => {
                let text = e["text"].as_str().unwrap_or("").to_string();
                let mode = self.settings["chatMode"].as_str().unwrap_or("auto").to_string();
                let m = &mut self.chat.msgs[ci];
                m.typing = false;
                if !m.streamed && !text.is_empty() {
                    m.parts.push(Part::Text(text.clone()));
                }
                if let Some(d) = e["denied"].as_array().filter(|d| !d.is_empty()) {
                    let list: Vec<String> = d.iter().map(|x| x.as_str().unwrap_or("").to_string()).collect();
                    m.parts.push(Part::Denied(format!("Blocked by the \"{mode}\" permission mode:\n{}", list.join("\n"))));
                }
                let ms = e["ms"].as_f64().unwrap_or(0.0);
                let cost = e["cost"].as_f64().unwrap_or(0.0);
                let meta: Vec<String> = [(ms > 0.0).then(|| format!("{:.1}s", ms / 1000.0)), (cost > 0.0).then(|| format!("${cost:.3}"))].into_iter().flatten().collect();
                m.parts.push(Part::Meta(meta.join(" · ")));
                for p in m.parts.iter_mut() {
                    if let Part::Tool { state, .. } = p {
                        if *state == 0 {
                            *state = 1;
                        }
                    }
                }
                if self.mode != Mode::Chat {
                    let who = if e["provider"] == "codex" { "Codex" } else { "Claude" };
                    let flat: String = if text.is_empty() { "done".into() } else { text.split_whitespace().collect::<Vec<_>>().join(" ") };
                    let cut: String = flat.chars().take(90).collect();
                    let more = if flat.chars().count() > 90 { "..." } else { "" };
                    self.say(format!("{who}: {cut}{more}"), None, 6000.0, 0.0);
                }
            }
            "error" => {
                self.chat.msgs[ci].typing = false;
                let t = e["text"].as_str().unwrap_or("").to_string();
                self.chat.push(Kind::Err, vec![Part::Text(t)], false);
            }
            "end" => {
                self.chat.msgs[ci].typing = false;
                self.chat.cur = None;
                self.ai_busy = false;
            }
            _ => {}
        }
    }
}

pub fn draw(app: &mut App, ui: &mut Ui, shell: Rect, hits: &mut Vec<Rect>) {
    let on = app.mode == Mode::Chat;
    let op = app.a.vd("chat_op", if on { 1.0 } else { 0.0 }, if on { 0.35 } else { 0.18 }, if on { 0.16 } else { 0.0 }, EASE);
    let ty = app.a.vd("chat_ty", if on { 0.0 } else { -10.0 }, if on { 0.55 } else { 0.3 }, if on { 0.12 } else { 0.0 }, if on { SPRING } else { SMOOTH });
    if op < 0.005 {
        return;
    }
    if on {
        hits.push(shell);
    }
    let area = Rect::from_min_size(pos2(CX - 370.0, BAR_H + ty), vec2(740.0, 554.0));
    let clip = Rect::from_min_max(pos2(shell.left(), BAR_H), shell.max);
    let mut root = ui.new_child(UiBuilder::new().max_rect(area).id_salt("chat"));
    root.set_clip_rect(clip);
    root.set_opacity(op);
    let was = app.live;
    app.live = on;
    let codex = app.provider == Some("codex");
    let accent = if codex { GREEN } else { ORANGE };
    let p = root.painter().clone();

    // header
    let hdr = Rect::from_min_max(pos2(area.left() + 16.0, area.top()), pos2(area.right() - 16.0, area.top() + 44.0));
    p.line_segment([pos2(hdr.left(), hdr.bottom() - 0.5), pos2(hdr.right(), hdr.bottom() - 0.5)], Stroke::new(1.0, LINE));
    let sp = Rect::from_center_size(pos2(hdr.left() + 7.0, hdr.center().y), vec2(14.0, 14.0));
    radial(&p, sp.center(), 12.0, fade(accent, 0.35));
    icon(&p, sp, icons::SPARK, 0.0, accent);
    let name = if codex { "Codex" } else { "Claude Code" };
    let nr = text(&p, pos2(sp.right() + 8.0, hdr.center().y), Align2::LEFT_CENTER, name, font6(13.0), TEXT, 0.78);
    let model = if app.chat.model.is_empty() { app.os_text("PC control") } else { app.chat.model.clone() };
    text(&p, pos2(nr.right() + 8.0, nr.bottom() - 2.0), Align2::LEFT_BOTTOM, &model, font(10.0), DIM, 0.8);
    // tools, right to left
    let mut x = hdr.right();
    let tools: [(&str, &[Prim], &str, f32); 3] = [("collapse", icons::CHEVRON_UP, "Collapse (Esc)", 1.9), ("panel", icons::DIAMOND, "Mission Control", 1.8), ("new", icons::RELOAD, "New conversation", 1.9)];
    for (k, ico, tip, sw) in tools {
        let b = Rect::from_min_size(pos2(x - 28.0, hdr.center().y - 14.0), vec2(28.0, 28.0));
        x -= 32.0;
        let resp = hit(app, &root, b, ("chat-tool", k), Sense::click());
        let hk = app.a.v(("ct_h", k), if resp.hovered() { 1.0 } else { 0.0 }, 0.2, EASE);
        fill(&p, b, 8.0, fade(rgba(255, 255, 255, 0.06), hk));
        icon(&p, Rect::from_center_size(b.center(), vec2(14.0, 14.0)), ico, sw, lerp_c(DIM, TEXT, hk));
        if resp.on_hover_text(tip).clicked() {
            match k {
                "collapse" => app.set_mode(Mode::Collapsed),
                "panel" => app.set_mode(Mode::Panel),
                _ => app.new_chat("New conversation. What should I do?".into()),
            }
        }
    }
    let sel = Rect::from_min_size(pos2(x - 4.0 - 96.0, hdr.center().y - 13.0), vec2(96.0, 26.0));
    let cur = app.settings["chatMode"].as_str().unwrap_or("auto").to_string();
    if let Some(v) = select(app, &root, sel, "chat-mode", &cur, &[("auto", "Auto"), ("acceptEdits", "Accept edits"), ("plan", "Plan only")], 10.5) {
        app.set_setting("chatMode", json!(v));
    }
    x = sel.left() - 4.0;
    let provs = app.prov_list();
    if provs.len() > 1 {
        let labels = [("claude", "Claude"), ("codex", "Codex")];
        let ws: Vec<f32> = labels.iter().map(|(_, l)| 20.0 + text_w(&p, l, font(10.5), 0.0)).collect();
        let total = ws.iter().sum::<f32>() + 4.0;
        let pr = Rect::from_min_size(pos2(x - 6.0 - total, hdr.center().y - 13.0), vec2(total, 26.0));
        boxed(&p, pr, 9.0, rgba(255, 255, 255, 0.04), LINE);
        let mut bx = pr.left() + 2.0;
        for ((k, l), w) in labels.into_iter().zip(ws) {
            let b = Rect::from_min_size(pos2(bx, pr.top() + 2.0), vec2(w, 22.0));
            bx += w;
            let resp = hit(app, &root, b, ("prov", k), Sense::click());
            let onp = app.provider == Some(k);
            let hk = app.a.v(("prov_on", k), if onp { 1.0 } else { 0.0 }, 0.3, EASE);
            let (bg, fg) = if k == "codex" { (rgba(68, 255, 168, 0.14), GREEN) } else { (rgba(255, 140, 90, 0.18), rgb(0xff, 0xb3, 0x8a)) };
            fill(&p, b, 7.0, fade(bg, hk));
            let base = if resp.hovered() { TEXT } else { DIM };
            text(&p, b.center(), Align2::CENTER_CENTER, l, font(10.5), lerp_c(base, fg, hk), 0.0);
            if resp.clicked() && !onp {
                if app.ai_busy {
                    app.toast("Wait for the current reply to finish");
                } else {
                    app.set_setting("chatProvider", json!(k));
                    app.new_chat(format!("Now chatting with {}. What should I do?", if k == "codex" { "Codex" } else { "Claude Code" }));
                }
            }
        }
    }

    // composer (bottom): margin 6 14 14, padding 6 6 6 12
    let te_id = root.scope_id().with("chat-in");
    let line_h = 12.5 * 1.45;
    let wrap_w = 740.0 - 28.0 - 12.0 - 6.0 - 8.0 - 32.0;
    let lines = {
        let mut j = job(if app.chat.input.is_empty() { " " } else { &app.chat.input }, font(12.5), TEXT, 0.0);
        j.wrap.max_width = wrap_w;
        p.layout_job(j).rows.len().max(1)
    };
    let te_h = (lines as f32 * line_h).min(110.0 - 12.0) + 12.0;
    let comp_h = te_h + 12.0;
    let comp = Rect::from_min_max(pos2(area.left() + 14.0, area.bottom() - 14.0 - comp_h), pos2(area.right() - 14.0, area.bottom() - 14.0));
    let focused = root.memory(|m| m.has_focus(te_id));
    let fk = app.a.v("comp_f", if focused { 1.0 } else { 0.0 }, 0.25, EASE);
    if fk > 0.01 {
        glow(&p, comp, 14.0, 20.0, fade(if codex { rgba(68, 255, 168, 0.1) } else { rgba(255, 140, 90, 0.1) }, fk));
    }
    fill(&p, comp, 14.0, rgb(6, 10, 20));
    boxed(&p, comp, 14.0, rgba(255, 255, 255, 0.03), lerp_c(LINE2, if codex { rgba(68, 255, 168, 0.5) } else { rgba(255, 150, 100, 0.5) }, fk));
    let send = Rect::from_min_size(pos2(comp.right() - 6.0 - 32.0, comp.bottom() - 6.0 - 32.0), vec2(32.0, 32.0));
    let te_r = Rect::from_min_max(pos2(comp.left() + 12.0, comp.top() + 6.0), pos2(send.left() - 8.0, comp.bottom() - 6.0));
    let mut send_now = false;
    if on && focused && root.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
        send_now = true;
    }
    let hint = format!("Ask {} to do something...", app.ai_name());
    if on {
        let te = egui::TextEdit::multiline(&mut app.chat.input)
            .id(te_id)
            .frame(egui::Frame::NONE)
            .font(font(12.5))
            .text_color(TEXT)
            .hint_text(egui::RichText::new(hint).color(DIM).font(font(12.5)))
            .desired_width(te_r.width())
            .desired_rows(1)
            .margin(egui::Margin::symmetric(0, 6));
        let mut cu = root.new_child(UiBuilder::new().max_rect(te_r));
        egui::ScrollArea::vertical().id_salt("chat-in-scroll").max_height(te_r.height()).auto_shrink([false, true]).show(&mut cu, |ui| {
            ui.add(te);
        });
        if let Some(t) = app.chat.focus_at {
            if app.now >= t {
                app.chat.focus_at = None;
                root.memory_mut(|m| m.request_focus(te_id));
            }
        }
    }
    let sresp = hit(app, &root, send, "send", Sense::click());
    let sh = app.a.v("send_h", if sresp.hovered() { 1.0 } else { 0.0 }, 0.25, SPRING);
    let sr = Rect::from_center_size(send.center(), send.size() * (1.0 + 0.06 * sh));
    let busy = app.ai_busy;
    let br = |c: Color32| lerp_c(c, Color32::WHITE, 0.1 * sh);
    if busy {
        diag(&p, sr, 10.0, br(rgb(0xff, 0x8a, 0x9c)), br(RED));
        fill(&p, Rect::from_center_size(sr.center(), vec2(10.0, 10.0)), 2.0, rgb(0x2a, 0x05, 0x10));
    } else {
        if codex {
            diag(&p, sr, 10.0, br(rgb(0x8d, 0xff, 0xc9)), br(rgb(0x2f, 0xd8, 0x8f)));
        } else {
            diag(&p, sr, 10.0, br(rgb(0xff, 0xb3, 0x8a)), br(rgb(0xff, 0x7a, 0x59)));
        }
        icon(&p, Rect::from_center_size(sr.center(), vec2(14.0, 14.0)), icons::SEND, 0.0, rgb(0x1a, 0x0a, 0x04));
    }
    if sresp.on_hover_text(if busy { "Stop" } else { "Send (Enter)" }).clicked() {
        if busy {
            app.be.claude.stop();
            app.be.codex.stop();
        } else {
            send_now = true;
        }
    }
    if send_now {
        let t = app.chat.input.clone();
        app.send_chat(&t);
    }

    // messages
    let msgs_r = Rect::from_min_max(pos2(area.left(), hdr.bottom()), pos2(area.right(), comp.top() - 6.0));
    let mut mu = root.new_child(UiBuilder::new().max_rect(msgs_r).id_salt("msgs"));
    mu.set_clip_rect(msgs_r.intersect(clip));
    let mut sa = egui::ScrollArea::vertical().id_salt("msgs-scroll").auto_shrink([false, false]).stick_to_bottom(true);
    if std::mem::take(&mut app.chat.scroll_down) {
        sa = sa.vertical_scroll_offset(f32::MAX);
    }
    sa.show(&mut mu, |ui| {
        ui.add_space(14.0);
        let w = 740.0 - 32.0;
        ui.horizontal(|ui| {
            ui.add_space(16.0);
            ui.vertical(|ui| {
                ui.set_width(w);
                ui.spacing_mut().item_spacing = vec2(0.0, 10.0);
                if app.chat.msgs.is_empty() {
                    empty(app, ui, w, msgs_r.height() - 20.0);
                } else {
                    for i in 0..app.chat.msgs.len() {
                        message(app, ui, i, w);
                    }
                }
            });
        });
        ui.add_space(6.0);
    });
    app.live = was;
}

fn empty(app: &mut App, ui: &mut Ui, w: f32, h: f32) {
    let p = ui.painter().clone();
    let custom = app.chat.empty_text.clone();
    let show_sugs = custom.is_none();
    // vertically centred (margin: auto 0)
    let sug_h = if show_sugs { 14.0 + 60.0 } else { 0.0 };
    let block = 16.0 + sug_h;
    let (r, _) = ui.allocate_exact_size(vec2(w, h.max(block)), Sense::hover());
    let top = r.center().y - block / 2.0;
    let msg = custom.unwrap_or_else(|| format!("Tell {} what to do on this {}.", app.ai_name(), if app.is_mac() { "Mac" } else { "PC" }));
    text(&p, pos2(r.center().x, top), Align2::CENTER_TOP, &msg, font(13.0), DIM, 0.0);
    if !show_sugs {
        return;
    }
    // suggestion chips, wrapped and centred
    let sugs: Vec<String> = SUGS.iter().map(|s| app.os_text(s)).collect();
    let ws: Vec<f32> = sugs.iter().map(|s| 24.0 + text_w(&p, s, font(11.0), 0.0)).collect();
    let mut rows: Vec<Vec<usize>> = vec![vec![]];
    let mut rw = 0.0;
    for (i, sw) in ws.iter().enumerate() {
        if rw + sw > w - 40.0 && !rows.last().unwrap().is_empty() {
            rows.push(vec![]);
            rw = 0.0;
        }
        rows.last_mut().unwrap().push(i);
        rw += sw + 6.0;
    }
    let mut y = top + 16.0 + 14.0;
    for row in rows {
        let total: f32 = row.iter().map(|i| ws[*i]).sum::<f32>() + 6.0 * (row.len() as f32 - 1.0);
        let mut x = r.center().x - total / 2.0;
        for i in row {
            let b = Rect::from_min_size(pos2(x, y), vec2(ws[i], 27.0));
            x += ws[i] + 6.0;
            let resp = hit(app, ui, b, ("sug", i), Sense::click());
            let hk = app.a.v(("sug_h", i), if resp.hovered() { 1.0 } else { 0.0 }, 0.25, SPRING);
            let b = b.translate(vec2(0.0, -hk));
            boxed(&p, b, 14.0, lerp_c(rgba(255, 255, 255, 0.02), rgba(255, 140, 90, 0.08), hk), lerp_c(LINE2, rgba(255, 150, 100, 0.45), hk));
            text(&p, b.center(), Align2::CENTER_CENTER, &sugs[i], font(11.0), TEXT, 0.0);
            if resp.clicked() {
                let s = sugs[i].clone();
                app.send_chat(&s);
            }
        }
        y += 27.0 + 6.0;
    }
}

fn message(app: &mut App, ui: &mut Ui, i: usize, w: f32) {
    let id = app.chat.msgs[i].id;
    // msgIn: fade up from 8 px, .45s spring
    let k = app.a.v_in(("msg", id), 0.0, 1.0, 0.45, 0.0, SPRING);
    let off = 8.0 * (1.0 - k);
    ui.add_space(0.0);
    let kind = &app.chat.msgs[i].kind;
    match kind {
        Kind::User => {
            let t = match &app.chat.msgs[i].parts[0] {
                Part::Text(t) => t.clone(),
                _ => String::new(),
            };
            let maxw = w * 0.88 - 24.0;
            let mut j = job(&t, font(12.5), TEXT, 0.0);
            j.wrap.max_width = maxw;
            for s in j.sections.iter_mut() {
                s.format.line_height = Some(12.5 * 1.5);
            }
            let g = ui.painter().layout_job(j);
            let sz = g.size() + vec2(24.0, 16.0);
            let (row, _) = ui.allocate_exact_size(vec2(w, sz.y), Sense::hover());
            let r = Rect::from_min_size(pos2(row.right() - sz.x, row.top() + off), sz);
            let mut p = ui.painter().clone();
            p.set_opacity(k.clamp(0.0, 1.0) * p.opacity());
            let rad = [14.0, 14.0, 4.0, 14.0];
            let (o, s) = (r.min, r.size());
            fill_with(&p, r, rad, 3, |q| lerp_c(rgba(62, 243, 255, 0.2), rgba(155, 123, 255, 0.2), ((q.x - o.x) + (q.y - o.y)) / (s.x + s.y)));
            p.rect_stroke(r, cr4(14.0, 14.0, 4.0, 14.0), Stroke::new(1.0, rgba(62, 243, 255, 0.25)), egui::StrokeKind::Inside);
            let lr = Rect::from_min_size(r.min + vec2(12.0, 8.0), g.size());
            ui.put(lr, egui::Label::new(g_to_job(&t, maxw, 12.5)).selectable(true));
        }
        Kind::Err => {
            let t = match &app.chat.msgs[i].parts[0] {
                Part::Text(t) => t.clone(),
                _ => String::new(),
            };
            let mut j = job(&t, font(11.5), rgb(0xff, 0xc2, 0xcc), 0.0);
            j.wrap.max_width = w - 24.0;
            let g = ui.painter().layout_job(j);
            let (r, _) = ui.allocate_exact_size(vec2(w, g.size().y + 16.0), Sense::hover());
            let p = ui.painter().clone();
            boxed(&p, r, 10.0, rgba(255, 77, 109, 0.1), rgba(255, 77, 109, 0.3));
            p.galley(r.min + vec2(12.0, 8.0), g, TEXT);
        }
        Kind::Bot => {
            let codex = app.provider == Some("codex");
            let accent = if codex { GREEN } else { ORANGE };
            let maxw = w * 0.88;
            let now = app.now;
            ui.vertical(|ui| {
                ui.set_max_width(maxw);
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                if off > 0.05 {
                    ui.add_space(off);
                }
                let typing = app.chat.msgs[i].typing;
                let n = app.chat.msgs[i].parts.len();
                for pi in 0..n {
                    match &app.chat.msgs[i].parts[pi] {
                        Part::Text(t) => markdown(ui, t, maxw - 4.0),
                        Part::Tool { name, detail, state, .. } => {
                            let (name, detail, state) = (name.clone(), detail.clone(), *state);
                            ui.add_space(5.0);
                            let (r, resp) = ui.allocate_exact_size(vec2(maxw.min(ui.available_width()), 24.0), Sense::hover());
                            let p = ui.painter().clone();
                            boxed(&p, r, 8.0, rgba(255, 255, 255, 0.03), LINE);
                            let dot = pos2(r.left() + 10.0 + 5.0, r.center().y);
                            match state {
                                0 => {
                                    p.circle_stroke(dot, 4.0, Stroke::new(2.0, fade(accent, 0.3)));
                                    let a0 = (now % 0.8 / 0.8) as f32 * std::f32::consts::TAU;
                                    p.add(egui::Shape::line(arc_pts(dot, 4.0, a0, a0 + 1.5), Stroke::new(2.0, accent)));
                                }
                                1 => {
                                    radial(&p, dot, 6.0, rgba(68, 255, 168, 0.6));
                                    p.circle_filled(dot, 3.5, GREEN);
                                }
                                _ => {
                                    p.circle_filled(dot, 3.5, RED);
                                }
                            }
                            let nr = text(&p, pos2(dot.x + 5.0 + 8.0, r.center().y), Align2::LEFT_CENTER, &name, mono(10.5), accent, 0.0);
                            text_fit(&p, pos2(nr.right() + 8.0, r.center().y), Align2::LEFT_CENTER, &detail, mono(10.5), DIM, 0.0, r.right() - 10.0 - nr.right() - 8.0);
                            resp.on_hover_text(detail);
                            ui.add_space(5.0);
                        }
                        Part::Denied(t) => {
                            let t = t.clone();
                            ui.add_space(6.0);
                            let mut j = job(&t, font(11.0), AMBER, 0.0);
                            j.wrap.max_width = maxw - 20.0;
                            let g = ui.painter().layout_job(j);
                            let (r, _) = ui.allocate_exact_size(vec2(maxw, g.size().y + 12.0), Sense::hover());
                            boxed(ui.painter(), r, 8.0, rgba(255, 181, 71, 0.08), rgba(255, 181, 71, 0.25));
                            ui.painter().galley(r.min + vec2(10.0, 6.0), g, AMBER);
                        }
                        Part::Meta(t) => {
                            if !t.is_empty() {
                                ui.add_space(4.0);
                                let t = t.clone();
                                let (r, _) = ui.allocate_exact_size(vec2(maxw, 12.0), Sense::hover());
                                text(ui.painter(), r.left_top(), Align2::LEFT_TOP, &t, font(9.5), DIM2, 0.57);
                            }
                        }
                    }
                }
                if typing {
                    let (r, _) = ui.allocate_exact_size(vec2(40.0, 18.0), Sense::hover());
                    let p = ui.painter().clone();
                    for d in 0..3 {
                        let t = ((now - d as f64 * 0.15) / 1.2).rem_euclid(1.0) as f32;
                        let k = if t < 0.4 { EASE_IN_OUT.at(t / 0.4) } else if t < 0.8 { 1.0 - EASE_IN_OUT.at((t - 0.4) / 0.4) } else { 0.0 };
                        let s = 0.5 + 0.5 * k;
                        let o = 0.4 + 0.6 * k;
                        p.circle_filled(pos2(r.left() + 2.0 + 3.0 + d as f32 * 10.0, r.center().y), 3.0 * s, fade(accent, o));
                    }
                }
            });
        }
    }
}

fn g_to_job(t: &str, maxw: f32, size: f32) -> LayoutJob {
    let mut j = job(t, font(size), TEXT, 0.0);
    j.wrap.max_width = maxw;
    for s in j.sections.iter_mut() {
        s.format.line_height = Some(size * 1.5);
    }
    j
}

// ---------------- markdown (md() in app.js) ----------------
fn markdown(ui: &mut Ui, text: &str, w: f32) {
    let fence = regex::Regex::new(r"```[\w+-]*\n?").unwrap();
    let parts: Vec<&str> = fence.split(text).collect();
    let list_re = regex::Regex::new(r"^\s*([-*]|\d+\.)\s+").unwrap();
    for (i, part) in parts.iter().enumerate() {
        if i % 2 == 1 {
            let code = part.trim_end_matches('\n');
            ui.add_space(6.0);
            let mut j = LayoutJob::default();
            j.append(code, 0.0, TextFormat { font_id: mono(11.0), color: TEXT, ..Default::default() });
            let g = ui.painter().layout_job(j.clone());
            let (r, _) = ui.allocate_exact_size(vec2(w, g.size().y + 16.0), Sense::hover());
            boxed(ui.painter(), r, 8.0, rgba(0, 0, 0, 0.35), LINE);
            let mut cu = ui.new_child(UiBuilder::new().max_rect(r.shrink2(vec2(10.0, 8.0))));
            egui::ScrollArea::horizontal().id_salt(("code", i, code.len())).auto_shrink([false, true]).show(&mut cu, |ui| {
                ui.add(egui::Label::new(j).selectable(true).extend());
            });
            ui.add_space(6.0);
            continue;
        }
        let blocks: Vec<&str> = regex::Regex::new(r"\n{2,}").unwrap().split(part).collect();
        for block in blocks {
            if block.trim().is_empty() {
                continue;
            }
            let lines: Vec<&str> = block.split('\n').collect();
            let is_list = lines.iter().all(|l| list_re.is_match(l) || l.trim().is_empty());
            if is_list {
                ui.add_space(2.0);
                for l in lines.iter().filter(|l| !l.trim().is_empty()) {
                    let item = list_re.replace(l, "").to_string();
                    let job = inline(&item, w - 18.0);
                    let g = ui.painter().layout_job(job.clone());
                    let (r, _) = ui.allocate_exact_size(vec2(w, g.size().y), Sense::hover());
                    ui.painter().circle_filled(pos2(r.left() + 8.0, r.top() + 9.5), 2.0, TEXT);
                    ui.put(Rect::from_min_size(r.min + vec2(18.0, 0.0), g.size()), egui::Label::new(job).selectable(true));
                }
                ui.add_space(6.0);
            } else {
                let job = inline(&lines.join("\n"), w);
                ui.add(egui::Label::new(job).selectable(true));
                ui.add_space(6.0);
            }
        }
    }
}

/// `code` and **bold** inside a paragraph.
fn inline(s: &str, w: f32) -> LayoutJob {
    let mut j = LayoutJob::default();
    j.wrap.max_width = w;
    let lh = Some(12.5 * 1.5);
    let normal = TextFormat { font_id: font(12.5), color: TEXT, line_height: lh, ..Default::default() };
    let bold = TextFormat { font_id: font7(12.5), color: TEXT, line_height: lh, ..Default::default() };
    let code = TextFormat { font_id: mono(11.0), color: TEXT, background: rgba(255, 255, 255, 0.07), line_height: lh, ..Default::default() };
    let re = regex::Regex::new(r"`([^`\n]+)`|\*\*([^*\n]+)\*\*").unwrap();
    let mut last = 0;
    for c in re.captures_iter(s) {
        let m = c.get(0).unwrap();
        j.append(&s[last..m.start()], 0.0, normal.clone());
        if let Some(code_t) = c.get(1) {
            j.append(code_t.as_str(), 0.0, code.clone());
        } else if let Some(b) = c.get(2) {
            j.append(b.as_str(), 0.0, bold.clone());
        }
        last = m.end();
    }
    j.append(&s[last..], 0.0, normal);
    j
}
