// Extreme mode dock (Windows): a centred, Mission Control-themed dock that replaces the full-width
// taskbar while Extreme mode is on. Start button, pinned apps (the taskbar's own pins), running apps,
// macOS-style magnification, running dots, launch bounce, hover labels and a right-click menu.
//
// It is a layered window drawn on the CPU (tiny-skia) from its own thread, so it needs no GPU context
// and only redraws while something moves. The Windows taskbar is set to auto-hide (restored when the
// mode is turned off); it still slides up at the bottom edge for the tray and clock, and the dock steps
// aside while it is up. The dock reserves its strip like the taskbar did (an AppBar), so maximised
// windows end above it.
#![cfg(windows)]
use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};
use tiny_skia::{Color, FillRule, GradientStop, LinearGradient, Paint, PathBuilder, Pixmap, PixmapPaint, Point, SpreadMode, Stroke, Transform};
use windows::core::{w, Interface, HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::Controls::IImageList;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

pub enum Msg {
    On(bool),
    /// The widget is idle (a fullscreen app is in front): hide
    Idle(bool),
}

pub struct Dock {
    tx: Sender<Msg>,
}

impl Dock {
    pub fn spawn() -> Self {
        let (tx, rx) = channel();
        std::thread::Builder::new().name("mc-dock".into()).spawn(move || run(rx)).ok();
        Self { tx }
    }
    pub fn send(&self, m: Msg) {
        let _ = self.tx.send(m);
    }
}

// ---------------- taskbar auto-hide (called from the backend, which keeps the backup) ----------------
pub fn taskbar_autohide() -> bool {
    unsafe {
        let mut d = APPBARDATA { cbSize: std::mem::size_of::<APPBARDATA>() as u32, ..Default::default() };
        SHAppBarMessage(ABM_GETSTATE, &mut d) & ABS_AUTOHIDE as usize != 0
    }
}
pub fn set_taskbar_autohide(on: bool) {
    unsafe {
        let mut d = APPBARDATA { cbSize: std::mem::size_of::<APPBARDATA>() as u32, ..Default::default() };
        d.hWnd = FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()).unwrap_or_default();
        d.lParam = LPARAM(if on { ABS_AUTOHIDE as isize } else { ABS_ALWAYSONTOP as isize });
        SHAppBarMessage(ABM_SETSTATE, &mut d);
    }
}

// ---------------- layout (logical px; multiplied by the DPI scale) ----------------
const ICON: f32 = 36.0;
const GAP: f32 = 6.0;
const PAD: f32 = 7.0;
const DOCK_H: f32 = 48.0;
const MARGIN: f32 = 5.0;
const MAG: f32 = 1.5;
/// room above the dock for magnified icons and the name label
const WIN_H: f32 = 108.0;
/// icon bottom above the dock's bottom edge (the running dot sits in this gap)
const ICON_LIFT: f32 = 7.0;
const CALLBACK: u32 = WM_APP + 7;
const WM_MOUSELEAVE_: u32 = 0x02A3;

#[derive(Clone)]
struct Item {
    key: String,
    name: String,
    launch: Option<PathBuf>,
    pinned: bool,
    start: bool,
    windows: Vec<HWND>,
    /// the file whose icon represents the item (a shortcut's target rather than the shortcut)
    icon_from: Option<PathBuf>,
}

struct St {
    hwnd: HWND,
    scale: f32,
    mon: RECT,
    on: bool,
    idle: bool,
    items: Vec<Item>,
    icons: HashMap<String, Option<Pixmap>>,
    font: Option<FontRef<'static>>,
    // animation
    cursor: Option<(f32, f32)>,
    mag: f32,
    show: f32,
    bounce: HashMap<String, Instant>,
    active: Option<String>,
    win_rect: (i32, i32, i32, i32),
    appbar: bool,
    last_refresh: Instant,
    last_frame: Instant,
    dirty: bool,
    hit: Vec<(f32, f32, usize)>,
    pressed: Option<usize>,
    /// the layered window's bitmap, kept between frames (memory DC, DIB, pixels, w, h)
    dib: Option<(HDC, HBITMAP, *mut u8, i32, i32)>,
}

thread_local! {
    static STATE: RefCell<Option<St>> = const { RefCell::new(None) };
}

/// State access that is safe when the window proc is re-entered (ShellExecute and shell messages pump
/// messages): a nested call just skips instead of panicking.
fn with<R>(f: impl FnOnce(&mut St) -> R) -> Option<R> {
    STATE.with(|s| s.try_borrow_mut().ok().and_then(|mut b| b.as_mut().map(f)))
}

// ---------------- apps: pins + running windows ----------------
fn exe_of_pid(pid: u32) -> Option<PathBuf> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut n = buf.len() as u32;
        let r = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut n);
        let _ = CloseHandle(h);
        r.ok()?;
        Some(PathBuf::from(String::from_utf16_lossy(&buf[..n as usize])))
    }
}

fn window_exe(h: HWND) -> Option<PathBuf> {
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, Some(&mut pid));
        let exe = exe_of_pid(pid)?;
        // Store apps live inside ApplicationFrameHost: use the hosted app's process instead
        if exe.file_name().is_some_and(|f| f.eq_ignore_ascii_case("ApplicationFrameHost.exe")) {
            let mut found: Option<PathBuf> = None;
            let _ = EnumChildWindows(Some(h), Some(child_cb), LPARAM(&mut (pid, &mut found) as *mut _ as isize));
            return found.or(Some(exe));
        }
        Some(exe)
    }
}

unsafe extern "system" fn child_cb(h: HWND, lp: LPARAM) -> windows_core::BOOL {
    let (host, found) = &mut *(lp.0 as *mut (u32, &mut Option<PathBuf>));
    let mut pid = 0u32;
    GetWindowThreadProcessId(h, Some(&mut pid));
    if pid != *host {
        **found = exe_of_pid(pid);
        return false.into();
    }
    true.into()
}

