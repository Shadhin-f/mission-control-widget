// Extreme mode (Windows only): restyles the native taskbar to match the widget using Windows' own
// accent-colour settings. No code injection and no extra process. The user's original values are backed
// up exactly (including values that didn't exist) and restored when the mode is turned off or the widget quits.
#![cfg(windows)]
use crate::util::{run, std_cmd};
use serde_json::{json, Map, Value};

const ACCENT: &str = "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Accent";
const PERSONALIZE: &str = "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize";
const DESKTOP: &str = "HKCU\\Control Panel\\Desktop";

// The widget's palette, lightest to darkest (RGBA). Windows draws the taskbar from the dark end
// (navy, blended with the wallpaper) and the running-app indicators from the light end (cyan).
pub const THEME_PALETTE: &str = "B8FBFF007FF6FF003EF3FF001FA9C400145E7A000A1A3000060E1C003EF3FF00";

struct V {
    key: &'static str,
    name: &'static str,
    ty: &'static str,
    theme: &'static str,
}
const VALUES: [V; 6] = [
    V { key: ACCENT, name: "AccentPalette", ty: "REG_BINARY", theme: THEME_PALETTE },
    V { key: ACCENT, name: "AccentColorMenu", ty: "REG_DWORD", theme: "0xffc4a91f" }, // accent #1FA9C4 (ABGR)
    V { key: ACCENT, name: "StartColorMenu", ty: "REG_DWORD", theme: "0xff301a0a" },  // Start menu navy #0A1A30
    V { key: PERSONALIZE, name: "ColorPrevalence", ty: "REG_DWORD", theme: "0x1" },   // "Show accent colour on Start and taskbar"
    V { key: PERSONALIZE, name: "SystemUsesLightTheme", ty: "REG_DWORD", theme: "0x0" }, // that option needs the dark system theme
    V { key: DESKTOP, name: "AutoColorization", ty: "REG_DWORD", theme: "0x0" },      // stop the wallpaper overriding the accent
];

async fn read_value(v: &V) -> Value {
    let r = run("reg.exe", &["query", v.key, "/v", v.name], 10000).await;
    if !r.ok {
        return Value::Null; // value doesn't exist
    }
    let re = regex::Regex::new(&format!(r"(?m)^\s+{}\s+(REG_\w+)\s+(.*)$", regex::escape(v.name))).unwrap();
    match re.captures(&r.out) {
        Some(c) => json!({ "type": &c[1], "data": c[2].trim() }),
        None => Value::Null,
    }
}

fn add_args(v: &V, ty: &str, data: &str) -> Vec<String> {
    ["add", v.key, "/v", v.name, "/t", ty, "/d", data, "/f"].iter().map(|s| s.to_string()).collect()
}
fn delete_args(v: &V) -> Vec<String> {
    ["delete", v.key, "/v", v.name, "/f"].iter().map(|s| s.to_string()).collect()
}

async fn reg(args: Vec<String>) {
    let a: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    run("reg.exe", &a, 10000).await;
}

/// Current values, to back up before theming
pub async fn backup() -> Value {
    let mut out = Map::new();
    for v in &VALUES {
        out.insert(v.name.into(), read_value(v).await);
    }
    Value::Object(out)
}

/// True when the taskbar already carries our theme (e.g. after a crash), so the old backup must be kept
pub async fn is_themed() -> bool {
    read_value(&VALUES[0]).await["data"].as_str().map(|d| d.to_uppercase() == THEME_PALETTE).unwrap_or(false)
}

pub async fn apply() {
    for v in &VALUES {
        reg(add_args(v, v.ty, v.theme)).await;
    }
    tokio::task::spawn_blocking(|| crate::native_win::broadcast_colors(1000)).await.ok();
}

fn restore_args(saved: &Value, v: &V) -> Vec<String> {
    let o = &saved[v.name];
    match (o["type"].as_str(), o["data"].as_str()) {
        (Some(t), Some(d)) => add_args(v, t, d),
        _ => delete_args(v),
    }
}

pub async fn restore(saved: &Value) {
    if !saved.is_object() {
        return;
    }
    for v in &VALUES {
        reg(restore_args(saved, v)).await;
    }
    tokio::task::spawn_blocking(|| crate::native_win::broadcast_colors(1000)).await.ok();
}

/// Synchronous restore for app quit, where async work can't finish
pub fn restore_sync(saved: &Value) {
    if !saved.is_object() {
        return;
    }
    for v in &VALUES {
        let _ = std_cmd("reg.exe").args(restore_args(saved, v)).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
    }
    crate::native_win::broadcast_colors(300);
}
