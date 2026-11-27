// Tray icon + menu, global shortcuts (Ctrl+Alt+M, Ctrl+Alt+Space) and the single-instance handshake.
use crate::backend::{emit, Backend};
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use std::sync::Arc;
use tray_icon::menu::accelerator::{Accelerator, Code as MCode, Modifiers as MMods};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

pub struct Tray {
    icon: TrayIcon,
    _keys: Option<GlobalHotKeyManager>,
}

/// The pet's face, drawn in code like the other editions' tray icon.
fn tray_rgba() -> Vec<u8> {
    let s = 32usize;
    let mut buf = vec![0u8; s * s * 4];
    for y in 0..s {
        for x in 0..s {
            let (dx, dy) = (x as f64 - 15.5, y as f64 - 15.5);
            let d = (dx * dx + dy * dy).sqrt();
            let a = (14.0 - d).clamp(0.0, 1.0);
            let eye = ((dx + 5.0).abs() < 2.0 || (dx - 5.0).abs() < 2.0) && (dy + 1.0).abs() < 3.5;
            let t = d / 14.0;
            let i = (y * s + x) * 4;
            if eye {
                buf[i..i + 3].copy_from_slice(&[10, 18, 40]);
            } else {
                buf[i] = (70.0 + 60.0 * t).round() as u8;
                buf[i + 1] = (240.0 - 90.0 * t).round() as u8;
                buf[i + 2] = 255;
            }
            buf[i + 3] = (a * 255.0).round() as u8;
        }
    }
    buf
}

fn menu(be: &Backend) -> Menu {
    let m = Menu::new();
    let ca = |c| Some(Accelerator::new(MMods::CONTROL | MMods::ALT, c));
    let _ = m.append(&MenuItem::with_id("open", "Open Mission Control", true, ca(MCode::KeyM)));
    if be.any_ai() {
        let label = if be.claude.found() { "Ask Claude" } else { "Ask Codex" };
        let _ = m.append(&MenuItem::with_id("chat", label, true, ca(MCode::Space)));
    }
    let _ = m.append(&PredefinedMenuItem::separator());
    let _ = m.append(&CheckMenuItem::with_id("openAtLogin", "Launch at startup", true, be.on("openAtLogin"), None));
    let _ = m.append(&CheckMenuItem::with_id("idleOnFullscreen", "Idle when an app is fullscreen", true, be.on("idleOnFullscreen"), None));
    if cfg!(windows) {
        let _ = m.append(&CheckMenuItem::with_id("extremeMode", "Extreme mode (themed taskbar)", true, be.on("extremeMode"), None));
    }
    let _ = m.append(&MenuItem::with_id("reload", "Reload widget", true, None));
    let _ = m.append(&PredefinedMenuItem::separator());
    let _ = m.append(&MenuItem::with_id("quit", "Quit", true, None));
    m
}

impl Tray {
    pub fn new(be: &Arc<Backend>) -> Option<Self> {
        MenuEvent::set_event_handler(Some(std::sync::Arc::new(|e: MenuEvent| emit("tray", e.id().0.clone()))));
        TrayIconEvent::set_event_handler(Some(|e: TrayIconEvent| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                emit("open", "panel");
            }
        }));
        let icon = tray_icon::Icon::from_rgba(tray_rgba(), 32, 32).ok()?;
        let icon = TrayIconBuilder::new()
            .with_icon(icon)
            .with_tooltip("Mission Control")
            .with_menu(Box::new(menu(be)))
            .with_menu_on_left_click(false)
            .build()
            .ok()?;

        let keys = GlobalHotKeyManager::new().ok();
        if let Some(k) = &keys {
            let panel = HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyM);
            let chat = HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::Space);
            let _ = k.register(panel);
            let _ = k.register(chat);
            let (pid, cid) = (panel.id(), chat.id());
            GlobalHotKeyEvent::set_event_handler(Some(move |e: GlobalHotKeyEvent| {
                if e.state != HotKeyState::Pressed {
                    return;
                }
                if e.id == pid {
                    emit("open", "panel");
                } else if e.id == cid {
                    emit("open", "chat");
                }
            }));
        }
        Some(Self { icon, _keys: keys })
    }

    pub fn update(&self, be: &Backend, idle: bool) {
        let _ = self.icon.set_tooltip(Some(if idle { "Mission Control (idle: fullscreen app)" } else { "Mission Control" }));
        self.icon.set_menu(Some(Box::new(menu(be))));
    }
}

// ---------------- single instance ----------------
// The first instance listens on a loopback port written to the settings folder; a second launch asks it
// to open the panel and exits.
fn port_file() -> std::path::PathBuf {
    let dir = std::env::var("MC_SETTINGS_DIR").map(std::path::PathBuf::from).unwrap_or_else(|_| dirs::config_dir().unwrap_or_else(crate::util::home).join("Mission Control"));
    dir.join("native-instance.port")
}

/// Returns false when another instance is running (it has been told to open the panel).
pub fn claim_instance() -> bool {
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpListener, TcpStream};
    use std::time::Duration;
    if let Ok(p) = std::fs::read_to_string(port_file()) {
        if let Ok(port) = p.trim().parse::<u16>() {
            let addr = SocketAddr::from(([127, 0, 0, 1], port));
            if let Ok(mut s) = TcpStream::connect_timeout(&addr, Duration::from_millis(300)) {
                let _ = s.set_read_timeout(Some(Duration::from_millis(500)));
                let _ = s.write_all(b"mission-control:open\n");
                let mut ack = [0u8; 2];
                if s.read(&mut ack).map(|n| n > 0 && &ack[..n.min(2)] == b"ok").unwrap_or(false) {
                    return false;
                }
            }
        }
    }
    let Ok(l) = TcpListener::bind(("127.0.0.1", 0)) else { return true };
    if let Ok(a) = l.local_addr() {
        let f = port_file();
        if let Some(d) = f.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(f, a.port().to_string());
    }
    std::thread::Builder::new()
        .name("mc-instance".into())
        .spawn(move || {
            for s in l.incoming().flatten() {
                let mut s = s;
                let _ = s.set_read_timeout(Some(Duration::from_millis(500)));
                let mut buf = [0u8; 64];
                if let Ok(n) = s.read(&mut buf) {
                    if buf[..n].starts_with(b"mission-control:open") {
                        let _ = s.write_all(b"ok");
                        emit("open", "panel");
                    }
                }
            }
        })
        .ok();
    true
}
