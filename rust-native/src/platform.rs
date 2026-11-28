// The few OS calls the window itself needs: where the pointer is (also while the window lets clicks
// through), the screen's work area, keeping the widget out of Alt+Tab / the Dock, and opening links.
// Positions are physical pixels with the origin at the primary screen's top-left.

#[cfg(windows)]
mod imp {
    use windows::core::{w, HSTRING};
    use windows::Win32::Foundation::{HWND, POINT, RECT};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::*;

    pub fn cursor() -> Option<(f64, f64)> {
        let mut p = POINT::default();
        unsafe { GetCursorPos(&mut p).ok()? };
        Some((p.x as f64, p.y as f64))
    }

    /// Primary screen work area (excludes the taskbar), physical pixels.
    pub fn work_area() -> Option<(f64, f64, f64, f64)> {
        let mut r = RECT::default();
        unsafe { SystemParametersInfoW(SPI_GETWORKAREA, 0, Some(&mut r as *mut _ as *mut _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)).ok()? };
        Some((r.left as f64, r.top as f64, (r.right - r.left) as f64, (r.bottom - r.top) as f64))
    }

    /// A tool window: no taskbar button and not in Alt+Tab, like Electron's "toolbar" window type.
    pub fn style_widget(hwnd: isize) {
        unsafe {
            // The whole window is client area. winit trims 1 px off the top of undecorated windows
            // (a hack to keep a drop shadow), and Windows paints that strip as a light line.
            let _ = windows::Win32::UI::Shell::SetWindowSubclass(HWND(hwnd as *mut _), Some(no_frame_proc), 0x4d43, 0);
        }
        set_click_through(hwnd, true);
    }

    unsafe extern "system" fn no_frame_proc(h: HWND, msg: u32, w: windows::Win32::Foundation::WPARAM, l: windows::Win32::Foundation::LPARAM, _id: usize, _data: usize) -> windows::Win32::Foundation::LRESULT {
        if msg == WM_NCCALCSIZE && w.0 != 0 {
            // leave the proposed rect as is: client area = window area, no non-client strip
            return windows::Win32::Foundation::LRESULT(0);
        }
        windows::Win32::UI::Shell::DefSubclassProc(h, msg, w, l)
    }

    /// Click-through on or off. A click-through window must be layered + transparent; a layered window
    /// only shows what it draws once SetLayeredWindowAttributes has been called, so do that every time.
    /// Cheap enough to call every frame: it only writes when something changed (winit rewrites the
    /// extended style whenever one of its own flags changes).
    pub fn set_click_through(hwnd: isize, through: bool) {
        unsafe {
            let h = HWND(hwnd as *mut _);
            let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
            let mut n = (ex | WS_EX_LAYERED.0 as isize | WS_EX_TOOLWINDOW.0 as isize) & !(WS_EX_APPWINDOW.0 as isize);
            if through {
                n |= WS_EX_TRANSPARENT.0 as isize;
            } else {
                n &= !(WS_EX_TRANSPARENT.0 as isize);
            }
            if n != ex {
                SetWindowLongPtrW(h, GWL_EXSTYLE, n);
                let _ = SetLayeredWindowAttributes(h, windows::Win32::Foundation::COLORREF(0), 255, LWA_ALPHA);
            }
            // A plain popup: a layered window that keeps caption styles gets a classic frame (the faint
            // "glass" border with a close button) drawn around it. winit adds them back whenever one
            // of its own flags changes, so this is checked every frame.
            let st = GetWindowLongPtrW(h, GWL_STYLE);
            let frame = (WS_CAPTION | WS_THICKFRAME | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX | WS_BORDER | WS_DLGFRAME).0 as isize;
            let ns = (st & !frame) | WS_POPUP.0 as isize;
            if ns != st {
                SetWindowLongPtrW(h, GWL_STYLE, ns);
                let _ = SetWindowPos(h, None, 0, 0, 0, 0, SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOOWNERZORDER);
                // Windows 11: no accent border and no rounded corners on the (invisible) window rect
                use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE};
                let none: u32 = 0xFFFF_FFFE; // DWMWA_COLOR_NONE
                let square: u32 = 1; // DWMWCP_DONOTROUND
                let _ = DwmSetWindowAttribute(h, DWMWA_BORDER_COLOR, &none as *const u32 as *const _, 4);
                let _ = DwmSetWindowAttribute(h, DWMWA_WINDOW_CORNER_PREFERENCE, &square as *const u32 as *const _, 4);
            }
        }
    }

    pub fn open_url(url: &str) {
        unsafe {
            ShellExecuteW(None, w!("open"), &HSTRING::from(url), None, None, SW_SHOWNORMAL);
        }
    }

    pub fn set_main_thread() {}
}