/// The windows the taskbar would show a button for.
fn taskbar_windows() -> Vec<HWND> {
    unsafe extern "system" fn cb(h: HWND, lp: LPARAM) -> windows_core::BOOL {
        let out = &mut *(lp.0 as *mut Vec<HWND>);
        if !IsWindowVisible(h).as_bool() {
            return true.into();
        }
        let ex = GetWindowLongW(h, GWL_EXSTYLE) as u32;
        if ex & WS_EX_TOOLWINDOW.0 != 0 || ex & WS_EX_NOACTIVATE.0 != 0 {
            return true.into();
        }
        let owner = GetWindow(h, GW_OWNER).unwrap_or_default();
        if !owner.0.is_null() && ex & WS_EX_APPWINDOW.0 == 0 {
            return true.into();
        }
        if GetWindowTextLengthW(h) == 0 {
            return true.into();
        }
        let mut cloaked: u32 = 0;
        let _ = DwmGetWindowAttribute(h, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut _, 4);
        if cloaked != 0 {
            return true.into();
        }
        out.push(h);
        true.into()
    }
    let mut v: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut v as *mut _ as isize));
    }
    v
}

fn lnk_target(lnk: &Path) -> Option<PathBuf> {
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let pf: IPersistFile = link.cast().ok()?;
        pf.Load(&HSTRING::from(lnk.as_os_str()), STGM_READ).ok()?;
        let mut buf = [0u16; 1024];
        link.GetPath(&mut buf, std::ptr::null_mut(), 0).ok()?;
        let n = buf.iter().position(|c| *c == 0).unwrap_or(0);
        if n == 0 {
            return None;
        }
        Some(PathBuf::from(String::from_utf16_lossy(&buf[..n])))
    }
}

fn file_description(exe: &Path) -> Option<String> {
    unsafe {
        let p = HSTRING::from(exe.as_os_str());
        let size = windows::Win32::Storage::FileSystem::GetFileVersionInfoSizeW(&p, None);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        windows::Win32::Storage::FileSystem::GetFileVersionInfoW(&p, None, size, data.as_mut_ptr() as *mut _).ok()?;
        let mut ptr: *mut core::ffi::c_void = std::ptr::null_mut();
        let mut len = 0u32;
        // first translation
        if !windows::Win32::Storage::FileSystem::VerQueryValueW(data.as_ptr() as *const _, w!("\\VarFileInfo\\Translation"), &mut ptr, &mut len).as_bool() || len < 4 {
            return None;
        }
        let lang = *(ptr as *const u16);
        let cp = *(ptr as *const u16).add(1);
        let q = HSTRING::from(format!("\\StringFileInfo\\{lang:04x}{cp:04x}\\FileDescription"));
        if !windows::Win32::Storage::FileSystem::VerQueryValueW(data.as_ptr() as *const _, &q, &mut ptr, &mut len).as_bool() || len == 0 {
            return None;
        }
        let s = std::slice::from_raw_parts(ptr as *const u16, len as usize);
        let s = String::from_utf16_lossy(s).trim_end_matches('\0').trim().to_string();
        (!s.is_empty()).then_some(s)
    }
}

fn pretty_stem(p: &Path) -> String {
    let s = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => s,
    }
}

fn collect(names: &mut HashMap<String, String>) -> Vec<Item> {
    let mut items = vec![Item { key: "::start".into(), name: "Start".into(), launch: None, pinned: true, start: true, windows: vec![], icon_from: None }];
    // the taskbar's own pins, in order (Explorer keeps them as shortcuts in this folder)
    // (backslashes only: the shell's icon lookup rejects mixed separators)
    let dir = dirs::data_dir().unwrap_or_default().join("Microsoft").join("Internet Explorer").join("Quick Launch").join("User Pinned").join("TaskBar");
    let mut pins: Vec<PathBuf> = std::fs::read_dir(&dir).map(|r| r.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("lnk"))).collect()).unwrap_or_default();
    pins.sort();
    for lnk in pins {
        let target = lnk_target(&lnk);
        let key = target.as_ref().map(|t| t.to_string_lossy().to_lowercase()).unwrap_or_else(|| lnk.to_string_lossy().to_lowercase());
        if items.iter().any(|i| i.key == key) {
            continue;
        }
        // the shortcut carries the right icon (web apps point at a browser proxy, Discord at its updater)
        let _ = target;
        items.push(Item { key, name: pretty_stem(&lnk), launch: Some(lnk), pinned: true, start: false, windows: vec![], icon_from: None });
    }
    for h in taskbar_windows() {
        let Some(exe) = window_exe(h) else { continue };
        // Store apps whose host couldn't be resolved: one item per window, named by its title
        if exe.file_name().is_some_and(|f| f.eq_ignore_ascii_case("ApplicationFrameHost.exe")) {
            let title = window_title(h);
            let key = format!("afh:{}", title.to_lowercase());
            if let Some(it) = items.iter_mut().find(|i| i.key == key) {
                it.windows.push(h);
            } else {
                items.push(Item { key, name: title, launch: None, pinned: false, start: false, windows: vec![h], icon_from: None });
            }
            continue;
        }
        let key = exe.to_string_lossy().to_lowercase();
        let stem = exe.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
        // A pin whose shortcut starts a launcher (Discord's Update.exe...) still owns the app's windows
        if let Some(it) = items.iter_mut().find(|i| i.key == key || (i.pinned && !i.start && i.name.to_lowercase() == stem)) {
            it.windows.push(h);
            continue;
        }
        let name = names
            .entry(key.clone())
            .or_insert_with(|| if stem == "explorer" { "File Explorer".into() } else { file_description(&exe).unwrap_or_else(|| pretty_stem(&exe)) })
            .clone();
        items.push(Item { key, name, launch: Some(exe.clone()), pinned: false, start: false, windows: vec![h], icon_from: Some(exe) });
    }
    items
}

