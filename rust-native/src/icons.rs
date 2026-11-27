// The renderer's inline SVG icons (renderer/index.html, app.js ICONS), as vector primitives.
use crate::draw::Prim::{self, *};

pub const CPU: &[Prim] = &[R(6.5, 6.5, 11.0, 11.0, 2.0), R(10.0, 10.0, 4.0, 4.0, 0.6), P("M9.5 3v3M14.5 3v3M9.5 18v3M14.5 18v3M3 9.5h3M3 14.5h3M18 9.5h3M18 14.5h3")];
pub const GPU: &[Prim] = &[R(2.5, 6.0, 17.0, 11.0, 2.0), C(9.0, 11.5, 3.0), P("M14.5 9.5h2.5M14.5 13.5h2.5M19.5 9h2v5h-2M5.5 17v2.5M9 17v2.5")];
pub const RAM: &[Prim] = &[R(2.5, 7.0, 19.0, 9.0, 1.5), P("M6 9.5v4M10 9.5v4M14 9.5v4M18 9.5v4M5 16v2.5M9 16v2.5M15 16v2.5M19 16v2.5")];
pub const SPARK: &[Prim] = &[F("M12 2.5l2.1 6.6 6.9 2.4-6.9 2.4L12 20.5l-2.1-6.6L3 11.5l6.9-2.4z")];
pub const HEX: &[Prim] = &[P("M12 2.8l8 4.6v9.2l-8 4.6-8-4.6V7.4z"), P("M9 10l-2 2 2 2M15 10l2 2-2 2")];

pub const WIFI_TILE: &[Prim] = &[P("M2.5 9a14 14 0 0 1 19 0M5.5 12.5a9.5 9.5 0 0 1 13 0M8.6 16a5 5 0 0 1 6.8 0"), Dot(12.0, 19.2, 1.2)];
pub const BT: &[Prim] = &[P("M7 7l10 10-5 4.5V2.5L17 7 7 17")];
pub const SOUND_BODY: &[Prim] = &[P("M4 9.5h3.5L12 5.5v13l-4.5-4H4z")];
pub const SOUND_WAVE: &[Prim] = &[P("M15.5 9a4 4 0 0 1 0 6M18 6.5a7.5 7.5 0 0 1 0 11")];
pub const SOUND_SLASH: &[Prim] = &[P("M15.5 9.5l5 5M20.5 9.5l-5 5")];
pub const MIC_BODY: &[Prim] = &[R(9.0, 3.0, 6.0, 11.0, 3.0), P("M5.5 11a6.5 6.5 0 0 0 13 0M12 17.5V21")];
pub const MIC_SLASH: &[Prim] = &[P("M4 4l16 16")];
pub const MOON: &[Prim] = &[P("M20 14.5A8 8 0 0 1 9.5 4a8 8 0 1 0 10.5 10.5z")];
pub const ETHERNET: &[Prim] = &[R(4.0, 3.0, 16.0, 12.0, 2.0), P("M8 15v3h8v-3M10 18v3M14 18v3M8 7v3M11 7v3M14 7v3M17 7v3")];

pub const PREV: &[Prim] = &[P("M7 5v14M18 5.5L9.5 12 18 18.5z")];
pub const NEXT: &[Prim] = &[P("M17 5v14M6 5.5L14.5 12 6 18.5z")];
pub const PLAY: &[Prim] = &[F("M8 5.5v13L18.5 12z"), P("M8 5.5v13L18.5 12z")];
pub const PAUSE: &[Prim] = &[P("M8.5 5.5v13M15.5 5.5v13")];

pub const LOCK: &[Prim] = &[R(5.0, 10.5, 14.0, 10.0, 2.0), P("M8 10.5V8a4 4 0 0 1 8 0v2.5")];
pub const SLEEP: &[Prim] = &[P("M19 14A7.5 7.5 0 0 1 10 5a7.5 7.5 0 1 0 9 9z")];
pub const RESTART: &[Prim] = &[P("M19.5 12a7.5 7.5 0 1 1-2.2-5.3M19.5 4v4.5H15")];
pub const SHUTDOWN: &[Prim] = &[P("M12 3.5v8M7 6.5a7.5 7.5 0 1 0 10 0")];

