// Colours, fonts and easing curves: the same values as renderer/styles.css.
use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, FontId};
use std::sync::Arc;

/// CSS rgba() -> egui's premultiplied colour.
pub const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color32 {
    Color32::from_rgba_premultiplied(
        (r as f32 * a + 0.5) as u8,
        (g as f32 * a + 0.5) as u8,
        (b as f32 * a + 0.5) as u8,
        (a * 255.0 + 0.5) as u8,
    )
}
pub const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

pub const BG_CARD: Color32 = rgba(16, 26, 46, 0.62);
pub const LINE: Color32 = rgba(110, 200, 255, 0.13);
pub const LINE2: Color32 = rgba(110, 200, 255, 0.24);
pub const TEXT: Color32 = rgb(0xdc, 0xef, 0xff);
pub const DIM: Color32 = rgb(0x7d, 0x92, 0xaf);
pub const DIM2: Color32 = rgb(0x50, 0x62, 0x7d);
pub const CYAN: Color32 = rgb(0x3e, 0xf3, 0xff);
pub const VIOLET: Color32 = rgb(0x9b, 0x7b, 0xff);
pub const PINK: Color32 = rgb(0xff, 0x4f, 0xd8);
pub const GREEN: Color32 = rgb(0x44, 0xff, 0xa8);
pub const AMBER: Color32 = rgb(0xff, 0xb5, 0x47);
pub const RED: Color32 = rgb(0xff, 0x4d, 0x6d);
pub const ORANGE: Color32 = rgb(0xff, 0x9a, 0x6b);
pub const WHITE05: Color32 = rgba(255, 255, 255, 0.05);
pub const INK: Color32 = rgb(0x04, 0x10, 0x18);
pub const SELECT_BG: Color32 = rgb(0x0d, 0x15, 0x27);

/// Multiply a colour's opacity.
pub fn fade(c: Color32, k: f32) -> Color32 {
    c.gamma_multiply(k.clamp(0.0, 1.0))
}

/// Linear blend of two colours (both premultiplied).
pub fn lerp_c(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()), l(a.a(), b.a()))
}

/// color-mix(in srgb, c k%, white)
pub fn mix_white(c: Color32, k: f32) -> Color32 {
    lerp_c(Color32::WHITE, c, k)
}

pub fn hsl(h: f32, s: f32, l: f32) -> Color32 {
    let (s, l) = (s / 100.0, l / 100.0);
    let a = s * l.min(1.0 - l);
    let f = |n: f32| {
        let k = (n + h / 30.0) % 12.0;
        let v = l - a * (k - 3.0).min(9.0 - k).min(1.0).max(-1.0);
        (v * 255.0).round().clamp(0.0, 255.0) as u8
    };
    Color32::from_rgb(f(0.0), f(8.0), f(4.0))
}

// ---------------- easing: CSS cubic-bezier() ----------------
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Ease {
    Linear,
    /// The number tween in app.js: 1 - (1 - k)^3
    OutCubic,
    Bez(f32, f32, f32, f32),
}

pub const EASE: Ease = Ease::Bez(0.25, 0.1, 0.25, 1.0); // CSS `ease`
pub const EASE_IN: Ease = Ease::Bez(0.42, 0.0, 1.0, 1.0);
pub const EASE_OUT: Ease = Ease::Bez(0.0, 0.0, 0.58, 1.0);
pub const EASE_IN_OUT: Ease = Ease::Bez(0.42, 0.0, 0.58, 1.0);
pub const SMOOTH: Ease = Ease::Bez(0.22, 0.9, 0.3, 1.0); // --ease
pub const SPRING: Ease = Ease::Bez(0.2, 1.18, 0.32, 1.0); // --spring

impl Ease {
    pub fn at(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Ease::Linear => t,
            Ease::OutCubic => 1.0 - (1.0 - t).powi(3),
            Ease::Bez(x1, y1, x2, y2) => {
                if t <= 0.0 || t >= 1.0 {
                    return t;
                }
                let bx = |u: f32| 3.0 * x1 * u * (1.0 - u) * (1.0 - u) + 3.0 * x2 * u * u * (1.0 - u) + u * u * u;
                let by = |u: f32| 3.0 * y1 * u * (1.0 - u) * (1.0 - u) + 3.0 * y2 * u * u * (1.0 - u) + u * u * u;
                let dx = |u: f32| 3.0 * x1 * (1.0 - u) * (1.0 - u) + 6.0 * (x2 - x1) * u * (1.0 - u) + 3.0 * (1.0 - x2) * u * u;
                // Newton, then bisection as a fallback
                let mut u = t;
                for _ in 0..8 {
                    let e = bx(u) - t;
                    if e.abs() < 1e-5 {
                        return by(u);
                    }
                    let d = dx(u);
                    if d.abs() < 1e-6 {
                        break;
                    }
                    u -= e / d;
                }
                let (mut lo, mut hi) = (0.0f32, 1.0f32);
                u = t;
                for _ in 0..30 {
                    let x = bx(u);
                    if (x - t).abs() < 1e-5 {
                        break;
                    }
                    if x < t {
                        lo = u
                    } else {
                        hi = u
                    }
                    u = (lo + hi) / 2.0;
                }
                by(u)
            }
        }
    }
}