// ---------------- icons ----------------
unsafe fn hicon_to_pixmap(icon: HICON) -> Option<Pixmap> {
    let mut ii = ICONINFO::default();
    GetIconInfo(icon, &mut ii).ok()?;
    let mut bm = BITMAP::default();
    let hb = if !ii.hbmColor.is_invalid() { ii.hbmColor } else { ii.hbmMask };
    GetObjectW(hb.into(), std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut _));
    let (w, h) = (bm.bmWidth, bm.bmHeight);
    let mut out = None;
    if w > 0 && h > 0 && !ii.hbmColor.is_invalid() {
        let dc = GetDC(None);
        let mut bmi = BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default() }, ..Default::default() };
        let mut px = vec![0u8; (w * h * 4) as usize];
        if GetDIBits(dc, ii.hbmColor, 0, h as u32, Some(px.as_mut_ptr() as *mut _), &mut bmi, DIB_RGB_COLORS) != 0 {
            let has_alpha = px.chunks(4).any(|c| c[3] != 0);
            if !has_alpha {
                for c in px.chunks_mut(4) {
                    c[3] = 255;
                }
            }
            if let Some(mut pm) = Pixmap::new(w as u32, h as u32) {
                let d = pm.data_mut();
                for (i, c) in px.chunks(4).enumerate() {
                    let a = c[3] as u32;
                    let pre = |v: u8| ((v as u32 * a + 127) / 255) as u8;
                    d[i * 4] = pre(c[2]);
                    d[i * 4 + 1] = pre(c[1]);
                    d[i * 4 + 2] = pre(c[0]);
                    d[i * 4 + 3] = a as u8;
                }
                out = Some(pm);
            }
        }
        ReleaseDC(None, dc);
    }
    if !ii.hbmColor.is_invalid() {
        let _ = DeleteObject(ii.hbmColor.into());
    }
    if !ii.hbmMask.is_invalid() {
        let _ = DeleteObject(ii.hbmMask.into());
    }
    out
}

fn prescale(src: &Pixmap, size: u32) -> Pixmap {
    let side = src.width().max(src.height());
    if side <= size {
        return src.clone();
    }
    let Some(mut out) = Pixmap::new(size, size) else { return src.clone() };
    let k = size as f32 / side as f32;
    // two steps for big downscales keeps it sharp without aliasing
    let mid = if k < 0.35 { Some(((side as f32) * 0.5).round() as u32) } else { None };
    let pp = PixmapPaint { quality: tiny_skia::FilterQuality::Bicubic, ..Default::default() };
    match mid.and_then(|m| Pixmap::new(m, m).map(|p| (m, p))) {
        Some((m, mut half)) => {
            let k1 = m as f32 / side as f32;
            half.draw_pixmap(0, 0, src.as_ref(), &pp, Transform::from_scale(k1, k1), None);
            let k2 = size as f32 / m as f32;
            out.draw_pixmap(0, 0, half.as_ref(), &pp, Transform::from_scale(k2, k2), None);
        }
        None => out.draw_pixmap(0, 0, src.as_ref(), &pp, Transform::from_scale(k, k), None),
    }
    out
}

/// Opaque bounding box size (some apps only have a small icon inside the 256 px "jumbo" canvas).
fn content_size(p: &Pixmap) -> u32 {
    let (w, h) = (p.width(), p.height());
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for y in 0..h {
        for x in 0..w {
            if p.pixel(x, y).is_some_and(|c| c.alpha() > 16) {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
    }
    if x1 < x0 {
        0
    } else {
        (x1 - x0 + 1).max(y1 - y0 + 1)
    }
}

fn window_title(h: HWND) -> String {
    unsafe {
        let mut buf = [0u16; 256];
        let n = GetWindowTextW(h, &mut buf);
        String::from_utf16_lossy(&buf[..n.max(0) as usize])
    }
}

fn load_icon(item: &Item) -> Option<Pixmap> {
    unsafe {
        let Some(path) = item.icon_from.as_ref().filter(|p| p.exists()).or(item.launch.as_ref()) else { return window_icon(item) };
        let mut info = SHFILEINFOW::default();
        let r = SHGetFileInfoW(&HSTRING::from(path.as_os_str()), windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES(0), Some(&mut info), std::mem::size_of::<SHFILEINFOW>() as u32, SHGFI_SYSICONINDEX);
        if r == 0 {
            return window_icon(item);
        }
        for size in [SHIL_JUMBO, SHIL_EXTRALARGE] {
            if let Ok(list) = SHGetImageList::<IImageList>(size as i32) {
                if let Ok(icon) = list.GetIcon(info.iIcon, 0x1 /* ILD_TRANSPARENT */) {
                    let pm = hicon_to_pixmap(icon);
                    let _ = DestroyIcon(icon);
                    if let Some(pm) = pm {
                        // a jumbo slot holding only a 32/48 px icon would look tiny: use the 48 px one
                        if size == SHIL_JUMBO && content_size(&pm) < 100 {
                            continue;
                        }
                        return Some(pm);
                    }
                }
            }
        }
        window_icon(item)
    }
}

fn window_icon(item: &Item) -> Option<Pixmap> {
    unsafe {
        let h = *item.windows.first()?;
        let mut r: usize = 0;
        let _ = SendMessageTimeoutW(h, WM_GETICON, WPARAM(ICON_BIG as usize), LPARAM(0), SMTO_ABORTIFHUNG, 200, Some(&mut r));
        let mut icon = HICON(r as *mut _);
        if icon.0.is_null() {
            icon = HICON(GetClassLongPtrW(h, GCLP_HICON) as *mut _);
        }
        if icon.0.is_null() {
            return None;
        }
        hicon_to_pixmap(icon)
    }
}

// ---------------- drawing ----------------
fn rrect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    let r = r.min(w / 2.0).min(h / 2.0);
    let k = 0.5523 * r;
    let mut p = PathBuilder::new();
    p.move_to(x + r, y);
    p.line_to(x + w - r, y);
    p.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    p.line_to(x + w, y + h - r);
    p.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    p.line_to(x + r, y + h);
    p.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    p.line_to(x, y + r);
    p.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    p.close();
    p.finish()
}

fn solid(r: u8, g: u8, b: u8, a: f32) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(Color::from_rgba8(r, g, b, (a * 255.0).round().clamp(0.0, 255.0) as u8));
    p.anti_alias = true;
    p
}

