// System controls on Windows, called directly from Rust (the Electron edition runs a PowerShell helper):
// Core Audio (devices, volume, mute, default device), radios (Wi-Fi/Bluetooth), paired Bluetooth devices,
// the system media session, DDC/CI monitor brightness, dark mode, lock and sleep; plus netsh / powercfg.
#![cfg(windows)]
#![allow(non_snake_case)]
use crate::util::run;
use serde_json::{json, Value};
use std::ffi::c_void;
use windows::core::{interface, Interface, GUID, HRESULT, HSTRING, PCWSTR};
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{eCapture, eRender, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE, EDataFlow};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ};

// ---------------- Core Audio ----------------

// Undocumented but stable since Windows 7: the only way to change the default audio device.
#[interface("f8679f50-850a-41cf-9c72-430f290290c8")]
unsafe trait IPolicyConfig: windows::core::IUnknown {
    fn GetMixFormat(&self, a: PCWSTR, b: *mut c_void) -> HRESULT;
    fn GetDeviceFormat(&self, a: PCWSTR, b: i32, c: *mut c_void) -> HRESULT;
    fn ResetDeviceFormat(&self, a: PCWSTR) -> HRESULT;
    fn SetDeviceFormat(&self, a: PCWSTR, b: *mut c_void, c: *mut c_void) -> HRESULT;
    fn GetProcessingPeriod(&self, a: PCWSTR, b: i32, c: *mut c_void, d: *mut c_void) -> HRESULT;
    fn SetProcessingPeriod(&self, a: PCWSTR, b: *mut c_void) -> HRESULT;
    fn GetShareMode(&self, a: PCWSTR, b: *mut c_void) -> HRESULT;
    fn SetShareMode(&self, a: PCWSTR, b: *mut c_void) -> HRESULT;
    fn GetPropertyValue(&self, a: PCWSTR, b: *mut c_void, c: *mut c_void) -> HRESULT;
    fn SetPropertyValue(&self, a: PCWSTR, b: *mut c_void, c: *mut c_void) -> HRESULT;
    fn SetDefaultEndpoint(&self, id: PCWSTR, role: u32) -> HRESULT;
    fn SetEndpointVisibility(&self, a: PCWSTR, b: i32) -> HRESULT;
}
const CLSID_POLICY_CONFIG: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

fn com() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

fn e(err: windows::core::Error) -> String {
    err.message().to_string()
}

fn enumerator() -> Result<IMMDeviceEnumerator, String> {
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(e) }
}

fn device_id(d: &IMMDevice) -> String {
    unsafe {
        match d.GetId() {
            Ok(p) => {
                let s = p.to_string().unwrap_or_default();
                CoTaskMemFree(Some(p.0 as *const c_void));
                s
            }
            Err(_) => String::new(),
        }
    }
}

fn device_name(d: &IMMDevice) -> String {
    use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
    unsafe {
        let Ok(store) = d.OpenPropertyStore(STGM_READ) else { return "Audio device".into() };
        match store.GetValue(&PKEY_Device_FriendlyName) {
            Ok(pv) => {
                let s = pv.to_string();
                if s.is_empty() { "Audio device".into() } else { s }
            }
            Err(_) => "Audio device".into(),
        }
    }
}

fn volume_of(d: &IMMDevice) -> Option<IAudioEndpointVolume> {
    unsafe { d.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok() }
}

fn default_id(en: &IMMDeviceEnumerator, flow: EDataFlow) -> Option<String> {
    unsafe { en.GetDefaultAudioEndpoint(flow, windows::Win32::Media::Audio::eConsole).ok().map(|d| device_id(&d)) }
}

fn list_devices(flow: EDataFlow) -> Result<Vec<Value>, String> {
    let en = enumerator()?;
    let def = default_id(&en, flow);
    let mut out = Vec::new();
    unsafe {
        let col = en.EnumAudioEndpoints(flow, DEVICE_STATE_ACTIVE).map_err(e)?;
        let n = col.GetCount().map_err(e)?;
        for i in 0..n {
            let Ok(d) = col.Item(i) else { continue };
            let id = device_id(&d);
            let (mut volume, mut muted) = (0i64, false);
            if let Some(v) = volume_of(&d) {
                if let Ok(f) = v.GetMasterVolumeLevelScalar() {
                    volume = (f * 100.0).round() as i64;
                }
                if let Ok(m) = v.GetMute() {
                    muted = m.as_bool();
                }
            }
            out.push(json!({ "id": id, "name": device_name(&d), "isDefault": Some(&id) == def.as_ref(), "volume": volume, "muted": muted }));
        }
    }
    Ok(out)
}

