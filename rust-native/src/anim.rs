// CSS-transition semantics for an immediate-mode UI: ask for a value with its target each frame and get
// the animated value back. A changed target retargets from the current value, like a CSS transition.
// Nothing is stored per frame except the running tweens, and when none run the UI stops repainting.
use crate::theme::Ease;
use eframe::egui::Color32;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

#[derive(Clone, Copy)]
struct Tween {
    from: f32,
    to: f32,
    start: f64,
    dur: f32,
    ease: Ease,
    seen: u64,
}

impl Tween {
    fn at(&self, now: f64) -> f32 {
        if self.dur <= 0.0 || now >= self.start + self.dur as f64 {
            return self.to;
        }
        if now <= self.start {
            return self.from;
        }
        let k = ((now - self.start) / self.dur as f64) as f32;
        self.from + (self.to - self.from) * self.ease.at(k)
    }
    fn done(&self, now: f64) -> bool {
        now >= self.start + self.dur as f64
    }
}

#[derive(Clone, Copy)]
struct CTween {
    from: Color32,
    to: Color32,
    start: f64,
    dur: f32,
    seen: u64,
}

pub struct Anims {
    now: f64,
    frame: u64,
    map: HashMap<u64, Tween>,
    colors: HashMap<u64, CTween>,
    /// Some tween is still moving (the UI keeps repainting while true)
    pub active: bool,
    /// Only background tweens are moving (small meters): repainted at 30 fps instead of 60
    pub active_lo: bool,
    lo: bool,
}

pub fn key(k: impl Hash) -> u64 {
    let mut h = DefaultHasher::new();
    k.hash(&mut h);
    h.finish()
}

impl Anims {
    pub fn new() -> Self {
        Self { now: 0.0, frame: 0, map: HashMap::new(), colors: HashMap::new(), active: false, active_lo: false, lo: false }
    }

    pub fn begin(&mut self, now: f64) {
        self.now = now;
        self.frame += 1;
        self.active = false;
        self.active_lo = false;
        // Forget values nobody asked for in a while (rows that left a list, etc.)
        if self.frame % 600 == 0 {
            let f = self.frame;
            self.map.retain(|_, t| f - t.seen < 600);
            self.colors.retain(|_, t| f - t.seen < 600);
        }
    }

    /// Animated value: jumps to `target` the first time a key is seen.
    pub fn v(&mut self, k: impl Hash, target: f32, dur: f32, ease: Ease) -> f32 {
        self.full(key(k), None, target, dur, 0.0, ease)
    }

    /// Like `v`, starting after `delay` seconds whenever the target changes.
    pub fn vd(&mut self, k: impl Hash, target: f32, dur: f32, delay: f32, ease: Ease) -> f32 {
        self.full(key(k), None, target, dur, delay, ease)
    }

    /// Like `v`, but the first time a key is seen it animates in from `initial` (CSS @keyframes "from").
    pub fn v_in(&mut self, k: impl Hash, initial: f32, target: f32, dur: f32, delay: f32, ease: Ease) -> f32 {
        self.full(key(k), Some(initial), target, dur, delay, ease)
    }

    fn full(&mut self, k: u64, initial: Option<f32>, target: f32, dur: f32, delay: f32, ease: Ease) -> f32 {
        let now = self.now;
        let frame = self.frame;
        let t = self.map.entry(k).or_insert_with(|| match initial {
            Some(i) => Tween { from: i, to: target, start: now + delay as f64, dur, ease, seen: frame },
            None => Tween { from: target, to: target, start: now, dur: 0.0, ease, seen: frame },
        });
        t.seen = frame;
        if (t.to - target).abs() > 1e-6 {
            let cur = t.at(now);
            *t = Tween { from: cur, to: target, start: now + delay as f64, dur, ease, seen: frame };
        }
        if !t.done(now) {
            if self.lo {
                self.active_lo = true;
            } else {
                self.active = true;
            }
            if std::env::var_os("MC_DIAG_ANIM").is_some() {
                eprintln!("[tween] k={} from={} to={} dur={} start={:.2} now={:.2}", k, t.from, t.to, t.dur, t.start, now);
            }
        }
        t.at(now)
    }

    /// Run `f` with its tweens marked as background (30 fps)
    pub fn low<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        self.lo = true;
        let r = f(self);
        self.lo = false;
        r
    }

    /// Set a value immediately (no transition), e.g. while dragging a slider.
    pub fn snap(&mut self, k: impl Hash, value: f32) {
        let frame = self.frame;
        self.map.insert(key(k), Tween { from: value, to: value, start: self.now, dur: 0.0, ease: Ease::Linear, seen: frame });
    }

    pub fn color(&mut self, k: impl Hash, target: Color32, dur: f32) -> Color32 {
        let now = self.now;
        let frame = self.frame;
        let t = self.colors.entry(key(k)).or_insert(CTween { from: target, to: target, start: now, dur: 0.0, seen: frame });
        t.seen = frame;
        let at = |t: &CTween| {
            if t.dur <= 0.0 || now >= t.start + t.dur as f64 {
                t.to
            } else {
                crate::theme::lerp_c(t.from, t.to, crate::theme::EASE.at(((now - t.start) / t.dur as f64) as f32))
            }
        };
        if t.to != target {
            let cur = at(t);
            *t = CTween { from: cur, to: target, start: now, dur, seen: frame };
        }
        if now < t.start + t.dur as f64 {
            if self.lo {
                self.active_lo = true;
            } else {
                self.active = true;
            }
            if std::env::var_os("MC_DIAG_ANIM").is_some() {
                eprintln!("[ctween] from={:?} to={:?} dur={} start={:.2} now={:.2}", t.from, t.to, t.dur, t.start, now);
            }
        }
        at(t)
    }
}

/// Number tween used for gauge readouts (650 ms, cubic out), like tweenNum() in app.js.
pub fn num(a: &mut Anims, k: impl Hash, target: f32) -> f32 {
    a.v(k, target, 0.65, Ease::OutCubic)
}
