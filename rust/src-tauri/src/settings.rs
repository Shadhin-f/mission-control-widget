// Settings live in the same file as the Electron edition (<config dir>/Mission Control/settings.json),
// so both editions share one set of preferences.
use serde_json::{json, Map, Value};
use std::path::PathBuf;

pub type Settings = Map<String, Value>;

pub fn defaults() -> Settings {
    let v = json!({
        "openAtLogin": true,
        "idleOnFullscreen": true,
        "chatMode": "auto",
        "collapseOnBlur": true,
        "hideSidesUntilHover": false,
        "autoLimits": true,
        "claudeLimits": null,
        "codexLimits": null,
        "chatProvider": "auto",
        "codexFromEditor": false,
        "extremeMode": false,
        "extremeBackup": null,
        "speedHistory": [],
    });
    match v {
        Value::Object(m) => m,
        _ => unreachable!(),
    }
}

fn file() -> PathBuf {
    if let Ok(dir) = std::env::var("MC_SETTINGS_DIR") {
        return PathBuf::from(dir).join("settings.json");
    }
    dirs::config_dir().unwrap_or_else(crate::util::home).join("Mission Control").join("settings.json")
}

pub fn load() -> Settings {
    let mut s = defaults();
    if let Ok(text) = std::fs::read_to_string(file()) {
        // Strip a byte-order mark (some editors add one) so a hand-edited file still loads
        if let Ok(Value::Object(m)) = serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')) {
            for (k, v) in m {
                s.insert(k, v);
            }
        }
    }
    s
}

pub fn save(s: &Settings) {
    let f = file();
    if let Some(dir) = f.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string_pretty(s) {
        let _ = std::fs::write(f, text);
    }
}

pub fn flag(s: &Settings, k: &str) -> bool {
    match s.get(k) {
        Some(Value::Bool(b)) => *b,
        Some(Value::Null) | None => false,
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0) != 0.0,
        Some(Value::String(t)) => !t.is_empty(),
        Some(_) => true,
    }
}
