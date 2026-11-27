// Credits tab: the project's GitHub contributors (public API, cached for a day in the settings folder,
// avatars cached next to it), plus what the widget is built with.
use crate::app::App;
use crate::backend::{call, call_ctx};
use crate::draw::*;
use crate::icons;
use crate::pages::{card, card_h, card_h_right, hit, PAGE_W};
use crate::theme::*;
use eframe::egui::{self, pos2, vec2, Align2, Color32, Mesh, Rect, Sense, Shape, Stroke, TextureHandle, Ui};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;

pub const REPO: &str = "Shadhin-f/mission-control-widget";
const DAY_MS: f64 = 24.0 * 3600.0 * 1000.0;

#[derive(Clone)]
struct Person {
    login: String,
    url: String,
    avatar: String,
    commits: u64,
}

pub struct Credits {
    people: Vec<Person>,
    fetched_at: f64,
    requested: bool,
    loading: bool,
    error: Option<String>,
    avatars: HashMap<String, TextureHandle>,
    avatar_req: HashMap<String, bool>,
}

fn dir() -> PathBuf {
    std::env::var("MC_SETTINGS_DIR").map(PathBuf::from).unwrap_or_else(|_| dirs::config_dir().unwrap_or_else(crate::util::home).join("Mission Control"))
}
fn cache_file() -> PathBuf {
    dir().join("contributors.json")
}
fn avatar_file(login: &str) -> PathBuf {
    let safe: String = login.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
    dir().join("avatars").join(format!("{safe}.img"))
}

fn parse(list: &Value) -> Vec<Person> {
    list.as_array()
        .into_iter()
        .flatten()
        .filter(|c| c["type"] != "Bot" && !c["login"].as_str().unwrap_or("").ends_with("[bot]"))
        .map(|c| Person {
            login: c["login"].as_str().unwrap_or("").to_string(),
            url: c["html_url"].as_str().unwrap_or("").to_string(),
            avatar: c["avatar_url"].as_str().unwrap_or("").to_string(),
            commits: c["contributions"].as_u64().unwrap_or(0),
        })
        .filter(|p| !p.login.is_empty())
        .collect()
}

/// Shown until the first successful fetch (offline first run)
fn fallback() -> Vec<Person> {
    ["Shadhin-f", "Awsaf2745"]
        .into_iter()
        .map(|l| Person { login: l.into(), url: format!("https://github.com/{l}"), avatar: String::new(), commits: 0 })
        .collect()
}

impl Credits {
    pub fn load() -> Self {
        let mut c = Self { people: vec![], fetched_at: 0.0, requested: false, loading: false, error: None, avatars: HashMap::new(), avatar_req: HashMap::new() };
        if let Ok(t) = std::fs::read_to_string(cache_file()) {
            if let Ok(v) = serde_json::from_str::<Value>(&t) {
                c.people = parse(&v["list"]);
                c.fetched_at = v["at"].as_f64().unwrap_or(0.0);
            }
        }
        c
    }

    pub fn loading(&self) -> bool {
        self.loading
    }

    /// Called when the tab opens: refresh from GitHub at most once a day.
    pub fn ensure(&mut self) {
        if self.requested || crate::util::now_ms() - self.fetched_at < DAY_MS && !self.people.is_empty() {
            return;
        }
        self.requested = true;
        self.loading = true;
        call("r:credits", async {
            let url = format!("https://api.github.com/repos/{REPO}/contributors?per_page=100");
            let r = http().get(url).header("Accept", "application/vnd.github+json").send().await.map_err(|e| e.to_string())?;
            if !r.status().is_success() {
                return Err(format!("GitHub answered {}", r.status()));
            }
            r.json::<Value>().await.map_err(|e| e.to_string())
        });
    }