#[cfg(target_os = "macos")]
mod imp {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSEvent, NSScreen, NSWindowCollectionBehavior};
    use std::sync::atomic::{AtomicU64, Ordering};

    // Primary screen height (points) and backing scale, refreshed on the main thread
    static SCREEN_H: AtomicU64 = AtomicU64::new(0);
    static SCALE: AtomicU64 = AtomicU64::new(0);

    pub fn set_main_thread() {
        if let Some(mtm) = MainThreadMarker::new() {
            let screens = NSScreen::screens(mtm);
            if let Some(s) = screens.firstObject() {
                let f = s.frame();
                SCREEN_H.store(f.size.height.to_bits(), Ordering::Relaxed);
                SCALE.store(s.backingScaleFactor().to_bits(), Ordering::Relaxed);
            }
        }
    }

    fn scale() -> f64 {
        let s = f64::from_bits(SCALE.load(Ordering::Relaxed));
        if s > 0.0 { s } else { 2.0 }
    }

    pub fn cursor() -> Option<(f64, f64)> {
        let h = f64::from_bits(SCREEN_H.load(Ordering::Relaxed));
        if h <= 0.0 {
            return None;
        }
        let p = NSEvent::mouseLocation();
        let s = scale();
        Some((p.x * s, (h - p.y) * s))
    }

    pub fn work_area() -> Option<(f64, f64, f64, f64)> {
        let mtm = MainThreadMarker::new()?;
        let screens = NSScreen::screens(mtm);
        let s = screens.firstObject()?;
        let (f, v, k) = (s.frame(), s.visibleFrame(), s.backingScaleFactor());
        let top = f.size.height - (v.origin.y + v.size.height);
        Some((v.origin.x * k, top * k, v.size.width * k, v.size.height * k))
    }

    /// Visible on every Space and over full-screen apps' Spaces; no shadow.
    pub fn style_widget(ns_view: isize) {
        unsafe {
            let view: &objc2_app_kit::NSView = &*(ns_view as *const objc2_app_kit::NSView);
            if let Some(win) = view.window() {
                win.setCollectionBehavior(NSWindowCollectionBehavior::CanJoinAllSpaces | NSWindowCollectionBehavior::Stationary | NSWindowCollectionBehavior::FullScreenAuxiliary);
                win.setHasShadow(false);
            }
        }
    }

    pub fn open_url(url: &str) {
        let _ = std::process::Command::new("/usr/bin/open").arg(url).spawn();
    }
}

pub use imp::*;

// ---------------- dates in the user's style (toLocale*String in the renderer) ----------------
/// 12-hour clock with month-first dates for US English, 24-hour and day-first otherwise.
pub fn us_style() -> bool {
    static US: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *US.get_or_init(|| {
        #[cfg(windows)]
        unsafe {
            // Chromium (the other editions) formats with the display language, not the region format
            return windows::Win32::Globalization::GetUserDefaultUILanguage() == 0x0409;
        }
        #[allow(unreachable_code)]
        std::env::var("LANG").map(|l| l.starts_with("en_US")).unwrap_or(false)
    })
}
pub fn fmt_time(d: chrono::DateTime<chrono::Local>) -> String {
    if us_style() { d.format("%I:%M %p").to_string() } else { d.format("%H:%M").to_string() }
}
/// "03:33 · Sat 28 Nov" (clock in the panel's tab bar)
pub fn fmt_clock(d: chrono::DateTime<chrono::Local>) -> String {
    if us_style() { d.format("%I:%M %p · %a, %b %-d").to_string() } else { d.format("%H:%M · %a %-d %b").to_string() }
}
/// "28 Nov 03:33" (speed test history)
pub fn fmt_short(d: chrono::DateTime<chrono::Local>) -> String {
    if us_style() { d.format("%b %-d, %I:%M %p").to_string() } else { d.format("%-d %b %H:%M").to_string() }
}