fn get_device(id: &str) -> Result<IMMDevice, String> {
    let en = enumerator()?;
    unsafe {
        if id.is_empty() {
            en.GetDefaultAudioEndpoint(eRender, windows::Win32::Media::Audio::eConsole).map_err(e)
        } else {
            en.GetDevice(&HSTRING::from(id)).map_err(e)
        }
    }
}

fn set_volume(id: &str, pct: i64) -> Result<Value, String> {
    let pct = pct.clamp(0, 100);
    let v = volume_of(&get_device(id)?).ok_or("This device has no volume control")?;
    unsafe {
        v.SetMasterVolumeLevelScalar(pct as f32 / 100.0, std::ptr::null()).map_err(e)?;
        if pct > 0 {
            let _ = v.SetMute(false, std::ptr::null());
        }
    }
    Ok(json!(pct))
}

fn step_volume(delta: i64) -> Result<Value, String> {
    let v = volume_of(&get_device("")?).ok_or("No output device")?;
    let cur = unsafe { v.GetMasterVolumeLevelScalar().map_err(e)? };
    set_volume("", (cur * 100.0).round() as i64 + delta)
}

fn set_mute(id: &str, mute: bool) -> Result<Value, String> {
    let v = volume_of(&get_device(id)?).ok_or("This device has no volume control")?;
    unsafe { v.SetMute(mute, std::ptr::null()).map_err(e)? };
    Ok(json!(mute))
}

fn set_default(id: &str) -> Result<Value, String> {
    let pc: IPolicyConfig = unsafe { CoCreateInstance(&CLSID_POLICY_CONFIG, None, CLSCTX_ALL).map_err(e)? };
    let w = HSTRING::from(id);
    for role in 0..3 {
        unsafe {
            pc.SetDefaultEndpoint(PCWSTR(w.as_ptr()), role).ok().map_err(e)?;
        }
    }
    Ok(json!(true))
}

// ---------------- Radios, Bluetooth, media session (WinRT) ----------------

fn radios() -> Result<Vec<Value>, String> {
    use windows::Devices::Radios::Radio;
    let list = Radio::GetRadiosAsync().map_err(e)?.get().map_err(e)?;
    let mut out = Vec::new();
    for r in list {
        out.push(json!({ "kind": radio_kind(r.Kind().map_err(e)?.0), "state": radio_state(r.State().map_err(e)?.0), "name": r.Name().map(|n| n.to_string()).unwrap_or_default() }));
    }
    Ok(out)
}

fn radio_kind(k: i32) -> &'static str {
    match k {
        1 => "WiFi",
        2 => "MobileBroadband",
        3 => "Bluetooth",
        4 => "FM",
        _ => "Other",
    }
}
fn radio_state(s: i32) -> &'static str {
    match s {
        1 => "On",
        2 => "Off",
        3 => "Disabled",
        _ => "Unknown",
    }
}

fn set_radio(kind: &str, on: bool) -> Result<Value, String> {
    use windows::Devices::Radios::{Radio, RadioAccessStatus, RadioState};
    let access = Radio::RequestAccessAsync().map_err(e)?.get().map_err(e)?;
    if access != RadioAccessStatus::Allowed {
        return Err(format!("Windows denied radio access ({})", access.0));
    }
    let list = Radio::GetRadiosAsync().map_err(e)?.get().map_err(e)?;
    let r = list.into_iter().find(|r| r.Kind().map(|k| radio_kind(k.0) == kind).unwrap_or(false)).ok_or(format!("No {kind} adapter found"))?;
    let res = r.SetStateAsync(if on { RadioState::On } else { RadioState::Off }).map_err(e)?.get().map_err(e)?;
    if res != RadioAccessStatus::Allowed {
        return Err(format!("Could not switch {kind} ({})", res.0));
    }
    Ok(json!(on))
}

