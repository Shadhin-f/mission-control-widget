// Extreme mode glow line (Windows): a 5 px strip on the taskbar's top edge. It is a layered Win32 window
// drawn on the CPU from its own small thread, so it needs no GPU context at all. Like the CSS version it
// only animates on events (grow in, a light sweep when the pointer reaches the taskbar, shrink away);
// while idle the thread sleeps on its channel.
#![cfg(windows)]
use crate::theme::Ease;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};
use windows::core::w;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;

pub enum Msg {
    /// Show (grow in) or hide (shrink away)
    On(bool),
    Hot(bool),
    /// Taskbar rect in physical pixels and the DPI scale; None hides the strip (auto-hide, side taskbar)
    Place(Option<(i32, i32, i32, f32)>),
}

pub struct Edge {
    tx: Sender<Msg>,
}

impl Edge {
    pub fn spawn() -> Self {
        let (tx, rx) = channel();
        std::thread::Builder::new().name("mc-edge".into()).spawn(move || run(rx)).ok();
        Self { tx }
    }
    pub fn send(&self, m: Msg) {
        let _ = self.tx.send(m);
    }
}

const GROW: Ease = Ease::Bez(0.2, 1.12, 0.32, 1.0);
const SWEEP: Ease = Ease::Bez(0.45, 0.0, 0.2, 1.0);

struct St {
    hwnd: HWND,
    on: bool,
    shown: bool,
    grow_from: f32,
    grow_to: f32,
    grow_t: Instant,
    hot: bool,
    op_from: f32,
    op_to: f32,
    op_t: Instant,
    sweep_t: Option<Instant>,
    place: Option<(i32, i32, i32, f32)>,
    buf: Vec<u32>,
}

fn tw(from: f32, to: f32, t: Instant, dur: f32, e: Ease) -> (f32, bool) {
    let k = t.elapsed().as_secs_f32() / dur;
    if k >= 1.0 {
        (to, true)
    } else {
        (from + (to - from) * e.at(k), false)
    }
}

impl St {
    fn grow(&self) -> (f32, bool) {
        tw(self.grow_from, self.grow_to, self.grow_t, 0.9, GROW)
    }
    fn opacity(&self) -> (f32, bool) {
        tw(self.op_from, self.op_to, self.op_t, 0.45, crate::theme::EASE)
    }

    fn render(&mut self) {
        let Some((x, y, w, scale)) = self.place else { return };
        let h = (5.0 * scale).round().max(3.0) as i32;
        let (grow, _) = self.grow();
        let (op, _) = self.opacity();
        let wf = w as f32;
        self.buf.clear();
        self.buf.resize((w * h) as usize, 0);
        let line_top = 1.0 * scale;
        let line_h = 2.0 * scale;
        let sweep = self.sweep_t.map(|t| {
            let k = (t.elapsed().as_secs_f32() / 1.15).min(1.0);
            let e = SWEEP.at(k);
            // ::after: 200 px wide, from left -220 px across the strip
            let left = (-220.0 + e * (wf / scale + 440.0)) * scale;
            (left, 200.0 * scale, 1.0 - e)
        });
        let half = wf * grow.max(0.0) / 2.0;
        let cx = wf / 2.0;
        for py in 0..h {
            let fy = py as f32 + 0.5;
            // coverage of the 2 px line, plus a soft 0 0 6px glow around it
            let d = if fy < line_top { line_top - fy } else if fy > line_top + line_h { fy - line_top - line_h } else { 0.0 };
            let line_a = if d == 0.0 { 1.0 } else { 0.0 };
            let glow_a = 0.55 * (-d / (3.0 * scale)).exp() * 0.6;
            let in_sweep_rows = fy >= line_top - scale && fy <= line_top + line_h + scale;
            for px in 0..w {
                let fx = px as f32 + 0.5;
                // scaleX from the centre
                if (fx - cx).abs() > half {
                    continue;
                }
                let u = if half > 0.0 { ((fx - cx) / half + 1.0) / 2.0 } else { 0.5 };
                // gradient: transparent, cyan .9 22%, violet .9 50%, cyan .9 78%, transparent
                let (r, g, b, a) = grad(u);
                let mut ca = a * line_a + 0.55 * glow_a * a.min(0.9) / 0.9;
                let (mut cr, mut cg, mut cb) = if line_a > 0.0 { (r, g, b) } else { (62.0, 243.0, 255.0) };
                ca *= op;
                if let (Some((sl, sw, sa)), true) = (sweep, in_sweep_rows) {
                    if fx >= sl && fx <= sl + sw {
                        let t = (fx - sl) / sw;
                        let wa = 0.95 * (1.0 - (t * 2.0 - 1.0).abs()) * sa;
                        // white sweep composited over the line
                        let out_a = wa + ca * (1.0 - wa);
                        if out_a > 0.0 {
                            cr = (255.0 * wa + cr * ca * (1.0 - wa)) / out_a;
                            cg = (255.0 * wa + cg * ca * (1.0 - wa)) / out_a;
                            cb = (255.0 * wa + cb * ca * (1.0 - wa)) / out_a;
                        }
                        ca = out_a;
                    }
                }
                if ca <= 0.0 {
                    continue;
                }
                let ca = ca.min(1.0);
                // premultiplied BGRA
                let p = |c: f32| (c * ca).round().clamp(0.0, 255.0) as u32;
                self.buf[(py * w + px) as usize] = ((ca * 255.0).round() as u32) << 24 | p(cr) << 16 | p(cg) << 8 | p(cb);
            }
        }
        unsafe { present(self.hwnd, x, y - (2.0 * scale).round() as i32, w, h, &self.buf) };
    }
}

