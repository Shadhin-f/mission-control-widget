// Running processes grouped by executable, with memory and whether they own a visible app window.
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System, UpdateKind};

#[cfg(windows)]
use crate::native_win as native;
#[cfg(target_os = "macos")]
use crate::native_mac as native;

#[cfg(windows)]
const PROTECTED: &[&str] = &[
    "system", "system idle process", "registry", "smss.exe", "csrss.exe", "wininit.exe", "winlogon.exe", "services.exe",
    "lsass.exe", "lsaiso.exe", "svchost.exe", "dwm.exe", "fontdrvhost.exe", "memory compression", "secure system",
    "explorer.exe", "sihost.exe", "ctfmon.exe", "audiodg.exe", "spoolsv.exe", "msmpeng.exe", "searchhost.exe",
    "startmenuexperiencehost.exe", "shellexperiencehost.exe", "taskhostw.exe", "runtimebroker.exe", "conhost.exe",
];
#[cfg(target_os = "macos")]
const PROTECTED: &[&str] = &[
    "kernel_task", "launchd", "WindowServer", "loginwindow", "runningboardd", "systemstats", "cfprefsd", "securityd", "trustd",
    "coreaudiod", "bluetoothd", "airportd", "powerd", "distnoted", "notifyd", "opendirectoryd", "tccd", "amfid", "syspolicyd", "logd",
];

pub struct Row {
    pub name: String,
    pub pid: u32,
    pub mem: f64,
    pub parent: Option<u32>,
    #[allow(dead_code)]
    pub uid_foreign: bool,
}

fn system() -> System {
    System::new_with_specifics(RefreshKind::nothing().with_processes(ProcessRefreshKind::nothing().with_memory().with_user(UpdateKind::OnlyIfNotSet)))
}

pub fn tasklist() -> Vec<Row> {
    let mut sys = system();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing().with_memory().with_user(UpdateKind::OnlyIfNotSet));
    #[cfg(unix)]
    let me = unsafe { libc::getuid() };
    sys.processes()
        .values()
        .map(|p| {
            #[cfg(unix)]
            let uid_foreign = p.user_id().map(|u| **u != me).unwrap_or(true);
            #[cfg(not(unix))]
            let uid_foreign = false;
            let name = p.name().to_string_lossy().to_string();
            #[cfg(windows)]
            let name = if p.pid().as_u32() == 0 { "System Idle Process".into() } else { name };
            Row { name, pid: p.pid().as_u32(), mem: p.memory() as f64, parent: p.parent().map(|x| x.as_u32()), uid_foreign }
        })
        .collect()
}

/// Our own process and everything it started (the WebView2 / WKWebView helpers).
pub fn own_pids(rows: &[Row]) -> HashSet<u32> {
    let mut own: HashSet<u32> = [std::process::id()].into();
    loop {
        let before = own.len();
        for r in rows {
            if let Some(p) = r.parent {
                if own.contains(&p) && p != r.pid {
                    own.insert(r.pid);
                }
            }
        }
        if own.len() == before {
            break;
        }
    }
    own
}

pub fn list() -> Value {
    let rows = tasklist();
    let windows = native::list_app_windows();
    let own = own_pids(&rows);
    struct G {
        name: String,
        pids: Vec<u32>,
        mem: f64,
        title: String,
        app: bool,
        own: bool,
        protected: bool,
    }
    let mut groups: HashMap<String, G> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for r in &rows {
        #[cfg(windows)]
        let key = r.name.to_lowercase();
        #[cfg(not(windows))]
        let key = r.name.clone();
        let g = groups.entry(key.clone()).or_insert_with(|| {
            order.push(key.clone());
            G { name: r.name.clone(), pids: vec![], mem: 0.0, title: String::new(), app: false, own: false, protected: false }
        });
        g.pids.push(r.pid);
        g.mem += r.mem;
        if let Some(t) = windows.get(&r.pid) {
            g.app = true;
            if g.title.is_empty() {
                g.title = t.clone();
            }
        }
        if own.contains(&r.pid) {
            g.own = true;
        }
        #[cfg(windows)]
        {
            g.protected |= PROTECTED.contains(&key.as_str());
        }
        #[cfg(not(windows))]
        {
            g.protected |= r.uid_foreign || PROTECTED.contains(&r.name.as_str()) || r.pid <= 1;
        }
    }
    let mut all: Vec<G> = order.into_iter().filter_map(|k| groups.remove(&k)).collect();
    all.sort_by(|a, b| b.mem.partial_cmp(&a.mem).unwrap_or(std::cmp::Ordering::Equal));
    let to_json = |g: &G| json!({ "name": g.name, "pids": g.pids, "mem": g.mem, "title": g.title, "app": g.app, "own": g.own, "count": g.pids.len(), "protected": g.protected || g.own });
    let apps: Vec<Value> = all.iter().filter(|g| g.app).map(to_json).collect();
    let background: Vec<Value> = all.iter().filter(|g| !g.app).take(80).map(to_json).collect();
    json!({ "apps": apps, "background": background, "total": rows.len() })
}

pub fn running_names() -> HashSet<String> {
    tasklist().into_iter().map(|r| r.name.to_lowercase()).collect()
}

pub async fn kill(pids: Vec<u32>) -> Result<Value, String> {
    if pids.is_empty() || pids.iter().any(|&p| p <= 1) {
        return Err("Invalid process IDs".into());
    }
    // Never kill protected processes or the widget itself, whatever the page asks for
    let listed = list();
    let killable: HashSet<u64> = ["apps", "background"]
        .iter()
        .flat_map(|k| listed[*k].as_array().cloned().unwrap_or_default())
        .filter(|g| !g["protected"].as_bool().unwrap_or(true))
        .flat_map(|g| g["pids"].as_array().cloned().unwrap_or_default())
        .filter_map(|p| p.as_u64())
        .collect();
    if pids.iter().any(|p| !killable.contains(&(*p as u64))) {
        return Err("System and widget processes are protected".into());
    }
    #[cfg(windows)]
    {
        let mut args: Vec<String> = vec!["/F".into(), "/T".into()];
        for p in &pids {
            args.push("/PID".into());
            args.push(p.to_string());
        }
        let a: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        let r = crate::util::run("taskkill", &a, 20000).await;
        let msg = if r.err.trim().is_empty() { r.out } else { r.err };
        Ok(json!({ "ok": r.ok, "message": msg.trim() }))
    }
    #[cfg(unix)]
    {
        let mut ok = true;
        for p in &pids {
            if unsafe { libc::kill(*p as i32, libc::SIGTERM) } != 0 {
                ok = false;
            }
        }
        Ok(json!({ "ok": ok, "message": if ok { "Termination requested" } else { "Some processes could not be stopped" } }))
    }
}
