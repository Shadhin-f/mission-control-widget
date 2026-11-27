// The pointer thread. While the window lets clicks through it gets no mouse events, so (like Electron's
// forwarded mouse moves) this polls the cursor: fast while it is near the widget, slowly otherwise. It
// wakes the UI only when the pointer crosses into or out of the widget, or when the pet's eyes should
// move by a visible amount. It also lights up the Extreme mode glow line over the taskbar.
use crate::app::Pointer;
use eframe::egui::{self, pos2, vec2};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub fn spawn(ptr: Arc<Mutex<Pointer>>, ctx: egui::Context) {
    std::thread::Builder::new()
        .name("mc-pointer".into())
        .spawn(move || loop {
            let near = tick(&ptr, &ctx);
            std::thread::sleep(Duration::from_millis(if near { 16 } else { 50 }));
        })
        .ok();
}

fn tick(ptr: &Arc<Mutex<Pointer>>, ctx: &egui::Context) -> bool {
    let Some((cx, cy)) = crate::platform::cursor() else { return false };
    let mut p = ptr.lock().unwrap();
    if !p.ready {
        return false;
    }
    let ppp = p.ppp.max(0.1) as f64;
    let local = pos2(((cx - p.origin.0) / ppp) as f32, ((cy - p.origin.1) / ppp) as f32);
    let near = local.x > -40.0 && local.x < crate::app::WIN_W + 40.0 && local.y > -40.0 && local.y < crate::app::WIN_H + 40.0;
    let hit = p.hits.iter().any(|r| r.contains(local));
    let mut wake = hit != p.hit;
    p.hit = hit;
    let moved = p.pos.map(|q| q.distance(local) > 0.5).unwrap_or(true);
    p.pos = Some(local);
    // Eyes follow the pointer anywhere on screen (app.js pet.look)
    let d = local - p.pet_center;
    let len = d.length().max(1.0);
    let k = (len / 260.0).min(1.0);
    let mut t = vec2(d.x / len * 3.6 * k, d.y / len * 2.2 * k);
    if p.think {
        t = vec2(2.5, -1.8);
    }
    // Ignore sub-pixel changes so distant mouse movement does not keep the window repainting
    if (t.x - p.eye_target.x).abs() >= 0.4 || (t.y - p.eye_target.y).abs() >= 0.4 {
        p.eye_target = t;
        wake = true;
    }
    if hit && moved {
        wake = true; // hover effects inside the widget
    }
    // Extreme mode: the taskbar glow line lights up under the pointer
    let hot = p.taskbar.is_some_and(|(x, y, w, h)| cx >= x && cx <= x + w && cy >= y && cy <= y + h);
    if hot != p.edge_hot {
        p.edge_hot = hot;
        drop(p);
        crate::backend::emit("edge-hot", hot);
        wake = true;
    } else {
        drop(p);
    }
    if wake {
        ctx.request_repaint();
    }
    near
}