fn major_class(c: i32) -> &'static str {
    match c {
        0 => "Miscellaneous",
        1 => "Computer",
        2 => "Phone",
        3 => "NetworkAccessPoint",
        4 => "AudioVideo",
        5 => "Peripheral",
        6 => "Imaging",
        7 => "Wearable",
        8 => "Toy",
        9 => "Health",
        _ => "",
    }
}

fn bt_devices() -> Result<Vec<Value>, String> {
    use windows::Devices::Bluetooth::{BluetoothConnectionStatus, BluetoothDevice, BluetoothLEDevice};
    use windows::Devices::Enumeration::DeviceInformation;
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for le in [false, true] {
        let sel = if le { BluetoothLEDevice::GetDeviceSelectorFromPairingState(true) } else { BluetoothDevice::GetDeviceSelectorFromPairingState(true) }.map_err(e)?;
        let infos = DeviceInformation::FindAllAsyncAqsFilter(&sel).map_err(e)?.get().map_err(e)?;
        for i in infos {
            let name = i.Name().map(|n| n.to_string()).unwrap_or_default();
            if name.is_empty() || seen.contains(&name) {
                continue;
            }
            let (mut connected, mut kind) = (false, String::new());
            if let Ok(id) = i.Id() {
                if le {
                    kind = "LE".into();
                    if let Ok(d) = BluetoothLEDevice::FromIdAsync(&id).and_then(|op| op.get()) {
                        connected = d.ConnectionStatus().map(|s| s == BluetoothConnectionStatus::Connected).unwrap_or(false);
                        let _ = d.Close();
                    }
                } else if let Ok(d) = BluetoothDevice::FromIdAsync(&id).and_then(|op| op.get()) {
                    connected = d.ConnectionStatus().map(|s| s == BluetoothConnectionStatus::Connected).unwrap_or(false);
                    kind = d.ClassOfDevice().and_then(|c| c.MajorClass()).map(|m| major_class(m.0).to_string()).unwrap_or_default();
                    let _ = d.Close();
                }
            }
            seen.insert(name.clone());
            out.push(json!({ "name": name, "connected": connected, "kind": kind }));
        }
    }
    Ok(out)
}

fn media_session() -> Option<windows::Media::Control::GlobalSystemMediaTransportControlsSession> {
    use windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager as M;
    M::RequestAsync().ok()?.get().ok()?.GetCurrentSession().ok()
}

fn playback_status(s: i32) -> &'static str {
    match s {
        0 => "Closed",
        1 => "Opened",
        2 => "Changing",
        3 => "Stopped",
        4 => "Playing",
        5 => "Paused",
        _ => "Unknown",
    }
}

fn get_media() -> Value {
    let Some(s) = media_session() else { return Value::Null };
    let props = s.TryGetMediaPropertiesAsync().ok().and_then(|op| op.get().ok());
    let status = s.GetPlaybackInfo().and_then(|p| p.PlaybackStatus()).map(|st| playback_status(st.0)).unwrap_or("Unknown");
    json!({
        "title": props.as_ref().and_then(|p| p.Title().ok()).map(|t| t.to_string()).unwrap_or_default(),
        "artist": props.as_ref().and_then(|p| p.Artist().ok()).map(|t| t.to_string()).unwrap_or_default(),
        "app": s.SourceAppUserModelId().map(|t| t.to_string()).unwrap_or_default(),
        "status": status,
    })
}

fn invoke_media(action: &str) -> Result<Value, String> {
    let Some(s) = media_session() else { return Ok(json!(false)) };
    let op = match action {
        "toggle" => s.TryTogglePlayPauseAsync(),
        "next" => s.TrySkipNextAsync(),
        "prev" => s.TrySkipPreviousAsync(),
        _ => return Err("Unknown media action".into()),
    };
    Ok(json!(op.map_err(e)?.get().map_err(e)?))
}

// ---------------- Dark mode ----------------

const THEME_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize";

fn reg_get_dword(key: &str, name: &str) -> Option<u32> {
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    let (k, n) = (HSTRING::from(key), HSTRING::from(name));
    let mut v: u32 = 0;
    let mut size = 4u32;
    let r = unsafe { RegGetValueW(HKEY_CURRENT_USER, PCWSTR(k.as_ptr()), PCWSTR(n.as_ptr()), RRF_RT_REG_DWORD, None, Some(&mut v as *mut u32 as *mut c_void), Some(&mut size)) };
    if r.is_ok() { Some(v) } else { None }
}