fn vgrad(y0: f32, y1: f32, stops: Vec<GradientStop>) -> Paint<'static> {
    let mut p = Paint::default();
    p.anti_alias = true;
    if let Some(s) = LinearGradient::new(Point::from_xy(0.0, y0), Point::from_xy(0.0, y1), stops, SpreadMode::Pad, Transform::identity()) {
        p.shader = s;
    }
    p
}
fn hgrad(x0: f32, x1: f32, stops: Vec<GradientStop>) -> Paint<'static> {
    let mut p = Paint::default();
    p.anti_alias = true;
    if let Some(s) = LinearGradient::new(Point::from_xy(x0, 0.0), Point::from_xy(x1, 0.0), stops, SpreadMode::Pad, Transform::identity()) {
        p.shader = s;
    }
    p
}
fn stop(pos: f32, r: u8, g: u8, b: u8, a: f32) -> GradientStop {
    GradientStop::new(pos, Color::from_rgba8(r, g, b, (a * 255.0).round().clamp(0.0, 255.0) as u8))
}

/// Soft glow: stacked, growing translucent rounded rects.
fn glow(pm: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, r: f32, rgb: (u8, u8, u8), a: f32, spread: f32) {
    let n = 3;
    for i in (1..=n).rev() {
        let e = spread * i as f32 / n as f32;
        if let Some(p) = rrect(x - e, y - e, w + 2.0 * e, h + 2.0 * e, r + e) {
            pm.fill_path(&p, &solid(rgb.0, rgb.1, rgb.2, a / n as f32), FillRule::Winding, Transform::identity(), None);
        }
    }
}

fn text(pm: &mut Pixmap, font: &FontRef, s: &str, size: f32, x: f32, y_base: f32, rgb: (u8, u8, u8)) {
    let sf = font.as_scaled(PxScale::from(size));
    let mut pen = x;
    let (w, h) = (pm.width() as i32, pm.height() as i32);
    let data = pm.data_mut();
    let mut prev = None;
    for ch in s.chars() {
        let id = sf.glyph_id(ch);
        if let Some(p) = prev {
            pen += sf.kern(p, id);
        }
        let g = id.with_scale_and_position(PxScale::from(size), ab_glyph::point(pen.round(), y_base.round()));
        pen += sf.h_advance(id);
        prev = Some(id);
        if let Some(og) = font.outline_glyph(g) {
            let b = og.px_bounds();
            og.draw(|gx, gy, c| {
                let (px, py) = (b.min.x as i32 + gx as i32, b.min.y as i32 + gy as i32);
                if px < 0 || py < 0 || px >= w || py >= h {
                    return;
                }
                let i = ((py * w + px) * 4) as usize;
                let a = c.clamp(0.0, 1.0);
                let blend = |dst: u8, src: u8| (src as f32 * a + dst as f32 * (1.0 - a)).round() as u8;
                data[i] = blend(data[i], rgb.0);
                data[i + 1] = blend(data[i + 1], rgb.1);
                data[i + 2] = blend(data[i + 2], rgb.2);
                data[i + 3] = blend(data[i + 3], 255);
            });
        }
    }
}

fn text_width(font: &FontRef, s: &str, size: f32) -> f32 {
    let sf = font.as_scaled(PxScale::from(size));
    let mut w = 0.0;
    let mut prev = None;
    for ch in s.chars() {
        let id = sf.glyph_id(ch);
        if let Some(p) = prev {
            w += sf.kern(p, id);
        }
        w += sf.h_advance(id);
        prev = Some(id);
    }
    w
}

/// The Start button: a dark tile with four glowing panes in the widget's colours.
fn draw_start(pm: &mut Pixmap, x: f32, y: f32, s: f32, hot: bool) {
    if let Some(p) = rrect(x, y, s, s, s * 0.24) {
        pm.fill_path(&p, &vgrad(y, y + s, vec![stop(0.0, 22, 34, 58, 1.0), stop(1.0, 8, 14, 28, 1.0)]), FillRule::Winding, Transform::identity(), None);
        let mut st = Stroke::default();
        st.width = (s * 0.025).max(1.0);
        pm.stroke_path(&p, &solid(62, 243, 255, if hot { 0.7 } else { 0.35 }), &st, Transform::identity(), None);
    }
    let pane = s * 0.21;
    let gap = s * 0.05;
    let x0 = x + (s - 2.0 * pane - gap) / 2.0;
    let y0 = y + (s - 2.0 * pane - gap) / 2.0;
    glow(pm, x0, y0, 2.0 * pane + gap, 2.0 * pane + gap, pane * 0.3, (62, 243, 255), 0.25, s * 0.12);
    for (i, j) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
        let (px, py) = (x0 + i as f32 * (pane + gap), y0 + j as f32 * (pane + gap));
        if let Some(p) = rrect(px, py, pane, pane, pane * 0.18) {
            pm.fill_path(&p, &hgrad(x0, x0 + 2.0 * pane + gap, vec![stop(0.0, 62, 243, 255, 1.0), stop(1.0, 155, 123, 255, 1.0)]), FillRule::Winding, Transform::identity(), None);
        }
    }
}

impl St {
    fn k(&self) -> f32 {
        self.scale
    }

    /// Sizes and x positions of every icon for the current pointer and magnification.
    fn layout(&self, w: f32) -> Vec<(f32, f32)> {
        let k = self.k();
        let n = self.items.len();
        let base = ICON * k;
        let slot = (ICON + GAP) * k;
        let base_total = n as f32 * base + (n.saturating_sub(1)) as f32 * GAP * k;
        let base_x0 = (w - base_total) / 2.0;
        let sigma = slot * 1.15;
        let mut sizes = Vec::with_capacity(n);
        for i in 0..n {
            let c = base_x0 + i as f32 * slot + base / 2.0;
            let f = match self.cursor {
                Some((cx, _)) if self.mag > 0.001 => {
                    let d = cx - c;
                    (-(d * d) / (2.0 * sigma * sigma)).exp()
                }
                _ => 0.0,
            };
            sizes.push(base * (1.0 + (MAG - 1.0) * f * self.mag));
        }
        let total: f32 = sizes.iter().sum::<f32>() + (n.saturating_sub(1)) as f32 * GAP * k;
        let mut x = (w - total) / 2.0;
        sizes
            .into_iter()
            .map(|s| {
                let r = (x, s);
                x += s + GAP * k;
                r
            })
            .collect()
    }

