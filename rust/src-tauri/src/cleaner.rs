// Cache / junk scanner and cleaner. Deletes folder *contents* only, never follows symlinks,
// and skips locked files. Same categories as the Electron edition.
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

#[cfg(windows)]
use crate::native_win as native;
#[cfg(target_os = "macos")]
use crate::native_mac as native;

const TWO_HOURS: Duration = Duration::from_secs(2 * 3600);

struct Cat {
    id: &'static str,
    name: &'static str,
    desc: &'static str,
    paths: Box<dyn Fn() -> Vec<PathBuf> + Send + Sync>,
    min_age: Option<Duration>,
    admin: bool,
    proc_name: Option<&'static str>,
    filter: Option<fn(&str) -> bool>,
    recycle: bool,
}

fn cat(id: &'static str, name: &'static str, desc: &'static str, paths: impl Fn() -> Vec<PathBuf> + Send + Sync + 'static) -> Cat {
    Cat { id, name, desc, paths: Box::new(paths), min_age: None, admin: false, proc_name: None, filter: None, recycle: false }
}

fn existing(v: Vec<PathBuf>) -> Vec<PathBuf> {
    v.into_iter().filter(|p| p.exists()).collect()
}

const CHROMIUM_SUB: [&str; 6] = ["Cache", "Code Cache", "GPUCache", "DawnGraphiteCache", "DawnWebGPUCache", "Service Worker/CacheStorage"];
const CHROMIUM_TOP: [&str; 3] = ["ShaderCache", "GrShaderCache", "GraphiteDawnCache"];

fn chromium_paths(user_data: PathBuf) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if !user_data.exists() {
        return out;
    }
    for t in CHROMIUM_TOP {
        out.push(user_data.join(t));
    }
    let re = regex::Regex::new(r"^(Default|Profile \d+|Guest Profile|System Profile)$").unwrap();
    if let Ok(rd) = std::fs::read_dir(&user_data) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if re.is_match(&n) {
                for s in CHROMIUM_SUB {
                    out.push(e.path().join(s));
                }
            }
        }
    }
    existing(out)
}

#[cfg(windows)]
fn categories() -> Vec<Cat> {
    let l = dirs::data_local_dir().unwrap_or_else(|| crate::util::home().join("AppData/Local"));
    let low = l.parent().map(|p| p.join("LocalLow")).unwrap_or_default();
    let win = PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into()));
    let (l1, l2, l3, l4, l5, l6, l7, l8, l9) = (l.clone(), l.clone(), l.clone(), l.clone(), l.clone(), l.clone(), l.clone(), l.clone(), l.clone());
    let (w1, w2) = (win.clone(), win.clone());
    vec![
        Cat { min_age: Some(TWO_HOURS), ..cat("usertemp", "User temp files", "Leftovers from installers and apps", || vec![std::env::temp_dir()]) },
        Cat { min_age: Some(TWO_HOURS), admin: true, ..cat("wintemp", "Windows temp", "System temp folder (some files need admin)", move || vec![w1.join("Temp")]) },
        Cat { proc_name: Some("chrome.exe"), ..cat("chrome", "Chrome cache", "Web, code and GPU caches", move || chromium_paths(l1.join("Google/Chrome/User Data"))) },
        Cat { proc_name: Some("msedge.exe"), ..cat("edge", "Edge cache", "Web, code and GPU caches", move || chromium_paths(l2.join("Microsoft/Edge/User Data"))) },
        Cat { proc_name: Some("brave.exe"), ..cat("brave", "Brave cache", "Web, code and GPU caches", move || chromium_paths(l3.join("BraveSoftware/Brave-Browser/User Data"))) },
        Cat {
            proc_name: Some("firefox.exe"),
            ..cat("firefox", "Firefox cache", "cache2 folders", move || {
                let root = l4.join("Mozilla/Firefox/Profiles");
                std::fs::read_dir(&root).map(|rd| existing(rd.flatten().map(|e| e.path().join("cache2")).collect())).unwrap_or_default()
            })
        },
        cat("shader", "GPU shader caches", "DirectX + NVIDIA caches, rebuilt on demand", move || vec![l5.join("D3DSCache"), l5.join("NVIDIA/DXCache"), l5.join("NVIDIA/GLCache"), low.join("NVIDIA/PerDriverVersion/DXCache")]),
        cat("inet", "Windows web cache", "INetCache used by Windows components", move || vec![l6.join("Microsoft/Windows/INetCache")]),
        cat("crash", "Crash dumps & error reports", "CrashDumps + Windows Error Reporting", move || vec![l7.join("CrashDumps"), l7.join("Microsoft/Windows/WER/ReportArchive"), l7.join("Microsoft/Windows/WER/ReportQueue")]),
        Cat {
            filter: Some(|n| {
                let n = n.to_lowercase();
                (n.starts_with("thumbcache_") || n.starts_with("iconcache_")) && n.ends_with(".db")
            }),
            ..cat("thumbs", "Thumbnail cache", "Explorer thumbnails (in-use files skipped)", move || vec![l8.join("Microsoft/Windows/Explorer")])
        },
        cat("devcache", "npm / pip caches", "Package download caches", move || {
            // npm may be pointed elsewhere (npm config cache); include that folder too
            let mut v = vec![l9.join("npm-cache/_cacache"), l9.join("pip/cache")];
            if let Some(c) = npm_cache_dir() {
                let p = c.join("_cacache");
                if !v.contains(&p) {
                    v.push(p);
                }
            }
            v
        }),
        Cat { admin: true, ..cat("wupdate", "Windows Update downloads", "Already-installed update files (needs admin)", move || vec![w2.join("SoftwareDistribution/Download")]) },
        Cat { recycle: true, ..cat("recycle", "Recycle Bin", "Everything in the Recycle Bin on all drives", Vec::new) },
    ]
}

