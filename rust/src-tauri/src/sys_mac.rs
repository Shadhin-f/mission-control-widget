// System controls on macOS: the Swift helper for CoreAudio / CoreWLAN / Bluetooth / brightness,
// AppleScript for media and appearance, and pmset / networksetup for power and Wi-Fi.
#![cfg(target_os = "macos")]
use crate::native_mac::{apple, helper};
use crate::util::run;
use serde_json::{json, Value};
use std::sync::Mutex;

static WIFI: Mutex<(Option<std::time::Instant>, Value)> = Mutex::new((None, Value::Null));

fn blueutil() -> Option<&'static str> {
    ["/opt/homebrew/bin/blueutil", "/usr/local/bin/blueutil"].into_iter().find(|p| std::path::Path::new(p).exists())
}

fn quote(v: &str) -> String {
    serde_json::to_string(v).unwrap()
}

async fn media_state() -> Value {
    // Check running apps first, so polling never launches a player or prompts for unrelated apps.
    let running = run("/bin/ps", &["-axo", "comm="], 5000).await.out;
    for app in ["Spotify", "Music"] {
        let suffix = format!("/{app}.app/Contents/MacOS/{app}");
        if !running.lines().any(|p| p.ends_with(&suffix)) {
            continue;
        }
        let script = format!("tell application \"{app}\"\nreturn (player state as text) & linefeed & (name of current track) & linefeed & (artist of current track)\nend tell");
        if let Ok(out) = apple(&script).await {
            let mut it = out.split('\n');
            let status = it.next().unwrap_or("");
            return json!({ "app": app, "title": it.next().unwrap_or(""), "artist": it.next().unwrap_or(""), "status": if status == "playing" { "Playing" } else { "Paused" } });
        }
    }
    Value::Null
}

pub async fn sys_helper(cmd: String, a: Value) -> Result<Value, String> {
    match cmd.as_str() {
        "requestLocation" | "state" | "bt" | "brightness" | "setVolume" | "setMute" | "setDefault" | "setBrightness" => {
            let mut v = helper(&cmd, &a, false).await?;
            if cmd == "state" {
                v["media"] = media_state().await;
            }
            Ok(v)
        }
        "stepVolume" => {
            let s = helper("state", &json!({}), false).await?;
            let d = s["audio"]["out"].as_array().and_then(|l| l.iter().find(|d| d["isDefault"] == true).cloned()).ok_or("No output device")?;
            let value = (d["volume"].as_f64().unwrap_or(0.0) + a["delta"].as_f64().unwrap_or(0.0)).clamp(0.0, 100.0);
            helper("setVolume", &json!({ "id": d["id"], "value": value }), false).await?;
            Ok(json!(value))
        }
        "setDark" => {
            let dark = a["dark"].as_bool().unwrap_or(false);
            apple(&format!("tell application \"System Events\" to tell appearance preferences to set dark mode to {dark}")).await?;
            Ok(json!({ "ok": true }))
        }
        "setRadio" => {
            let on = a["on"].as_bool().unwrap_or(false);
            match a["kind"].as_str().unwrap_or("") {
                "WiFi" => helper("setWifi", &json!({ "on": on }), false).await,
                "Bluetooth" => match blueutil() {
                    Some(b) => {
                        run(b, &["--power", if on { "1" } else { "0" }], 10000).await;
                        Ok(json!({ "ok": true }))
                    }
                    None => {
                        let _ = run("/usr/bin/open", &["x-apple.systempreferences:com.apple.BluetoothSettings"], 5000).await;
                        Err("Use Bluetooth Settings to change power. Install blueutil for the in-widget toggle.".into())
                    }
                },
                _ => Err("Unknown radio".into()),
            }
        }
        "media" => {
            let state = media_state().await;
            if state.is_null() {
                return helper("media", &a, false).await;
            }
            let action = match a["action"].as_str().unwrap_or("") {
                "toggle" => "playpause",
                "next" => "next track",
                "prev" => "previous track",
                _ => return Err("Unknown media action".into()),
            };
            apple(&format!("tell application {} to {action}", quote(state["app"].as_str().unwrap_or("Music")))).await?;
            Ok(json!({ "ok": true }))
        }
        "lock" => {
            run("/System/Library/CoreServices/Menu Extras/User.menu/Contents/Resources/CGSession", &["-suspend"], 5000).await;
            Ok(json!({ "ok": true }))
        }
        "sleep" => {
            run("/usr/bin/pmset", &["sleepnow"], 5000).await;
            Ok(json!({ "ok": true }))
        }
        _ => Err(format!("Unsupported command: {cmd}")),
    }
}

pub async fn wifi() -> Result<Value, String> {
    let scan = WIFI.lock().unwrap().0.map(|t| t.elapsed().as_secs() > 30).unwrap_or(true);
    if scan {
        WIFI.lock().unwrap().0 = Some(std::time::Instant::now());
    }
    let mut w = match helper("wifi", &json!({}), scan).await {
        Ok(w) => w,
        Err(_) => helper("wifi", &json!({}), false).await?,
    };
    let mut g = WIFI.lock().unwrap();
    if scan {
        g.1 = w["networks"].clone();
    } else {
        w["networks"] = if g.1.is_null() { json!([]) } else { g.1.clone() };
    }
    Ok(w)
}

pub async fn wifi_connect(ssid: String) -> Result<Value, String> {
    if ssid.is_empty() || ssid.starts_with('-') {
        return Err("Invalid network name".into());
    }
    let w = helper("wifi", &json!({}), false).await?;
    let iface = w["interface"].as_str().unwrap_or("en0").to_string();
    let r = run("/usr/sbin/networksetup", &["-setairportnetwork", &iface, &ssid], 20000).await;
    // networksetup prints errors on stdout and still exits 0
    if r.ok && r.out.trim().is_empty() {
        Ok(json!({ "ok": true }))
    } else {
        Ok(json!({ "ok": false, "message": format!("{}. Connect using Wi-Fi Settings if a password is required.", format!("{}{}", r.out, r.err).trim()) }))
    }
}

pub async fn wifi_disconnect() -> Result<Value, String> {
    // CoreWLAN disconnects without changing radio power.
    helper("disconnectWifi", &json!({}), false).await
}

pub async fn power_plans() -> Value {
    let p = run("/usr/bin/pmset", &["-g", "custom"], 5000).await.out;
    let low = regex::Regex::new(r"lowpowermode\s+1").unwrap().is_match(&p);
    json!([{ "guid": "auto", "name": "Automatic", "active": !low }, { "guid": "low", "name": "Low Power", "active": low }])
}

pub async fn set_power_plan(guid: String) -> Result<Value, String> {
    if guid != "auto" && guid != "low" {
        return Err("Unknown energy mode".into());
    }
    apple(&format!("do shell script \"/usr/bin/pmset -a lowpowermode {}\" with administrator privileges", if guid == "low" { 1 } else { 0 })).await?;
    Ok(json!({ "ok": true }))
}

pub async fn session(kind: String) -> Result<Value, String> {
    let action = match kind.as_str() {
        "restart" => "restart",
        "shutdown" => "shut down",
        "signout" => "log out",
        _ => return Err("Unknown session action".into()),
    };
    apple(&format!("tell application \"System Events\" to {action}")).await?;
    Ok(json!({ "ok": true }))
}