    fn render(&mut self) {
        let (_, _, w, h) = self.win_rect;
        let Some(mut pm) = Pixmap::new(w.max(1) as u32, h.max(1) as u32) else { return };
        let k = self.k();
        let (wf, hf) = (w as f32, h as f32);
        let lay = self.layout(wf);
        self.hit.clear();
        if lay.is_empty() {
            self.present(&pm);
            return;
        }
        // slide down when hidden
        let drop = (1.0 - self.show) * (DOCK_H + MARGIN + 8.0) * k;
        let dock_bottom = hf - MARGIN * k + drop;
        let dock_top = dock_bottom - DOCK_H * k;
        let left = lay[0].0 - PAD * k;
        let right = lay[lay.len() - 1].0 + lay[lay.len() - 1].1 + PAD * k;
        let rad = 15.0 * k;
        // background: navy glass with the widget's cyan rim and glow
        glow(&mut pm, left, dock_top, right - left, DOCK_H * k, rad, (62, 243, 255), 0.08, 10.0 * k);
        if let Some(p) = rrect(left, dock_top, right - left, DOCK_H * k, rad) {
            pm.fill_path(&p, &vgrad(dock_top, dock_bottom, vec![stop(0.0, 14, 22, 40, 0.94), stop(1.0, 6, 10, 20, 0.96)]), FillRule::Winding, Transform::identity(), None);
            let mut st = Stroke::default();
            st.width = k.max(1.0);
            pm.stroke_path(&p, &solid(110, 200, 255, 0.26), &st, Transform::identity(), None);
        }
        // glowing rim along the top, like the notch's bottom rim
        let rim_l = left + (right - left) * 0.12;
        let rim_r = right - (right - left) * 0.12;
        if let Some(p) = rrect(rim_l, dock_top, rim_r - rim_l, k.max(1.0), 0.0) {
            pm.fill_path(&p, &hgrad(rim_l, rim_r, vec![stop(0.0, 62, 243, 255, 0.0), stop(0.5, 62, 243, 255, 0.75), stop(1.0, 62, 243, 255, 0.0)]), FillRule::Winding, Transform::identity(), None);
        }
        let now = Instant::now();
        let mut hovered: Option<usize> = None;
        let pinned_end = self.items.iter().rposition(|i| i.pinned).unwrap_or(0);
        for (i, ((x, s), it)) in lay.iter().zip(self.items.clone()).enumerate() {
            let base_y = dock_bottom - ICON_LIFT * k - s;
            // launch bounce: two hops
            let mut by = 0.0;
            if let Some(t0) = self.bounce.get(&it.key) {
                let t = now.duration_since(*t0).as_secs_f32();
                if t < 1.1 {
                    let hop = (t / 0.55).floor();
                    let ph = (t % 0.55) / 0.55;
                    by = -(ph * std::f32::consts::PI).sin() * 12.0 * k * if hop == 0.0 { 1.0 } else { 0.45 };
                }
            }
            let y = base_y + by;
            self.hit.push((*x, *x + *s, i));
            let hot = self.cursor.is_some_and(|(cx, cy)| cx >= *x && cx <= *x + *s && cy >= dock_top - 30.0 * k);
            if hot {
                hovered = Some(i);
            }
            if it.start {
                draw_start(&mut pm, *x, y, *s, hot);
            } else if let Some(Some(icon)) = self.icons.get(&it.key) {
                let sc = *s / icon.width().max(icon.height()) as f32;
                let pp = PixmapPaint { quality: tiny_skia::FilterQuality::Bilinear, ..Default::default() };
                pm.draw_pixmap(0, 0, icon.as_ref(), &pp, Transform::from_row(sc, 0.0, 0.0, sc, *x, y), None);
            } else {
                // no icon: a themed tile with the initial
                if let Some(p) = rrect(*x, y, *s, *s, *s * 0.24) {
                    pm.fill_path(&p, &vgrad(y, y + s, vec![stop(0.0, 30, 48, 80, 1.0), stop(1.0, 12, 20, 38, 1.0)]), FillRule::Winding, Transform::identity(), None);
                }
                if let Some(f) = self.font.clone() {
                    let l: String = it.name.chars().take(1).collect::<String>().to_uppercase();
                    let fs = *s * 0.42;
                    let tw = text_width(&f, &l, fs);
                    text(&mut pm, &f, &l, fs, *x + (*s - tw) / 2.0, y + *s * 0.64, (220, 239, 255));
                }
            }
            // pressed: darken a little
            if self.pressed == Some(i) {
                if let Some(p) = rrect(*x, y, *s, *s, *s * 0.24) {
                    pm.fill_path(&p, &solid(0, 0, 0, 0.25), FillRule::Winding, Transform::identity(), None);
                }
            }
            // running indicator: a cyan dot (wider for the active app)
            if !it.windows.is_empty() {
                let active = self.active.as_deref() == Some(it.key.as_str());
                let (dw, dh) = if active { (12.0 * k, 2.5 * k) } else { (3.5 * k, 3.5 * k) };
                let dx = *x + *s / 2.0 - dw / 2.0;
                let dy = dock_bottom - 3.5 * k - dh / 2.0;
                glow(&mut pm, dx, dy, dw, dh, dh / 2.0, (62, 243, 255), 0.35, 4.0 * k);
                if let Some(p) = rrect(dx, dy, dw, dh, dh / 2.0) {
                    pm.fill_path(&p, &solid(62, 243, 255, 1.0), FillRule::Winding, Transform::identity(), None);
                }
            }
            // separator between pinned and the rest
            if i == pinned_end && i + 1 < self.items.len() {
                let sx = *x + *s + GAP * k / 2.0;
                if let Some(p) = rrect(sx - 0.5 * k, dock_top + 11.0 * k, k.max(1.0), (DOCK_H - 22.0) * k, 0.0) {
                    pm.fill_path(&p, &solid(110, 200, 255, 0.22), FillRule::Winding, Transform::identity(), None);
                }
            }
        }
        // label above the hovered icon
        if let (Some(i), Some(f)) = (hovered, self.font.clone()) {
            if self.mag > 0.6 {
                let (x, s) = lay[i];
                let name = self.items[i].name.clone();
                let fs = 11.5 * k;
                let tw = text_width(&f, &name, fs);
                let (lw, lh) = (tw + 18.0 * k, 22.0 * k);
                let lx = (x + s / 2.0 - lw / 2.0).clamp(2.0, wf - lw - 2.0);
                let ly = (dock_bottom - ICON_LIFT * k - s - 8.0 * k - lh).max(1.0);
                if let Some(p) = rrect(lx, ly, lw, lh, 10.0 * k) {
                    pm.fill_path(&p, &solid(10, 16, 30, 0.96), FillRule::Winding, Transform::identity(), None);
                    let mut st = Stroke::default();
                    st.width = k.max(1.0);
                    pm.stroke_path(&p, &solid(110, 200, 255, 0.3), &st, Transform::identity(), None);
                }
                text(&mut pm, &f, &name, fs, lx + 10.0 * k, ly + lh / 2.0 + fs * 0.36, (220, 239, 255));
            }
        }
        self.present(&pm);
    }