/// The folder `npm config get cache` points at, read from the user's .npmrc (no child process).
#[cfg(windows)]
fn npm_cache_dir() -> Option<PathBuf> {
    let text = std::fs::read_to_string(crate::util::home().join(".npmrc")).ok()?;
    text.lines().find_map(|l| {
        let l = l.trim();
        let v = l.strip_prefix("cache")?.trim_start().strip_prefix('=')?;
        Some(PathBuf::from(v.trim().trim_matches('"')))
    })
}

#[cfg(target_os = "macos")]
fn categories() -> Vec<Cat> {
    let h = crate::util::home();
    let mc = h.join("Library/Caches");
    let support = h.join("Library/Application Support");
    let mut v = vec![Cat { min_age: Some(TWO_HOURS), ..cat("usertemp", "User temporary files", "Files older than two hours; recent files are retained", || vec![std::env::temp_dir()]) }];
    for (id, name, dir, proc_name) in [("chrome", "Chrome cache", "Google/Chrome", "google chrome"), ("edge", "Edge cache", "Microsoft Edge", "microsoft edge"), ("brave", "Brave cache", "BraveSoftware/Brave-Browser", "brave browser")] {
        let (s, c) = (support.clone(), mc.clone());
        v.push(Cat {
            proc_name: Some(proc_name),
            ..cat(id, name, "Web, code and GPU caches", move || {
                let mut p = chromium_paths(s.join(dir));
                p.extend(existing(vec![c.join(dir)]));
                p
            })
        });
    }
    let (c1, c2, c3, c4) = (mc.clone(), mc.clone(), mc.clone(), mc.clone());
    let (h1, h2, h3) = (h.clone(), h.clone(), h.clone());
    v.push(Cat { proc_name: Some("firefox"), ..cat("firefox", "Firefox cache", "Browser cache only", move || existing(vec![c1.join("Firefox")])) });
    v.push(Cat {
        proc_name: Some("safari"),
        ..cat("safari", "Safari cache", "Requires Full Disk Access on some macOS versions", move || existing(vec![c2.join("com.apple.Safari"), h1.join("Library/Containers/com.apple.Safari/Data/Library/Caches/com.apple.Safari")]))
    });
    v.push(cat("shader", "GPU shader cache", "Metal caches rebuilt on demand", move || existing(vec![c3.join("com.apple.metal")])));
    v.push(cat("crash", "User crash reports", "DiagnosticReports for this user", move || existing(vec![h2.join("Library/Logs/DiagnosticReports")])));
    v.push(cat("devcache", "npm / pip caches", "Package download caches", move || existing(vec![h3.join(".npm/_cacache"), c4.join("pip")])));
    v.push(Cat { recycle: true, ..cat("recycle", "Trash", "This user\u{2019}s Trash; macOS may require Full Disk Access", Vec::new) });
    v
}