    pub fn on_result(&mut self, v: Value, _ctx: &egui::Context) {
        self.loading = false;
        if v["ok"] == true {
            let list = v["v"].clone();
            self.people = parse(&list);
            self.fetched_at = crate::util::now_ms();
            self.error = None;
            let _ = std::fs::create_dir_all(dir());
            let _ = std::fs::write(cache_file(), json!({ "at": self.fetched_at, "list": list }).to_string());
            // avatars may have changed: fetch them again
            self.avatar_req.clear();
            for p in &self.people {
                let _ = std::fs::remove_file(avatar_file(&p.login));
            }
        } else {
            self.error = Some(v["err"].as_str().unwrap_or("Offline").to_string());
        }
    }

    pub fn on_avatar(&mut self, v: Value, ctx: &egui::Context) {
        let login = v["ctx"].as_str().unwrap_or("").to_string();
        if v["ok"] == true {
            self.load_avatar(&login, ctx);
        }
    }

    fn load_avatar(&mut self, login: &str, ctx: &egui::Context) -> bool {
        let Ok(bytes) = std::fs::read(avatar_file(login)) else { return false };
        let Ok(img) = image::load_from_memory(&bytes) else { return false };
        let img = img.resize_exact(96, 96, image::imageops::FilterType::Triangle).to_rgba8();
        let ci = egui::ColorImage::from_rgba_unmultiplied([96, 96], img.as_raw());
        let tex = ctx.load_texture(format!("avatar-{login}"), ci, egui::TextureOptions::LINEAR);
        self.avatars.insert(login.to_string(), tex);
        true
    }

    fn avatar(&mut self, p: &Person, ctx: &egui::Context) -> Option<egui::TextureId> {
        if let Some(t) = self.avatars.get(&p.login) {
            return Some(t.id());
        }
        if self.avatar_req.contains_key(&p.login) {
            return None;
        }
        self.avatar_req.insert(p.login.clone(), true);
        if self.load_avatar(&p.login, ctx) {
            return self.avatars.get(&p.login).map(|t| t.id());
        }
        if p.avatar.is_empty() {
            return None;
        }
        let url = format!("{}{}s=96", p.avatar, if p.avatar.contains('?') { "&" } else { "?" });
        let file = avatar_file(&p.login);
        call_ctx("r:avatar", json!(p.login), async move {
            let b = http().get(url).send().await.map_err(|e| e.to_string())?.bytes().await.map_err(|e| e.to_string())?;
            if let Some(d) = file.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            std::fs::write(&file, &b).map_err(|e| e.to_string())?;
            Ok(Value::Null)
        });
        None
    }
}

fn http() -> reqwest::Client {
    reqwest::Client::builder().user_agent("MissionControl/1.0").timeout(std::time::Duration::from_secs(10)).build().unwrap_or_default()
}

fn circle_image(p: &egui::Painter, tex: egui::TextureId, c: egui::Pos2, r: f32) {
    let segs = 40u32;
    let mut mesh = Mesh::with_texture(tex);
    mesh.vertices.push(egui::epaint::Vertex { pos: c, uv: pos2(0.5, 0.5), color: Color32::WHITE });
    for i in 0..segs {
        let a = std::f32::consts::TAU * i as f32 / segs as f32;
        let (x, y) = (a.cos(), a.sin());
        mesh.vertices.push(egui::epaint::Vertex { pos: c + vec2(x, y) * r, uv: pos2(0.5 + x * 0.5, 0.5 + y * 0.5), color: Color32::WHITE });
    }
    for i in 0..segs {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % segs);
    }
    p.add(Shape::mesh(mesh));
}

