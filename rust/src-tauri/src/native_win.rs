// Win32 layer (replaces the koffi bridge): fullscreen detection, app windows, NIC counters,
// native ICMP ping, the Recycle Bin, user idle time, taskbar position and drive space.
#![cfg(windows)]
use std::collections::HashMap;
use windows::core::{BOOL, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, IcmpCreateFile, IcmpSendEcho, ICMP_ECHO_REPLY, MIB_IF_TABLE2};
use windows::Win32::UI::Shell::{SHAppBarMessage, SHEmptyRecycleBinW, SHQueryRecycleBinW, SHQueryUserNotificationState, ABM_GETSTATE, ABM_GETTASKBARPOS, APPBARDATA, QUNS_BUSY, QUNS_RUNNING_D3D_FULL_SCREEN, QUNS_PRESENTATION_MODE, SHQUERYRBINFO};
use windows::Win32::UI::WindowsAndMessaging::*;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn class_name(h: HWND) -> String {
    let mut buf = [0u16; 256];
    let n = unsafe { GetClassNameW(h, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

const SHELL_CLASSES: [&str; 5] = ["Progman", "WorkerW", "Shell_TrayWnd", "Shell_SecondaryTrayWnd", "Windows.UI.Core.CoreWindow"];

/// True when the foreground window (not ours, not the desktop) covers its whole monitor.
pub fn is_foreground_fullscreen(own: &[isize]) -> bool {
    unsafe {
        if let Ok(st) = SHQueryUserNotificationState() {
            // D3D fullscreen / presentation mode
            if st == QUNS_RUNNING_D3D_FULL_SCREEN || st == QUNS_PRESENTATION_MODE {
                return true;
            }
            let _ = QUNS_BUSY;
        }
        let h = GetForegroundWindow();
        if h.0.is_null() || own.contains(&(h.0 as isize)) {
            return false;
        }
        if SHELL_CLASSES.contains(&class_name(h).as_str()) {
            return false;
        }
        if IsIconic(h).as_bool() {
            return false;
        }
        let mut r = RECT::default();
        if GetWindowRect(h, &mut r).is_err() {
            return false;
        }
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST), &mut mi).as_bool() {
            return false;
        }
        let m = mi.rcMonitor;
        if !(r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom) {
            return false;
        }
        // A maximized captioned window can fill the screen when the taskbar auto-hides: not fullscreen.
        let style = GetWindowLongW(h, GWL_STYLE) as u32;
        if style & WS_CAPTION.0 == WS_CAPTION.0 && IsZoomed(h).as_bool() {
            return false;
        }
        true
    }
}

unsafe extern "system" fn enum_cb(h: HWND, lp: LPARAM) -> BOOL {
    let res = &mut *(lp.0 as *mut HashMap<u32, String>);
    if !IsWindowVisible(h).as_bool() {
        return true.into();
    }
    if let Ok(owner) = GetWindow(h, GW_OWNER) {
        if !owner.0.is_null() {
            return true.into(); // owned popup
        }
    }
    if (GetWindowLongW(h, GWL_EXSTYLE) as u32) & WS_EX_TOOLWINDOW.0 != 0 {
        return true.into();
    }
    let len = GetWindowTextLengthW(h);
    if len == 0 {
        return true.into();
    }
    let mut cloaked: u32 = 0;
    let _ = DwmGetWindowAttribute(h, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut _, 4);
    if cloaked != 0 {
        return true.into();
    }
    let mut pid = 0u32;
    GetWindowThreadProcessId(h, Some(&mut pid));
    let mut buf = [0u16; 256];
    let n = GetWindowTextW(h, &mut buf);
    res.entry(pid).or_insert_with(|| String::from_utf16_lossy(&buf[..n.max(0) as usize]));
    true.into()
}

/// pid -> title for every real top-level app window (what the taskbar would show).
pub fn list_app_windows() -> HashMap<u32, String> {
    let mut res: HashMap<u32, String> = HashMap::new();
    unsafe {
        let _ = EnumWindows(Some(enum_cb), LPARAM(&mut res as *mut _ as isize));
    }
    res
}

#[derive(Clone, serde::Serialize)]
pub struct Adapter {
    pub name: String,
    #[serde(rename = "linkBps")]
    pub link_bps: f64,
    pub wifi: bool,
}

pub struct NetCounters {
    pub rx: f64,
    pub tx: f64,
    pub adapter: Option<Adapter>,
}

/// Sum of bytes in/out on physical, connected adapters, plus the busiest adapter's name and link speed.
pub fn net_counters() -> Option<NetCounters> {
    unsafe {
        let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
        if GetIfTable2(&mut table).is_err() || table.is_null() {
            return None;
        }
        let n = (*table).NumEntries as usize;
        let rows = std::slice::from_raw_parts((*table).Table.as_ptr(), n);
        let (mut rx, mut tx) = (0f64, 0f64);
        let mut best: Option<(f64, Adapter)> = None;
        for r in rows {
            let flags = r.InterfaceAndOperStatusFlags._bitfield;
            let hardware = flags & 1 != 0;
            let filter = flags & 2 != 0;
            if !hardware || filter {
                continue;
            }
            if r.OperStatus.0 != 1 {
                continue; // IfOperStatusUp
            }
            if r.Type == 24 {
                continue; // loopback
            }
            let (i, o) = (r.InOctets as f64, r.OutOctets as f64);
            rx += i;
            tx += o;
            if best.as_ref().map(|b| i + o > b.0).unwrap_or(true) {
                let end = r.Alias.iter().position(|&c| c == 0).unwrap_or(r.Alias.len());
                best = Some((i + o, Adapter { name: String::from_utf16_lossy(&r.Alias[..end]), link_bps: r.TransmitLinkSpeed as f64, wifi: r.Type == 71 }));
            }
        }
        FreeMibTable(table as *const _);
        Some(NetCounters { rx, tx, adapter: best.map(|b| b.1) })
    }
}

/// Native ICMP echo (blocking: call from a worker thread). RTT in ms, or None on loss.
pub fn ping(ip: std::net::Ipv4Addr, timeout_ms: u32) -> Option<f64> {
    unsafe {
        let h = IcmpCreateFile().ok()?;
        let data = [0x61u8; 32];
        // u64 storage keeps the reply 8-byte aligned (ICMP_ECHO_REPLY holds a pointer)
        let mut reply = [0u64; 32];
        let dest = u32::from_ne_bytes(ip.octets());
        let n = IcmpSendEcho(h, dest, data.as_ptr() as *const _, data.len() as u16, None, reply.as_mut_ptr() as *mut _, std::mem::size_of_val(&reply) as u32, timeout_ms);
        let _ = windows::Win32::NetworkManagement::IpHelper::IcmpCloseHandle(h);
        if n == 0 {
            return None;
        }
        let r = &*(reply.as_ptr() as *const ICMP_ECHO_REPLY);
        if r.Status != 0 {
            return None; // IP_SUCCESS
        }
        Some(r.RoundTripTime as f64)
    }
}

pub fn recycle_bin_info() -> (f64, f64) {
    unsafe {
        let mut info = SHQUERYRBINFO { cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32, ..Default::default() };
        if SHQueryRecycleBinW(PCWSTR::null(), &mut info).is_err() {
            return (0.0, 0.0);
        }
        (info.i64Size as f64, info.i64NumItems as f64)
    }
}

pub fn empty_recycle_bin() -> bool {
    // SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND
    unsafe { SHEmptyRecycleBinW(None, PCWSTR::null(), 1 | 2 | 4).is_ok() }
}

/// Seconds since the last keyboard / mouse input (Electron's powerMonitor.getSystemIdleTime).
pub fn user_idle_secs() -> u64 {
    use windows::Win32::System::SystemInformation::GetTickCount;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    unsafe {
        let mut li = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
        if !GetLastInputInfo(&mut li).as_bool() {
            return 0;
        }
        (GetTickCount().wrapping_sub(li.dwTime) / 1000) as u64
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Taskbar {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub edge: u32,
    pub autohide: bool,
}

/// Taskbar position in physical pixels, its edge (0 left, 1 top, 2 right, 3 bottom) and auto-hide state.
pub fn taskbar() -> Option<Taskbar> {
    unsafe {
        let mut d = APPBARDATA { cbSize: std::mem::size_of::<APPBARDATA>() as u32, ..Default::default() };
        let autohide = SHAppBarMessage(ABM_GETSTATE, &mut d) & 1 == 1;
        if SHAppBarMessage(ABM_GETTASKBARPOS, &mut d) == 0 {
            return None;
        }
        let r = d.rc;
        Some(Taskbar { x: r.left, y: r.top, width: r.right - r.left, height: r.bottom - r.top, edge: d.uEdge, autohide })
    }
}

/// Tells Explorer and other apps the colours changed.
pub fn broadcast_colors(timeout_ms: u32) {
    let s = wide("ImmersiveColorSet");
    unsafe {
        let mut r = 0usize;
        let _ = SendMessageTimeoutW(HWND_BROADCAST, WM_SETTINGCHANGE, WPARAM(0), LPARAM(s.as_ptr() as isize), SMTO_ABORTIFHUNG, timeout_ms, Some(&mut r));
    }
}

/// Drive letters C..Z with their size and free space.
pub fn disks() -> Vec<(String, f64, f64)> {
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let mut out = Vec::new();
    for c in b'C'..=b'Z' {
        let root = wide(&format!("{}:\\", c as char));
        let (mut avail, mut total, mut free) = (0u64, 0u64, 0u64);
        let ok = unsafe { GetDiskFreeSpaceExW(PCWSTR(root.as_ptr()), Some(&mut avail), Some(&mut total), Some(&mut free)) };
        if ok.is_ok() && total > 0 {
            out.push((format!("{}:", c as char), total as f64, avail as f64));
        }
    }
    out
}

/// "10.0.26100" style version, like Node's os.release().
pub fn os_release() -> String {
    #[repr(C)]
    struct OsVersionInfo {
        size: u32,
        major: u32,
        minor: u32,
        build: u32,
        platform: u32,
        csd: [u16; 128],
    }
    #[link(name = "ntdll")]
    extern "system" {
        fn RtlGetVersion(info: *mut OsVersionInfo) -> i32;
    }
    let mut v = OsVersionInfo { size: std::mem::size_of::<OsVersionInfo>() as u32, major: 0, minor: 0, build: 0, platform: 0, csd: [0; 128] };
    unsafe {
        RtlGetVersion(&mut v);
    }
    format!("{}.{}.{}", v.major, v.minor, v.build)
}

/// Shows a window without activating it (Electron's showInactive).
pub fn show_inactive(hwnd: isize) {
    unsafe {
        let _ = ShowWindow(HWND(hwnd as *mut _), SW_SHOWNOACTIVATE);
    }
}

/// Hides a window behind Tauri's back, so its own "visible" flag stays set: Tauri re-applies that
/// flag whenever a window style changes (e.g. click-through on/off), which would undo our state.
pub fn hide(hwnd: isize) {
    unsafe {
        let _ = ShowWindow(HWND(hwnd as *mut _), SW_HIDE);
    }
}

/// Clips a window to a strip so nothing outside it covers or is composited (Electron's setShape).
pub fn clip_window(hwnd: isize, w: i32, h: i32) {
    use windows::Win32::Graphics::Gdi::{CreateRectRgn, SetWindowRgn};
    unsafe {
        let rgn = CreateRectRgn(0, 0, w, h);
        let _ = SetWindowRgn(HWND(hwnd as *mut _), Some(rgn), true);
    }
}

/// Raises a window to the top of the topmost band without activating it.
pub fn move_top(hwnd: isize) {
    unsafe {
        let _ = SetWindowPos(HWND(hwnd as *mut _), Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
}