fn reg_set_dword(key: &str, name: &str, v: u32) -> Result<(), String> {
    use windows::Win32::System::Registry::{RegSetKeyValueW, HKEY_CURRENT_USER, REG_DWORD};
    let (k, n) = (HSTRING::from(key), HSTRING::from(name));
    let r = unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, PCWSTR(k.as_ptr()), PCWSTR(n.as_ptr()), REG_DWORD.0, Some(&v as *const u32 as *const c_void), 4) };
    if r.is_ok() { Ok(()) } else { Err(format!("Could not change the theme ({})", r.0)) }
}

fn get_dark() -> bool {
    reg_get_dword(THEME_KEY, "AppsUseLightTheme") == Some(0)
}

fn set_dark(dark: bool) -> Result<Value, String> {
    let v = if dark { 0 } else { 1 };
    reg_set_dword(THEME_KEY, "AppsUseLightTheme", v)?;
    reg_set_dword(THEME_KEY, "SystemUsesLightTheme", v)?;
    crate::native_win::broadcast_colors(300);
    Ok(json!(dark))
}

// ---------------- Brightness ----------------

fn physical_monitors() -> Vec<windows::Win32::Devices::Display::PHYSICAL_MONITOR> {
    use windows::Win32::Devices::Display::{GetNumberOfPhysicalMonitorsFromHMONITOR, GetPhysicalMonitorsFromHMONITOR, PHYSICAL_MONITOR};
    use windows::Win32::Foundation::{LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR};
    unsafe extern "system" fn cb(h: HMONITOR, _: HDC, _: *mut RECT, lp: LPARAM) -> windows::core::BOOL {
        (*(lp.0 as *mut Vec<HMONITOR>)).push(h);
        true.into()
    }
    let mut hmons: Vec<HMONITOR> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(cb), LPARAM(&mut hmons as *mut _ as isize));
    }
    let mut out = Vec::new();
    for hm in hmons {
        let mut n = 0u32;
        if unsafe { GetNumberOfPhysicalMonitorsFromHMONITOR(hm, &mut n) }.is_err() || n == 0 {
            continue;
        }
        let mut arr = vec![PHYSICAL_MONITOR::default(); n as usize];
        if unsafe { GetPhysicalMonitorsFromHMONITOR(hm, &mut arr) }.is_ok() {
            out.extend(arr);
        }
    }
    out
}

fn release(mons: Vec<windows::Win32::Devices::Display::PHYSICAL_MONITOR>) {
    for m in mons {
        unsafe {
            let _ = windows::Win32::Devices::Display::DestroyPhysicalMonitor(m.hPhysicalMonitor);
        }
    }
}

fn ddc_list() -> Vec<Value> {
    use windows::Win32::Devices::Display::GetMonitorBrightness;
    let mons = physical_monitors();
    let mut res = Vec::new();
    for (i, m) in mons.iter().enumerate() {
        let (mut mn, mut cur, mut mx) = (0u32, 0u32, 0u32);
        if unsafe { GetMonitorBrightness(m.hPhysicalMonitor, &mut mn, &mut cur, &mut mx) } != 0 {
            // PHYSICAL_MONITOR is packed: copy the name out before reading it
            let desc = m.szPhysicalMonitorDescription;
            let end = desc.iter().position(|&c| c == 0).unwrap_or(desc.len());
            let name = String::from_utf16_lossy(&desc[..end]);
            res.push(json!({ "index": i, "name": name, "min": mn, "cur": cur, "max": mx }));
        }
    }
    release(mons);
    res
}

fn ps_text(script: &str) -> String {
    let o = crate::util::std_cmd("powershell.exe").args(["-NoProfile", "-NonInteractive", "-Command", script]).output();
    o.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
}