    fn present(&mut self, pm: &Pixmap) {
        let (x, y, w, h) = self.win_rect;
        unsafe {
            // one DIB for the window's size, reused every frame
            if !matches!(self.dib, Some((_, _, _, dw, dh)) if dw == w && dh == h) {
                if let Some((dc, bmp, _, _, _)) = self.dib.take() {
                    let _ = DeleteObject(bmp.into());
                    let _ = DeleteDC(dc);
                }
                let mem = CreateCompatibleDC(None);
                let bmi = BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default() }, ..Default::default() };
                let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
                let Ok(bmp) = CreateDIBSection(Some(mem), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) else {
                    let _ = DeleteDC(mem);
                    return;
                };
                SelectObject(mem, bmp.into());
                self.dib = Some((mem, bmp, bits as *mut u8, w, h));
            }
            let Some((mem, _, bits, _, _)) = self.dib else { return };
            // RGBA premultiplied -> BGRA premultiplied, straight into the DIB
            let src = pm.data();
            let dst = std::slice::from_raw_parts_mut(bits, src.len());
            for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
                d[0] = s[2];
                d[1] = s[1];
                d[2] = s[0];
                d[3] = s[3];
            }
            let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
            let (pt, sz, sp) = (POINT { x, y }, SIZE { cx: w, cy: h }, POINT::default());
            let _ = UpdateLayeredWindow(self.hwnd, None, Some(&pt), Some(&sz), Some(mem), Some(&sp), COLORREF(0), Some(&blend), ULW_ALPHA);
        }
    }

    fn place(&mut self) {
        let k = self.k();
        let n = self.items.len().max(1) as f32;
        let base_w = n * ICON * k + (n - 1.0) * GAP * k + 2.0 * PAD * k;
        // room for magnified icons and labels on both sides
        let w = (base_w + 2.0 * 120.0 * k).min((self.mon.right - self.mon.left) as f32) as i32;
        let h = (WIN_H * k) as i32;
        let x = self.mon.left + ((self.mon.right - self.mon.left) - w) / 2;
        let y = self.mon.bottom - h;
        self.win_rect = (x, y, w, h);
    }

    fn appbar(&mut self, on: bool) {
        // Diagnostic / capture runs never reserve screen space (they may be force-stopped)
        if std::env::var_os("MC_DIAG").is_some() || std::env::var_os("MC_SHOTS").is_some() {
            return;
        }
        unsafe {
            let mut d = APPBARDATA { cbSize: std::mem::size_of::<APPBARDATA>() as u32, hWnd: self.hwnd, uCallbackMessage: CALLBACK, ..Default::default() };
            if on && !self.appbar {
                SHAppBarMessage(ABM_NEW, &mut d);
                self.appbar = true;
            }
            if !on && self.appbar {
                SHAppBarMessage(ABM_REMOVE, &mut d);
                self.appbar = false;
                return;
            }
            if self.appbar {
                // reserve a full-width strip at the bottom, as tall as the dock
                let reserve = ((DOCK_H + MARGIN + 3.0) * self.k()) as i32;
                d.uEdge = ABE_BOTTOM;
                d.rc = RECT { left: self.mon.left, right: self.mon.right, top: self.mon.bottom - reserve, bottom: self.mon.bottom };
                SHAppBarMessage(ABM_QUERYPOS, &mut d);
                d.rc.top = d.rc.bottom - reserve;
                SHAppBarMessage(ABM_SETPOS, &mut d);
            }
        }
    }

    fn click(&mut self, i: usize, button: u32) {
        let Some(it) = self.items.get(i).cloned() else { return };
        unsafe {
            if it.start {
                let mut inputs = [INPUT::default(), INPUT::default()];
                inputs[0].r#type = INPUT_KEYBOARD;
                inputs[0].Anonymous.ki = KEYBDINPUT { wVk: VK_LWIN, ..Default::default() };
                inputs[1].r#type = INPUT_KEYBOARD;
                inputs[1].Anonymous.ki = KEYBDINPUT { wVk: VK_LWIN, dwFlags: KEYEVENTF_KEYUP, ..Default::default() };
                SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
                return;
            }
            let launch = |it: &Item| {
                if let Some(p) = &it.launch {
                    ShellExecuteW(None, w!("open"), &HSTRING::from(p.as_os_str()), None, None, SW_SHOWNORMAL);
                }
            };
            if button == 2 || it.windows.is_empty() {
                // middle click or not running: open (another) one, with a bounce
                launch(&it);
                self.bounce.insert(it.key.clone(), Instant::now());
                return;
            }
            let fg = GetForegroundWindow();
            if let Some(pos) = it.windows.iter().position(|h| *h == fg) {
                if it.windows.len() == 1 {
                    // the active app: minimise, like the taskbar
                    let _ = ShowWindow(fg, SW_MINIMIZE);
                } else {
                    activate(it.windows[(pos + 1) % it.windows.len()]);
                }
            } else {
                activate(it.windows[0]);
            }
        }
    }

    fn menu(&mut self, i: usize, sx: i32, sy: i32) {
        let Some(it) = self.items.get(i).cloned() else { return };
        if it.start {
            return;
        }
        unsafe {
            let Ok(m) = CreatePopupMenu() else { return };
            let _ = AppendMenuW(m, MF_STRING | MF_GRAYED, 0, &HSTRING::from(it.name.as_str()));
            let _ = AppendMenuW(m, MF_SEPARATOR, 0, PCWSTR::null());
            let _ = AppendMenuW(m, MF_STRING, 1, w!("New window"));
            if !it.windows.is_empty() {
                let label = if it.windows.len() > 1 { "Close all windows" } else { "Close window" };
                let _ = AppendMenuW(m, MF_STRING, 2, &HSTRING::from(label));
            }
            let _ = SetForegroundWindow(self.hwnd);
            let cmd = TrackPopupMenu(m, TPM_RETURNCMD | TPM_BOTTOMALIGN | TPM_CENTERALIGN | TPM_NONOTIFY, sx, sy, None, self.hwnd, None);
            let _ = DestroyMenu(m);
            match cmd.0 {
                1 => {
                    if let Some(p) = &it.launch {
                        ShellExecuteW(None, w!("open"), &HSTRING::from(p.as_os_str()), None, None, SW_SHOWNORMAL);
                        self.bounce.insert(it.key.clone(), Instant::now());
                    }
                }
                2 => {
                    for h in &it.windows {
                        let _ = PostMessageW(Some(*h), WM_CLOSE, WPARAM(0), LPARAM(0));
                    }
                }
                _ => {}
            }
        }
    }

    fn item_at(&self, x: f32) -> Option<usize> {
        self.hit.iter().find(|(l, r, _)| x >= *l && x <= *r).map(|h| h.2)
    }
}

