// Dev only (MC_SHOTS=<folder>): walks through the collapsed notch, hover, every tab and the chat, and
// saves a PNG of each, for comparing against the other editions. MC_SHOTS_ONLY limits it to a list
// such as "overview,chat".
use crate::app::{App, Mode, TABS};
use eframe::egui::{self, ViewportCommand};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub struct Shots {
    dir: PathBuf,
    steps: Vec<String>,
    i: usize,
    at: f64,
    pending: Arc<AtomicBool>,
}

impl Shots {
    pub fn from_env() -> Option<Self> {
        let dir = PathBuf::from(std::env::var_os("MC_SHOTS")?);
        let _ = std::fs::create_dir_all(&dir);
        let mut steps: Vec<String> = vec!["collapsed".into(), "hover".into()];
        steps.extend(TABS.iter().map(|t| t.label().to_lowercase()));
        steps.push("chat".into());
        if let Ok(only) = std::env::var("MC_SHOTS_ONLY") {
            let keep: Vec<&str> = only.split(',').collect();
            steps.retain(|s| keep.contains(&s.as_str()));
        }
        Some(Self { dir, steps, i: 0, at: 6.0, pending: Arc::new(AtomicBool::new(false)) })
    }

    pub fn step(&mut self, ctx: &egui::Context, app: &mut App) {
        ctx.request_repaint();
        app.pinned = true; // keep the panel open while the test window loses focus
        if self.pending.load(Ordering::SeqCst) {
            return;
        }
        if self.i >= self.steps.len() {
            if self.i == self.steps.len() {
                self.i += 1;
                std::fs::write(self.dir.join("done.txt"), "ok").ok();
            }
            return;
        }
        let name = self.steps[self.i].clone();
        match name.as_str() {
            "collapsed" => app.set_mode(Mode::Collapsed),
            "hover" => {
                app.shell_hover = true;
                app.hover_raw = true;
            }
            "chat" => app.set_mode(Mode::Chat),
            t => {
                if let Some(tab) = TABS.iter().find(|x| x.label().to_lowercase() == t) {
                    app.shell_hover = false;
                    if app.mode != Mode::Panel {
                        app.set_mode(Mode::Panel);
                    }
                    if app.tab != *tab {
                        app.set_tab(*tab);
                        self.at = self.at.max(app.now + 2.5);
                    }
                }
            }
        }
        if app.now >= self.at {
            self.pending.store(true, Ordering::SeqCst);
            let (pending, file) = (self.pending.clone(), self.dir.join(format!("{name}.png")));
            self.i += 1;
            self.at = app.now + 2.5;
            ctx.send_viewport_cmd(ViewportCommand::Screenshot(egui::ScreenshotCallback::new(move |img: Arc<egui::ColorImage>| {
                let [w, h] = img.size;
                let mut buf = image::RgbaImage::new(w as u32, h as u32);
                for (i, px) in img.pixels.iter().enumerate() {
                    let [r, g, b, a] = px.to_srgba_unmultiplied();
                    buf.put_pixel((i % w) as u32, (i / w) as u32, image::Rgba([r, g, b, a]));
                }
                let _ = buf.save(file);
                pending.store(false, Ordering::SeqCst);
            })));
        }
    }
}