fn get_brightness() -> Vec<Value> {
    let list = ddc_list();
    if !list.is_empty() {
        return list;
    }
    // Laptop panels expose brightness through WMI instead of DDC/CI
    let cur = ps_text("(Get-CimInstance -Namespace root/wmi -ClassName WmiMonitorBrightness -ErrorAction Stop | Select-Object -First 1).CurrentBrightness");
    match cur.parse::<i64>() {
        Ok(c) => vec![json!({ "index": -1, "name": "Built-in display", "min": 0, "cur": c, "max": 100 })],
        Err(_) => vec![],
    }
}

fn set_brightness(index: i64, value: i64) -> Result<Value, String> {
    if index < 0 {
        let v = value.clamp(0, 100);
        ps_text(&format!("$m = Get-CimInstance -Namespace root/wmi -ClassName WmiMonitorBrightnessMethods | Select-Object -First 1; Invoke-CimMethod -InputObject $m -MethodName WmiSetBrightness -Arguments @{{ Timeout = 1; Brightness = [byte]{v} }} | Out-Null"));
        return Ok(json!(true));
    }
    let mons = physical_monitors();
    let ok = mons.get(index as usize).map(|m| unsafe { windows::Win32::Devices::Display::SetMonitorBrightness(m.hPhysicalMonitor, value.max(0) as u32) } != 0).unwrap_or(false);
    release(mons);
    Ok(json!(ok))
}

// ---------------- Dispatcher ----------------

fn arg_s(a: &Value, k: &str) -> String {
    match &a[k] {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        v => v.to_string(),
    }
}
fn arg_i(a: &Value, k: &str) -> i64 {
    a[k].as_f64().map(|f| f.round() as i64).or_else(|| a[k].as_str().and_then(|s| s.parse().ok())).unwrap_or(0)
}
fn arg_b(a: &Value, k: &str) -> bool {
    crate::settings::flag(a.as_object().unwrap_or(&Default::default()), k)
}

fn helper_blocking(cmd: &str, a: &Value) -> Result<Value, String> {
    com();
    match cmd {
        "state" => Ok(json!({
            "audio": { "out": list_devices(eRender)?, "in": list_devices(eCapture)? },
            "radios": radios().unwrap_or_default(),
            "media": get_media(),
            "dark": get_dark(),
        })),
        "bt" => Ok(json!({ "devices": bt_devices()? })),
        "brightness" => Ok(json!({ "monitors": get_brightness() })),
        "setVolume" => set_volume(&arg_s(a, "id"), arg_i(a, "value")),
        "stepVolume" => step_volume(arg_i(a, "delta")),
        "setMute" => set_mute(&arg_s(a, "id"), arg_b(a, "muted")),
        "setDefault" => set_default(&arg_s(a, "id")),
        "setRadio" => set_radio(&arg_s(a, "kind"), arg_b(a, "on")),
        "media" => invoke_media(&arg_s(a, "action")),
        "setBrightness" => set_brightness(arg_i(a, "index"), arg_i(a, "value")),
        "setDark" => set_dark(arg_b(a, "dark")),
        "lock" => Ok(json!(unsafe { windows::Win32::System::Shutdown::LockWorkStation() }.is_ok())),
        "sleep" => Ok(json!(unsafe { windows::Win32::System::Power::SetSuspendState(false, false, false) })),
        _ => Err(format!("Unknown command {cmd}")),
    }
}

pub async fn helper(cmd: String, args: Value) -> Result<Value, String> {
    let work = tokio::task::spawn_blocking(move || helper_blocking(&cmd, &args));
    match tokio::time::timeout(std::time::Duration::from_secs(20), work).await {
        Ok(Ok(r)) => r,
        Ok(Err(_)) => Err("System helper stopped".into()),
        Err(_) => Err("Timed out".into()),
    }
}

// ---------------- Wi-Fi (netsh) ----------------

fn field(txt: &str, name: &str) -> Option<String> {
    regex::Regex::new(&format!(r"(?mi)^\s*{}\s*:\s*(.+)$", regex::escape(name))).unwrap().captures(txt).map(|c| c[1].trim().to_string())
}