unsafe fn activate(h: HWND) {
    if IsIconic(h).as_bool() {
        let _ = ShowWindow(h, SW_RESTORE);
    }
    // we just received the click, so Windows lets us hand the foreground over
    if !SetForegroundWindow(h).as_bool() {
        let fg = GetForegroundWindow();
        let a = GetWindowThreadProcessId(fg, None);
        let b = GetCurrentThreadId();
        let _ = AttachThreadInput(b, a, true);
        let _ = SetForegroundWindow(h);
        let _ = AttachThreadInput(b, a, false);
    }
}

unsafe extern "system" fn wndproc(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let x = (lp.0 & 0xffff) as i16 as f32;
    let y = ((lp.0 >> 16) & 0xffff) as i16 as f32;
    match m {
        WM_MOUSEACTIVATE => return LRESULT(MA_NOACTIVATE as isize),
        WM_MOUSEMOVE => {
            with(|s| {
                if s.cursor.is_none() {
                    let mut t = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: h, dwHoverTime: 0 };
                    let _ = TrackMouseEvent(&mut t);
                }
                s.cursor = Some((x, y));
                s.dirty = true;
            });
            return LRESULT(0);
        }
        WM_MOUSELEAVE_ => {
            with(|s| {
                s.cursor = None;
                s.pressed = None;
                s.dirty = true;
            });
            return LRESULT(0);
        }
        WM_LBUTTONDOWN => {
            with(|s| {
                s.pressed = s.item_at(x);
                s.dirty = true;
            });
            return LRESULT(0);
        }
        WM_LBUTTONUP | WM_MBUTTONUP => {
            let btn = if m == WM_MBUTTONUP { 2 } else { 1 };
            with(|s| {
                let i = s.item_at(x);
                if let Some(i) = i {
                    if btn == 2 || s.pressed == Some(i) {
                        s.click(i, btn);
                    }
                }
                s.pressed = None;
                s.dirty = true;
            });
            return LRESULT(0);
        }
        WM_RBUTTONUP => {
            let mut pt = POINT { x: x as i32, y: y as i32 };
            let _ = ClientToScreen(h, &mut pt);
            // run the modal menu outside the state borrow
            let target = with(|s| s.item_at(x)).flatten();
            if let Some(i) = target {
                let st = STATE.with(|c| c.try_borrow_mut().ok().and_then(|mut b| b.take()));
                if let Some(mut st) = st {
                    st.menu(i, pt.x, pt.y);
                    STATE.with(|c| *c.borrow_mut() = Some(st));
                }
            }
            return LRESULT(0);
        }
        CALLBACK => {
            // the shell asks AppBars to re-check their position (taskbar or display changes)
            if wp.0 as u32 == ABN_POSCHANGED {
                with(|s| s.appbar(true));
            }
            return LRESULT(0);
        }
        _ => {}
    }
    DefWindowProcW(h, m, wp, lp)
}

fn primary_monitor() -> RECT {
    unsafe {
        let m = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if GetMonitorInfoW(m, &mut mi).as_bool() {
            mi.rcMonitor
        } else {
            RECT { left: 0, top: 0, right: 1920, bottom: 1080 }
        }
    }
}

/// The auto-hidden taskbar is up (pointer at the bottom edge, Start menu open...)
fn taskbar_revealed(mon: &RECT) -> bool {
    unsafe {
        let Ok(t) = FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) else { return false };
        let mut r = RECT::default();
        if GetWindowRect(t, &mut r).is_err() {
            return false;
        }
        // only an auto-hidden taskbar "comes up"; a permanent one (test runs) never hides the dock
        IsWindowVisible(t).as_bool() && r.top < mon.bottom - 8 && r.bottom > mon.bottom - 8 && taskbar_autohide()
    }
}

fn load_font() -> Option<FontRef<'static>> {
    let dir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
    for f in ["bahnschrift.ttf", "segoeui.ttf"] {
        if let Ok(b) = std::fs::read(Path::new(&dir).join("Fonts").join(f)) {
            let b: &'static [u8] = Box::leak(b.into_boxed_slice());
            if let Ok(font) = FontRef::try_from_slice(b) {
                return Some(font);
            }
        }
    }
    None
}

