// Idle while a fullscreen app (game, video) is in front: the widget hides and all sampling stops.
use crate::backend::{emit, Backend};
use std::sync::Arc;
use std::time::Duration;

pub fn spawn(be: Arc<Backend>, own: Option<isize>) {
    std::thread::Builder::new()
        .name("mc-idle".into())
        .spawn(move || {
            let mut last = false;
            loop {
                std::thread::sleep(Duration::from_millis(1500));
                #[cfg(windows)]
                let full = be.on("idleOnFullscreen") && crate::native::is_foreground_fullscreen(&own.into_iter().collect::<Vec<_>>());
                #[cfg(target_os = "macos")]
                let full = {
                    let _ = own;
                    be.on("idleOnFullscreen") && crate::native::is_foreground_fullscreen()
                };
                if full != last {
                    last = full;
                    emit("fullscreen", full);
                }
            }
        })
        .ok();
}

/// Hide/show without activating (the user's fullscreen app keeps focus).
pub fn hide(h: Option<isize>) {
    #[cfg(windows)]
    if let Some(h) = h {
        crate::native::hide(h);
    }
    #[cfg(target_os = "macos")]
    if let Some(h) = h {
        unsafe {
            let view: &objc2_app_kit::NSView = &*(h as *const objc2_app_kit::NSView);
            if let Some(w) = view.window() {
                w.orderOut(None);
            }
        }
    }
}

pub fn show(h: Option<isize>) {
    #[cfg(windows)]
    if let Some(h) = h {
        crate::native::show_inactive(h);
    }
    #[cfg(target_os = "macos")]
    if let Some(h) = h {
        unsafe {
            let view: &objc2_app_kit::NSView = &*(h as *const objc2_app_kit::NSView);
            if let Some(w) = view.window() {
                w.orderFrontRegardless();
            }
        }
    }
}