pub fn page(app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    // hero
    let (hero, _) = ui.allocate_exact_size(vec2(PAGE_W, 96.0), Sense::hover());
    let p = ui.painter().clone();
    card(&p, hero);
    let (o, s) = (hero.min, hero.size());
    fill_with(&p, hero, [14.0; 4], 4, |q| {
        let dx = (q.x - o.x) / (0.7 * s.x);
        let dy = (q.y - o.y) / (1.6 * s.y);
        fade(rgba(62, 243, 255, 0.10), (1.0 - (dx * dx + dy * dy).sqrt()).max(0.0))
    });
    let tr = text(&p, hero.min + vec2(18.0, 18.0), Align2::LEFT_TOP, "Mission Control", font6(20.0), TEXT, 0.2);
    let sub = "Open source and built by the people below. Thank you for every commit, idea and bug report.";
    text_fit(&p, pos2(hero.left() + 18.0, tr.bottom() + 6.0), Align2::LEFT_TOP, sub, font(11.5), DIM, 0.0, PAGE_W - 220.0);
    let chips = ["MIT licence", "Pure Rust edition", "egui + OpenGL"];
    let mut x = hero.left() + 18.0;
    let cy = hero.bottom() - 20.0;
    for c in chips {
        let g = galley(&p, &c.to_uppercase(), font(9.0), CYAN, 1.26);
        let r = Rect::from_min_size(pos2(x, cy - g.size().y / 2.0 - 3.0), g.size() + vec2(16.0, 6.0));
        fill(&p, r, 9.0, rgba(62, 243, 255, 0.08));
        p.rect_stroke(r, cr(9.0), Stroke::new(1.0, rgba(62, 243, 255, 0.25)), egui::StrokeKind::Inside);
        p.galley(r.min + vec2(8.0, 3.0), g, CYAN);
        x = r.right() + 6.0;
    }
    let btn = Rect::from_min_size(pos2(hero.right() - 18.0 - 170.0, hero.center().y - 17.0), vec2(170.0, 34.0));
    if crate::pages::cta(app, ui, &p, btn, "repo", "View on GitHub ↗", true, false) {
        crate::platform::open_url(&format!("https://github.com/{REPO}"));
    }

    // contributors
    let people = if app.credits.people.is_empty() { fallback() } else { app.credits.people.clone() };
    let cols = 3usize;
    let rows = people.len().div_ceil(cols).max(1);
    let card_h_ = 64.0;
    let h = 12.0 + 12.0 + 10.0 + rows as f32 * card_h_ + (rows as f32 - 1.0) * 10.0 + 14.0;
    let (cr_, _) = ui.allocate_exact_size(vec2(PAGE_W, h), Sense::hover());
    card(&p, cr_);
    let hh = card_h(&p, cr_, "Contributors");
    let status = if app.credits.loading {
        "Loading from GitHub...".to_string()
    } else if let Some(e) = &app.credits.error {
        if app.credits.people.is_empty() { format!("Offline ({e})") } else { "Offline · saved list".into() }
    } else if app.credits.people.is_empty() {
        "--".into()
    } else {
        format!("{} people · from GitHub", app.credits.people.len())
    };
    card_h_right(&p, hh, &status, DIM);
    let gw = (PAGE_W - 28.0 - 20.0) / 3.0;
    let max_commits = people.iter().map(|p| p.commits).max().unwrap_or(0);
    for (i, person) in people.iter().enumerate() {
        let r = Rect::from_min_size(pos2(cr_.left() + 14.0 + (i % cols) as f32 * (gw + 10.0), hh.bottom() + 10.0 + (i / cols) as f32 * (card_h_ + 10.0)), vec2(gw, card_h_));
        let k = app.a.v_in(("cred_in", &person.login), 0.0, 1.0, 0.45, i as f32 * 0.05, SPRING);
        let resp = hit(app, ui, r, ("person", &person.login), Sense::click());
        let hk = app.a.v(("cred_h", &person.login), if resp.hovered() { 1.0 } else { 0.0 }, 0.3, SPRING);
        let r = r.translate(vec2(14.0 * (1.0 - k), -2.0 * hk));
        let mut q = p.clone();
        q.set_opacity(k.clamp(0.0, 1.0) * p.opacity());
        boxed(&q, r, 12.0, lerp_c(rgba(255, 255, 255, 0.025), rgba(62, 243, 255, 0.06), hk), lerp_c(LINE, rgba(62, 243, 255, 0.4), hk));
        let ac = pos2(r.left() + 12.0 + 20.0, r.center().y);
        let hue = crate::app::hue_of(&person.login);
        let ring = [CYAN, VIOLET, PINK, GREEN, AMBER][i % 5];
        radial(&q, ac, 30.0, fade(ring, 0.18 + 0.12 * hk));
        match app.credits.avatar(person, &ctx) {
            Some(t) => circle_image(&q, t, ac, 20.0),
            None => {
                q.circle_filled(ac, 20.0, fade(hsl(hue, 80.0, 60.0), 0.18));
                let l = person.login.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
                text(&q, ac, Align2::CENTER_CENTER, &l, font7(15.0), hsl(hue, 90.0, 72.0), 0.0);
            }
        }
        q.circle_stroke(ac, 21.0, Stroke::new(1.5, fade(ring, 0.8)));
        let tx = ac.x + 20.0 + 12.0;
        let tw = r.right() - 10.0 - tx;
        let nr = text_fit(&q, pos2(tx, r.center().y - 1.0), Align2::LEFT_BOTTOM, &person.login, font6(13.0), TEXT, 0.0, tw - 18.0);
        let sub = match person.commits {
            0 => "Contributor".to_string(),
            1 => "1 commit".to_string(),
            n => format!("{n} commits"),
        };
        let sub = if i == 0 && person.commits > 0 && person.commits == max_commits { format!("{sub} · maintainer") } else { sub };
        text_fit(&q, pos2(tx, r.center().y + 2.0), Align2::LEFT_TOP, &sub, font(11.0), DIM, 0.0, tw);
        if i == 0 && max_commits > 0 {
            icon(&q, Rect::from_min_size(pos2(nr.right() + 5.0, nr.center().y - 6.5), vec2(13.0, 13.0)), icons::STAR, 1.8, AMBER);
        }
        if resp.on_hover_text(format!("Open {} on GitHub", person.login)).clicked() && !person.url.is_empty() {
            crate::platform::open_url(&person.url);
        }
    }

    // built with
    let built: [(&str, &str); 10] = [
        ("egui", "interface"),
        ("glow", "OpenGL"),
        ("tokio", "async runtime"),
        ("sysinfo", "CPU / RAM"),
        ("reqwest", "speed test"),
        ("serde", "settings"),
        (if cfg!(windows) { "windows-rs" } else { "objc2" }, "system APIs"),
        ("tray-icon", "tray"),
        ("global-hotkey", "shortcuts"),
        ("image", "avatars"),
    ];
    let mut widths = vec![];
    for (n, d) in built {
        widths.push(20.0 + text_w(&p, n, font6(11.0), 0.0) + 6.0 + text_w(&p, d, font(10.5), 0.0));
    }
    // flow into rows
    let inner_w = PAGE_W - 28.0;
    let mut rows_: Vec<Vec<usize>> = vec![vec![]];
    let mut rw = 0.0;
    for (i, w) in widths.iter().enumerate() {
        if rw + w > inner_w && !rows_.last().unwrap().is_empty() {
            rows_.push(vec![]);
            rw = 0.0;
        }
        rows_.last_mut().unwrap().push(i);
        rw += w + 8.0;
    }
    let bh = 12.0 + 12.0 + 10.0 + rows_.len() as f32 * 28.0 + (rows_.len() as f32 - 1.0) * 8.0 + 14.0;
    let (br, _) = ui.allocate_exact_size(vec2(PAGE_W, bh), Sense::hover());
    card(&p, br);
    let hh = card_h(&p, br, "Built with");
    card_h_right(&p, hh, "and the Rust community", DIM);
    let mut y = hh.bottom() + 10.0;
    for row in rows_ {
        let mut x = br.left() + 14.0;
        for i in row {
            let (n, d) = built[i];
            let r = Rect::from_min_size(pos2(x, y), vec2(widths[i], 28.0));
            boxed(&p, r, 14.0, rgba(255, 255, 255, 0.03), LINE);
            let nr = text(&p, pos2(r.left() + 10.0, r.center().y), Align2::LEFT_CENTER, n, font6(11.0), TEXT, 0.0);
            text(&p, pos2(nr.right() + 6.0, r.center().y), Align2::LEFT_CENTER, d, font(10.5), DIM, 0.0);
            x = r.right() + 8.0;
        }
        y += 36.0;
    }
    let _ = icons::HEART;
}