pub async fn wifi() -> Value {
    let iface = run("netsh", &["wlan", "show", "interfaces"], 15000).await;
    let both = format!("{}{}", iface.out, iface.err);
    if regex::Regex::new(r"(?i)no wireless interface|wlansvc.*not running").unwrap().is_match(&both) {
        return json!({ "available": false });
    }
    let current = json!({
        "state": field(&iface.out, "State"),
        "ssid": field(&iface.out, "SSID"),
        "signal": field(&iface.out, "Signal").and_then(|s| s.trim_end_matches('%').trim().parse::<i64>().ok()).unwrap_or(0),
    });
    let profiles_out = run("netsh", &["wlan", "show", "profiles"], 15000).await;
    let profiles: Vec<String> = regex::Regex::new(r"(?im)All User Profile\s*:\s*(.+)$").unwrap().captures_iter(&profiles_out.out).map(|c| c[1].trim().to_string()).collect();
    let nets = run("netsh", &["wlan", "show", "networks", "mode=bssid"], 15000).await;
    let mut networks: Vec<Value> = Vec::new();
    let mut needs_location = false;
    let has_ssid = regex::Regex::new(r"SSID \d+").unwrap().is_match(&nets.out);
    if regex::Regex::new(r"(?i)location").unwrap().is_match(&format!("{}{}", nets.out, nets.err)) && !has_ssid {
        needs_location = true;
    } else {
        let ssid_re = regex::Regex::new(r"(?m)^SSID \d+ :\s*(.*)$").unwrap();
        let sig_re = regex::Regex::new(r"Signal\s*:\s*(\d+)%").unwrap();
        let auth_re = regex::Regex::new(r"(?m)Authentication\s*:\s*(.+)$").unwrap();
        let starts: Vec<usize> = ssid_re.find_iter(&nets.out).map(|m| m.start()).collect();
        for (i, &s) in starts.iter().enumerate() {
            let block = &nets.out[s..*starts.get(i + 1).unwrap_or(&nets.out.len())];
            let Some(ssid) = ssid_re.captures(block).map(|c| c[1].trim().to_string()) else { continue };
            let signal = sig_re.captures_iter(block).filter_map(|c| c[1].parse::<i64>().ok()).max().unwrap_or(0);
            let auth = auth_re.captures(block).map(|c| c[1].trim().to_string()).unwrap_or_default();
            if !ssid.is_empty() {
                networks.push(json!({ "ssid": ssid, "signal": signal, "secure": !auth.to_lowercase().contains("open"), "known": profiles.contains(&ssid) }));
            }
        }
        networks.sort_by(|a, b| b["signal"].as_i64().cmp(&a["signal"].as_i64()));
    }
    json!({ "available": true, "current": current, "networks": networks, "needsLocation": needs_location })
}

pub async fn wifi_connect(ssid: String) -> Value {
    let r = run("netsh", &["wlan", "connect", &format!("name={ssid}")], 15000).await;
    let msg = if r.out.trim().is_empty() { r.err } else { r.out };
    json!({ "ok": r.ok && msg.to_lowercase().contains("success"), "message": msg.trim() })
}

pub async fn wifi_disconnect() -> Value {
    json!({ "ok": run("netsh", &["wlan", "disconnect"], 15000).await.ok })
}

// ---------------- Power plans (powercfg) ----------------

pub async fn power_plans() -> Value {
    let r = run("powercfg", &["/list"], 15000).await;
    let re = regex::Regex::new(r"(?i)GUID:\s*([0-9a-f-]{36})\s*\((.+?)\)\s*(\*)?").unwrap();
    Value::Array(re.captures_iter(&r.out).map(|c| json!({ "guid": &c[1], "name": &c[2], "active": c.get(3).is_some() })).collect())
}

pub async fn set_power_plan(guid: String) -> Result<Value, String> {
    if !regex::Regex::new(r"(?i)^[0-9a-f-]{36}$").unwrap().is_match(&guid) {
        return Err("Bad plan id".into());
    }
    Ok(json!({ "ok": run("powercfg", &["/setactive", &guid], 15000).await.ok }))
}

// ---------------- Session ----------------

pub async fn session(kind: String) -> Result<Value, String> {
    let args: &[&str] = match kind.as_str() {
        "restart" => &["/r", "/t", "0"],
        "shutdown" => &["/s", "/t", "0"],
        "signout" => &["/l"],
        _ => return Err("Unknown action".into()),
    };
    Ok(json!({ "ok": run("shutdown", args, 15000).await.ok }))
}

#[allow(dead_code)]
fn _assert_interface() {
    let _ = IPolicyConfig::IID;
}
