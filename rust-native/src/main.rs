// Mission Control, pure Rust edition: the same widget drawn natively on the GPU (egui + OpenGL), no web view.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// ---- Backend: shared, unchanged, with the Tauri edition (see host/src/lib.rs) ----
#[path = "../../rust/src-tauri/src/claude.rs"]
mod claude;
#[path = "../../rust/src-tauri/src/cleaner.rs"]
mod cleaner;
#[path = "../../rust/src-tauri/src/codex.rs"]
mod codex;
#[path = "../../rust/src-tauri/src/login.rs"]
mod login;
#[path = "../../rust/src-tauri/src/network.rs"]
mod network;
#[path = "../../rust/src-tauri/src/processes.rs"]
mod processes;
#[path = "../../rust/src-tauri/src/settings.rs"]
mod settings;
#[path = "../../rust/src-tauri/src/stats.rs"]
mod stats;
#[path = "../../rust/src-tauri/src/util.rs"]
mod util;
#[cfg(windows)]
#[path = "../../rust/src-tauri/src/extreme.rs"]
mod extreme;
#[cfg(target_os = "macos")]
#[path = "../../rust/src-tauri/src/native_mac.rs"]
mod native_mac;
#[cfg(windows)]
#[path = "../../rust/src-tauri/src/native_win.rs"]
#[allow(dead_code)] // window helpers the Tauri edition needs for its web views
mod native_win;
#[cfg(target_os = "macos")]
#[path = "../../rust/src-tauri/src/sys_mac.rs"]
mod sys_mac;
#[cfg(windows)]
#[path = "../../rust/src-tauri/src/sys_win.rs"]
mod sys_win;
#[cfg(target_os = "macos")]
use native_mac as native;
#[cfg(windows)]
use native_win as native;

// ---- The native interface ----
mod anim;
mod app;
mod backend;
mod chat;
mod controls;
mod credits;
mod draw;
#[cfg(windows)]
mod dock;
#[cfg(windows)]
mod edge;
mod health;
mod icons;
mod idle;
mod notch;
mod pages;
mod platform;
mod pointer;
mod power;
mod power_page;
mod shots;
mod theme;
mod tray;

use eframe::egui;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

fn main() -> eframe::Result {
    // A second launch opens the running widget's panel instead
    if !tray::claim_instance() {
        return Ok(());
    }
    // Backend events land in this queue; the UI drains it each frame and is woken on every push
    let q: Arc<Mutex<VecDeque<(String, serde_json::Value)>>> = Arc::new(Mutex::new(VecDeque::new()));
    let ctx_slot: Arc<Mutex<Option<egui::Context>>> = Arc::new(Mutex::new(None));
    {
        let (q, ctx_slot) = (q.clone(), ctx_slot.clone());
        tauri::set_sink(move |ch, v| {
            q.lock().unwrap().push_back((ch.to_string(), v));
            if let Some(c) = ctx_slot.lock().unwrap().as_ref() {
                c.request_repaint();
            }
        });
    }

    let viewport = egui::ViewportBuilder::default()
        .with_title("Mission Control")
        .with_app_id("mission-control")
        .with_inner_size([app::WIN_W, app::SMALL_H])
        .with_position(initial_pos())
        .with_decorations(false)
        .with_transparent(true)
        .with_resizable(false)
        .with_maximize_button(false)
        .with_minimize_button(false)
        .with_taskbar(false)
        .with_active(false)
        .with_has_shadow(false)
        .with_always_on_top()
        .with_mouse_passthrough(cfg!(target_os = "macos"))
        .with_drag_and_drop(false);
    let options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Glow,
        multisampling: 0,
        persist_window: false,
        event_loop_builder: Some(Box::new(|_b| {
            #[cfg(target_os = "macos")]
            {
                use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
                _b.with_activation_policy(ActivationPolicy::Accessory);
            }
        })),
        ..Default::default()
    };
    eframe::run_native(
        "Mission Control",
        options,
        Box::new(move |cc| {
            *ctx_slot.lock().unwrap() = Some(cc.egui_ctx.clone());
            Ok(Box::new(app::App::new(cc, q)))
        }),
    )
}

/// Top-centre of the primary work area, in points (refined on the first frame).
fn initial_pos() -> egui::Pos2 {
    let scale = initial_scale();
    match platform::work_area() {
        Some((x, y, w, _)) => egui::pos2(((x + (w - app::WIN_W as f64 * scale) / 2.0) / scale) as f32, (y / scale) as f32),
        None => egui::pos2(0.0, 0.0),
    }
}

fn initial_scale() -> f64 {
    #[cfg(windows)]
    unsafe {
        let _ = windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let dpi = windows::Win32::UI::HiDpi::GetDpiForSystem();
        if dpi > 0 {
            return dpi as f64 / 96.0;
        }
    }
    #[cfg(target_os = "macos")]
    {
        platform::set_main_thread();
        return 2.0;
    }
    #[allow(unreachable_code)]
    1.0
}