// ---------------- fonts ----------------
// Same stacks as the CSS: Bahnschrift / Cascadia Mono on Windows, the system font / Menlo on macOS.
pub const DISPLAY: &str = "display";
pub const DISPLAY6: &str = "display-600";
pub const DISPLAY7: &str = "display-700";
pub const MONO: &str = "mono";

pub fn font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(DISPLAY.into()))
}
pub fn font6(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(DISPLAY6.into()))
}
pub fn font7(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(DISPLAY7.into()))
}
pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(MONO.into()))
}

fn read_first(paths: &[&str]) -> Option<&'static [u8]> {
    for p in paths {
        let p = expand(p);
        if let Ok(b) = std::fs::read(&p) {
            return Some(Box::leak(b.into_boxed_slice()));
        }
    }
    None
}

fn expand(p: &str) -> String {
    if let Some(rest) = p.strip_prefix("%WINDIR%") {
        return format!("{}{}", std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into()), rest);
    }
    if let Some(rest) = p.strip_prefix("%LOCALAPPDATA%") {
        return format!("{}{}", std::env::var("LOCALAPPDATA").unwrap_or_default(), rest);
    }
    p.to_string()
}

pub fn install_fonts(ctx: &egui::Context) {
    let mut defs = FontDefinitions::default();
    let default_fallback: Vec<String> = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();

    #[cfg(windows)]
    let (display, monos, symbols): (&[&str], &[&str], &[&str]) = (
        &["%WINDIR%\\Fonts\\bahnschrift.ttf", "%WINDIR%\\Fonts\\SegUIVar.ttf", "%WINDIR%\\Fonts\\segoeui.ttf"],
        &["%WINDIR%\\Fonts\\CascadiaMono.ttf", "%LOCALAPPDATA%\\Microsoft\\Windows\\Fonts\\CascadiaMono.ttf", "%WINDIR%\\Fonts\\CascadiaCode.ttf", "%WINDIR%\\Fonts\\consola.ttf"],
        &["%WINDIR%\\Fonts\\seguisym.ttf", "%WINDIR%\\Fonts\\segoeui.ttf"],
    );
    #[cfg(target_os = "macos")]
    let (display, monos, symbols): (&[&str], &[&str], &[&str]) = (
        &["/System/Library/Fonts/SFNS.ttf", "/System/Library/Fonts/Helvetica.ttc"],
        &["/System/Library/Fonts/SFNSMono.ttf", "/System/Library/Fonts/Menlo.ttc"],
        &["/System/Library/Fonts/Apple Symbols.ttf"],
    );
    #[cfg(not(any(windows, target_os = "macos")))]
    let (display, monos, symbols): (&[&str], &[&str], &[&str]) = (&[], &[], &[]);

    let mut fam = |name: &str, list: Vec<String>| {
        defs.families.insert(FontFamily::Name(name.into()), list);
    };
    let mut d_names: [Vec<String>; 3] = Default::default();
    let mut extra: Vec<(String, Arc<FontData>)> = Vec::new();
    if let Some(bytes) = read_first(display) {
        for (i, w) in [400.0f32, 600.0, 700.0].into_iter().enumerate() {
            let mut tweak = egui::FontTweak::default();
            tweak.coords = egui::epaint::text::VariationCoords::new([(b"wght", w)]);
            // Bahnschrift's line box sits its figures ~1.5 px high at 12 px: nudge them down so text
            // is optically centred next to icons and inside chips
            #[cfg(windows)]
            {
                tweak.y_offset_factor = 0.1;
            }
            let name = format!("display{w}");
            extra.push((name.clone(), Arc::new(FontData::from_static(bytes).tweak(tweak))));
            d_names[i].push(name);
        }
    }
    let mut m_names = Vec::new();
    if let Some(bytes) = read_first(monos) {
        extra.push(("mono".into(), Arc::new(FontData::from_static(bytes))));
        m_names.push("mono".to_string());
    }
    let mut s_names = Vec::new();
    // SF/Apple Symbols do not cover the Taka currency glyph used by Bengali locales.
    #[cfg(target_os = "macos")]
    if let Some(bytes) = read_first(&["/System/Library/Fonts/Supplemental/Bangla MN.ttc", "/System/Library/Fonts/KohinoorBangla.ttc"]) {
        extra.push(("bangla".into(), Arc::new(FontData::from_static(bytes))));
        s_names.push("bangla".to_string());
    }
    if let Some(bytes) = read_first(symbols) {
        extra.push(("symbols".into(), Arc::new(FontData::from_static(bytes))));
        s_names.push("symbols".to_string());
    }
    for (n, d) in extra {
        defs.font_data.insert(n, d);
    }
    let chain = |first: &Vec<String>| {
        let mut v = first.clone();
        v.extend(s_names.iter().cloned());
        v.extend(default_fallback.iter().cloned());
        v
    };
    let (d4, d6, d7) = (chain(&d_names[0]), chain(&d_names[1]), chain(&d_names[2]));
    let mut mono_chain = m_names.clone();
    mono_chain.extend(chain(&d_names[0]));
    fam(DISPLAY, d4.clone());
    fam(DISPLAY6, d6);
    fam(DISPLAY7, d7);
    fam(MONO, mono_chain);
    defs.families.insert(FontFamily::Proportional, d4);
    ctx.set_fonts(defs);
}