fn dir_size(p: &Path) -> (f64, f64) {
    let (mut size, mut files) = (0f64, 0f64);
    let mut stack = vec![p.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_symlink() {
                continue;
            }
            if ft.is_dir() {
                stack.push(e.path());
            } else if let Ok(m) = std::fs::symlink_metadata(e.path()) {
                size += m.len() as f64;
                files += 1.0;
            }
        }
    }
    (size, files)
}

fn entries_of(c: &Cat, p: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(p) else { return vec![] };
    rd.flatten().filter(|e| c.filter.map(|f| f(&e.file_name().to_string_lossy())).unwrap_or(true)).map(|e| e.path()).collect()
}

fn aged_files(root: &Path, min_age: Duration, remove: bool) -> (f64, f64) {
    let Ok(m) = std::fs::symlink_metadata(root) else { return (0.0, 0.0) };
    if m.file_type().is_symlink() {
        return (0.0, 0.0);
    }
    if m.is_dir() {
        let (mut size, mut files) = (0f64, 0f64);
        if let Ok(rd) = std::fs::read_dir(root) {
            for e in rd.flatten() {
                let (s, f) = aged_files(&e.path(), min_age, remove);
                size += s;
                files += f;
            }
        }
        if remove {
            let _ = std::fs::remove_dir(root); // only succeeds once empty
        }
        return (size, files);
    }
    let age = m.modified().ok().and_then(|t| SystemTime::now().duration_since(t).ok()).unwrap_or_default();
    if age >= min_age {
        if remove {
            let _ = std::fs::remove_file(root);
        }
        return (m.len() as f64, 1.0);
    }
    (0.0, 0.0)
}

fn scan_category(c: &Cat) -> (f64, f64) {
    if c.recycle {
        return native::recycle_bin_info();
    }
    let (mut size, mut files) = (0f64, 0f64);
    for p in (c.paths)() {
        for e in entries_of(c, &p) {
            let Ok(m) = std::fs::symlink_metadata(&e) else { continue };
            if m.file_type().is_symlink() {
                continue;
            }
            let (s, f) = if let Some(age) = c.min_age {
                aged_files(&e, age, false)
            } else if m.is_dir() {
                dir_size(&e)
            } else {
                (m.len() as f64, 1.0)
            };
            size += s;
            files += f;
        }
    }
    (size, files)
}

pub fn scan(running: &HashSet<String>, progress: impl Fn(Value)) -> Vec<Value> {
    let mut out = Vec::new();
    for c in categories() {
        if !c.recycle && (c.paths)().is_empty() {
            continue; // app not installed
        }
        progress(json!({ "id": c.id, "name": c.name }));
        let (size, files) = scan_category(&c);
        out.push(json!({
            "id": c.id, "name": c.name, "desc": c.desc, "admin": c.admin, "size": size, "files": files,
            "running": c.proc_name.map(|p| running.contains(p)).unwrap_or(false),
        }));
    }
    out
}

pub fn clean(ids: &[String], progress: impl Fn(Value)) -> Vec<Value> {
    let cats = categories();
    let mut results = Vec::new();
    for id in ids {
        let Some(c) = cats.iter().find(|c| c.id == id) else { continue };
        progress(json!({ "id": c.id, "name": c.name }));
        let before = scan_category(c).0;
        if c.recycle {
            native::empty_recycle_bin();
        } else {
            for p in (c.paths)() {
                for e in entries_of(c, &p) {
                    if let Some(age) = c.min_age {
                        aged_files(&e, age, true);
                        continue;
                    }
                    let Ok(m) = std::fs::symlink_metadata(&e) else { continue };
                    // locked / access denied: skipped
                    let _ = if m.is_dir() { std::fs::remove_dir_all(&e) } else { std::fs::remove_file(&e) };
                }
            }
        }
        let after = scan_category(c).0;
        results.push(json!({ "id": c.id, "name": c.name, "freed": (before - after).max(0.0), "left": after }));
    }
    results
}