fn grad(u: f32) -> (f32, f32, f32, f32) {
    let cyan = (62.0, 243.0, 255.0);
    let violet = (155.0, 123.0, 255.0);
    let stops = [(0.0, cyan, 0.0), (0.22, cyan, 0.9), (0.5, violet, 0.9), (0.78, cyan, 0.9), (1.0, cyan, 0.0)];
    for wdw in stops.windows(2) {
        let (a, b) = (wdw[0], wdw[1]);
        if u <= b.0 {
            let k = ((u - a.0) / (b.0 - a.0)).clamp(0.0, 1.0);
            let l = |x: f32, y: f32| x + (y - x) * k;
            return (l(a.1 .0, b.1 .0), l(a.1 .1, b.1 .1), l(a.1 .2, b.1 .2), l(a.2, b.2));
        }
    }
    (cyan.0, cyan.1, cyan.2, 0.0)
}

unsafe fn present(hwnd: HWND, x: i32, y: i32, w: i32, h: i32, px: &[u32]) {
    let screen = GetDC(None);
    let mem = CreateCompatibleDC(Some(screen));
    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default() },
        ..Default::default()
    };
    let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
    if let Ok(bmp) = CreateDIBSection(Some(mem), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
        std::ptr::copy_nonoverlapping(px.as_ptr(), bits as *mut u32, (w * h) as usize);
        let old = SelectObject(mem, bmp.into());
        let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
        let (pt, sz, src) = (POINT { x, y }, SIZE { cx: w, cy: h }, POINT::default());
        let _ = UpdateLayeredWindow(hwnd, Some(screen), Some(&pt), Some(&sz), Some(mem), Some(&src), COLORREF(0), Some(&blend), ULW_ALPHA);
        SelectObject(mem, old);
        let _ = DeleteObject(bmp.into());
    }
    let _ = DeleteDC(mem);
    ReleaseDC(None, screen);
}

unsafe extern "system" fn wndproc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if m == WM_MOUSEACTIVATE {
        return LRESULT(MA_NOACTIVATE as isize);
    }
    DefWindowProcW(h, m, w, l)
}

fn run(rx: Receiver<Msg>) {
    let hwnd = unsafe {
        let inst = GetModuleHandleW(None).unwrap_or_default();
        let wc = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: inst.into(), lpszClassName: w!("MissionControlEdge"), ..Default::default() };
        RegisterClassW(&wc);
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST,
            w!("MissionControlEdge"),
            w!("Mission Control edge"),
            WS_POPUP,
            -32000,
            -32000,
            10,
            5,
            None,
            None,
            Some(inst.into()),
            None,
        )
    };
    let Ok(hwnd) = hwnd else { return };
    let now = Instant::now();
    let mut s = St { hwnd, on: false, shown: false, grow_from: 0.0, grow_to: 0.0, grow_t: now, hot: false, op_from: 0.5, op_to: 0.5, op_t: now, sweep_t: None, place: None, buf: Vec::new() };
    loop {
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        let animating = s.grow_t > Instant::now() || (s.on && !s.shown && s.place.is_some()) || (s.shown && (!s.grow().1 || !s.opacity().1 || s.sweep_t.is_some()));
        let msg = rx.recv_timeout(if animating { Duration::from_millis(16) } else { Duration::from_millis(1000) });
        match msg {
            Ok(Msg::On(on)) if on != s.on => {
                s.on = on;
                let (g, _) = s.grow();
                s.grow_from = g;
                s.grow_to = if on { 1.0 } else { 0.0 };
                s.grow_t = Instant::now() + if on { Duration::from_millis(120) } else { Duration::ZERO };
                if on {
                    s.grow_from = 0.0;
                }
            }
            Ok(Msg::Hot(h)) if h != s.hot => {
                s.hot = h;
                let (o, _) = s.opacity();
                s.op_from = o;
                s.op_to = if h { 1.0 } else { 0.5 };
                s.op_t = Instant::now();
                if h {
                    s.sweep_t = Some(Instant::now());
                    unsafe {
                        let _ = SetWindowPos(s.hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
                    }
                }
            }
            Ok(Msg::Place(p)) => {
                if p != s.place {
                    s.place = p;
                    if s.shown {
                        s.render();
                    }
                }
            }
            Ok(_) => {}
            Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {}
        }
        // drop the delayed grow start while it is still in the future
        if s.grow_t > Instant::now() {
            continue;
        }
        if let Some(t) = s.sweep_t {
            if t.elapsed().as_secs_f32() > 1.15 {
                s.sweep_t = None;
            }
        }
        let visible = s.place.is_some() && (s.on || !s.grow().1 || s.grow().0 > 0.001);
        unsafe {
            if visible {
                s.render();
                if !s.shown {
                    let _ = ShowWindow(s.hwnd, SW_SHOWNOACTIVATE);
                    let _ = SetWindowPos(s.hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
                    s.shown = true;
                }
            } else if s.shown {
                let _ = ShowWindow(s.hwnd, SW_HIDE);
                s.shown = false;
            }
        }
    }
    unsafe {
        let _ = DestroyWindow(hwnd);
    }
}