fn run(rx: Receiver<Msg>) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let inst = GetModuleHandleW(None).unwrap_or_default();
        let wc = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: inst.into(), lpszClassName: w!("MissionControlDock"), hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(), ..Default::default() };
        RegisterClassW(&wc);
        let Ok(hwnd) = CreateWindowExW(WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST, w!("MissionControlDock"), w!("Mission Control dock"), WS_POPUP, -32000, -32000, 10, 10, None, None, Some(inst.into()), None) else {
            return;
        };
        let scale = windows::Win32::UI::HiDpi::GetDpiForSystem() as f32 / 96.0;
        let now = Instant::now();
        STATE.with(|s| {
            *s.borrow_mut() = Some(St {
                hwnd,
                scale: scale.max(1.0),
                mon: primary_monitor(),
                on: false,
                idle: false,
                items: vec![],
                icons: HashMap::new(),
                font: load_font(),
                cursor: None,
                mag: 0.0,
                show: 0.0,
                bounce: HashMap::new(),
                active: None,
                win_rect: (0, 0, 1, 1),
                appbar: false,
                last_refresh: now - Duration::from_secs(10),
                last_frame: now,
                dirty: true,
                hit: vec![],
                pressed: None,
                dib: None,
            })
        });
        let mut names: HashMap<String, String> = HashMap::new();
        let mut shown = false;
        loop {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            let animating = with(|s| {
                let want_mag = if s.cursor.is_some() { 1.0 } else { 0.0 };
                let want_show = if s.on && !s.idle && !taskbar_revealed(&s.mon) { 1.0 } else { 0.0 };
                (s.mag - want_mag).abs() > 0.002 || (s.show - want_show).abs() > 0.002 || s.bounce.values().any(|t| t.elapsed().as_secs_f32() < 1.1) || s.dirty
            })
            .unwrap_or(false);
            let on = with(|s| s.on).unwrap_or(false);
            // While animating, wait for the compositor's next frame (vsync) rather than sleeping: Windows'
            // default timer granularity (15.6 ms) would make the motion uneven.
            let msg = if animating {
                let _ = windows::Win32::Graphics::Dwm::DwmFlush();
                match rx.try_recv() {
                    Ok(m) => Ok(m),
                    Err(std::sync::mpsc::TryRecvError::Empty) => Err(RecvTimeoutError::Timeout),
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => Err(RecvTimeoutError::Disconnected),
                }
            } else {
                rx.recv_timeout(Duration::from_millis(if on { 120 } else { 1000 }))
            };
            match msg {
                Ok(Msg::On(v)) => {
                    with(|s| {
                        s.on = v;
                        s.mon = primary_monitor();
                        s.dirty = true;
                        s.last_refresh = Instant::now() - Duration::from_secs(10);
                    });
                }
                Ok(Msg::Idle(v)) => {
                    with(|s| {
                        s.idle = v;
                        s.dirty = true;
                    });
                }
                Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {}
            }
            let _ = with(|s| {
                if !s.on && s.show < 0.002 {
                    if shown {
                        let _ = ShowWindow(s.hwnd, SW_HIDE);
                        s.appbar(false);
                        shown = false;
                    }
                    return;
                }
                // refresh apps once a second (and the active app every tick)
                if s.last_refresh.elapsed() > Duration::from_millis(1000) {
                    s.last_refresh = Instant::now();
                    let items = collect(&mut names);
                    let changed = items.len() != s.items.len() || items.iter().zip(&s.items).any(|(a, b)| a.key != b.key || a.windows.len() != b.windows.len());
                    for it in &items {
                        if !it.start && !s.icons.contains_key(&it.key) {
                            let icon = load_icon(it);
                            if std::env::var_os("MC_DOCK_DEBUG").is_some() {
                                eprintln!("[dock] {:?} key={} launch={:?} windows={} icon={:?}", it.name, it.key, it.launch, it.windows.len(), icon.as_ref().map(|p| (p.width(), content_size(p))));
                            }
                            // pre-scale once to the largest size it is drawn at, so each frame only does a
                            // small, cheap resample (smooth magnification even with big 256 px icons)
                            let target = (ICON * MAG * s.scale).ceil() as u32 + 2;
                            let icon = icon.map(|i| prescale(&i, target));
                            s.icons.insert(it.key.clone(), icon);
                        }
                    }
                    s.items = items;
                    if changed {
                        s.mon = primary_monitor();
                        s.place();
                        s.dirty = true;
                    }
                }
                let fg = GetForegroundWindow();
                let active = s.items.iter().find(|i| i.windows.contains(&fg)).map(|i| i.key.clone());
                if active != s.active {
                    s.active = active;
                    s.dirty = true;
                }
                // ease magnification and slide
                let dt = s.last_frame.elapsed().as_secs_f32().min(0.05);
                s.last_frame = Instant::now();
                let want_mag = if s.cursor.is_some() { 1.0 } else { 0.0 };
                let want_show = if s.on && !s.idle && !taskbar_revealed(&s.mon) { 1.0 } else { 0.0 };
                let ease = |v: &mut f32, t: f32, rate: f32| {
                    let k = 1.0 - (-rate * dt).exp();
                    *v += (t - *v) * k;
                    if (*v - t).abs() < 0.002 {
                        *v = t;
                    }
                };
                let (m0, s0) = (s.mag, s.show);
                ease(&mut s.mag, want_mag, 14.0);
                ease(&mut s.show, want_show, 12.0);
                let moving = m0 != s.mag || s0 != s.show || s.bounce.values().any(|t| t.elapsed().as_secs_f32() < 1.1);
                s.bounce.retain(|_, t| t.elapsed().as_secs_f32() < 1.2);
                if !shown && s.on {
                    s.place();
                    let (x, y, w, h) = s.win_rect;
                    let _ = SetWindowPos(s.hwnd, Some(HWND_TOPMOST), x, y, w, h, SWP_NOACTIVATE);
                    let _ = ShowWindow(s.hwnd, SW_SHOWNOACTIVATE);
                    s.appbar(true);
                    shown = true;
                }
                if !s.on && s.appbar {
                    s.appbar(false);
                }
                if moving || s.dirty {
                    s.dirty = false;
                    let t0 = Instant::now();
                    s.render();
                    if std::env::var_os("MC_DOCK_DEBUG").is_some() {
                        eprintln!("[dock] frame {:.2} ms", t0.elapsed().as_secs_f64() * 1000.0);
                    }
                }
            });
        }
        with(|s| s.appbar(false));
        let _ = DestroyWindow(hwnd);
        CoUninitialize();
    }
}

/// Removes the dock's reserved strip right away (quit path).
pub fn release_appbar() {
    unsafe {
        if let Ok(h) = FindWindowW(w!("MissionControlDock"), PCWSTR::null()) {
            let mut d = APPBARDATA { cbSize: std::mem::size_of::<APPBARDATA>() as u32, hWnd: h, ..Default::default() };
            SHAppBarMessage(ABM_REMOVE, &mut d);
        }
    }
}