pub const DISPLAY: &[Prim] = &[R(3.0, 4.0, 18.0, 12.0, 2.0), P("M8 20h8M12 16v4")];
pub const SOUND_SMALL: &[Prim] = &[P("M4 9.5h3.5L12 5.5v13l-4.5-4H4z"), P("M15.5 9a4 4 0 0 1 0 6")];
pub const NETWORK: &[Prim] = &[P("M2.5 9a14 14 0 0 1 19 0M5.5 12.5a9.5 9.5 0 0 1 13 0M8.6 16a5 5 0 0 1 6.8 0M12 19.2h.01")];
pub const NIGHT: &[Prim] = &[C(12.0, 12.0, 4.0), P("M12 2.5v2M12 19.5v2M2.5 12h2M19.5 12h2M5.3 5.3l1.4 1.4M17.3 17.3l1.4 1.4M5.3 18.7l1.4-1.4M17.3 6.7l1.4-1.4")];
pub const BELL: &[Prim] = &[P("M6 16V11a6 6 0 0 1 12 0v5l1.5 2h-15zM10 20.5a2 2 0 0 0 4 0")];

pub const SPEAKER: &[Prim] = &[P("M4 9.5h3.5L12 5.5v13l-4.5-4H4z"), P("M15.5 9a4 4 0 0 1 0 6M18 6.5a7.5 7.5 0 0 1 0 11")];
pub const MIC: &[Prim] = &[R(9.0, 3.0, 6.0, 11.0, 3.0), P("M5.5 11a6.5 6.5 0 0 0 13 0M12 17.5V21")];
pub const HEADPHONES: &[Prim] = &[P("M4 15v-3a8 8 0 0 1 16 0v3"), R(3.5, 14.0, 4.0, 6.5, 1.5), R(16.5, 14.0, 4.0, 6.5, 1.5)];
pub const PHONE: &[Prim] = &[R(7.0, 2.5, 10.0, 19.0, 2.5), P("M11 18.5h2")];
pub const KEYBOARD: &[Prim] = &[R(2.5, 6.5, 19.0, 11.0, 2.0), P("M6 10h.01M9 10h.01M12 10h.01M15 10h.01M18 10h.01M7.5 14h9")];
pub const WIFI: &[Prim] = NETWORK;

// Glyphs the HTML used as text characters, drawn as vectors so they look the same everywhere
pub const DIAMOND: &[Prim] = &[P("M12 3.5L20.5 12 12 20.5 3.5 12z"), F("M12 8.2L15.8 12 12 15.8 8.2 12z")]; // ◈
pub const CHEVRON_UP: &[Prim] = &[P("M6 14.5l6-6 6 6")]; // ⌃
pub const PIN: &[Prim] = &[C(12.0, 8.5, 4.5), P("M12 13v8")]; // ⚲
pub const RELOAD: &[Prim] = &[P("M19 12a7 7 0 1 1-2.05-4.95M19 4.5v3.5h-3.5")]; // ⟳
pub const SEND: &[Prim] = &[F("M4 4.5L21 12 4 19.5l2.8-7.5z")]; // ➤
pub const BACK: &[Prim] = &[P("M14.5 5.5L8 12l6.5 6.5")]; // ‹
pub const CHEV_R: &[Prim] = &[P("M9.5 5.5L16 12l-6.5 6.5")]; // ›
pub const CARET_DOWN: &[Prim] = &[F("M6.5 9.5h11L12 16z")]; // ▾
pub const CROSS: &[Prim] = &[P("M7 7l10 10M17 7L7 17")]; // ✕
pub const HEART: &[Prim] = &[P("M12 20s-7.5-4.6-7.5-10.2A4.3 4.3 0 0 1 12 7.2a4.3 4.3 0 0 1 7.5 2.6C19.5 15.4 12 20 12 20z")];
pub const STAR: &[Prim] = &[P("M12 3.5l2.6 5.4 5.9.8-4.3 4.1 1 5.8L12 16.8l-5.2 2.8 1-5.8-4.3-4.1 5.9-.8z")];
