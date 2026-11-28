// Live system stats: CPU, RAM, GPU (nvidia-smi stream / macOS IOAccelerator), disks, network throughput.
use crate::util::now_ms;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};
use tauri::{AppHandle, Emitter};
use tokio::io::AsyncBufReadExt;

#[cfg(windows)]
use crate::native_win as native;
#[cfg(target_os = "macos")]
use crate::native_mac as native;

pub struct Stats {
    gen: AtomicU64,
    running: AtomicU64,
    gpu: Mutex<Value>,
    disks: Mutex<Value>,
    pub static_info: Mutex<Value>,
}

fn has_nvidia_smi() -> bool {
    #[cfg(windows)]
    {
        crate::util::env_path_dirs().iter().any(|d| d.join("nvidia-smi.exe").exists())
    }
    #[cfg(not(windows))]
    {
        false
    }
}

impl Stats {
    pub fn new() -> Arc<Self> {
        let sys = System::new_with_specifics(RefreshKind::nothing().with_cpu(CpuRefreshKind::everything()).with_memory(MemoryRefreshKind::everything()));
        let model = sys.cpus().first().map(|c| c.brand().split_whitespace().collect::<Vec<_>>().join(" ")).unwrap_or_else(|| "CPU".into());
        #[cfg(windows)]
        let os = format!("Windows {}", native::os_release());
        #[cfg(target_os = "macos")]
        let os = format!("macOS {}", System::os_version().unwrap_or_default());
        Arc::new(Self {
            gen: AtomicU64::new(0),
            running: AtomicU64::new(0),
            gpu: Mutex::new(Value::Null),
            disks: Mutex::new(json!([])),
            static_info: Mutex::new(json!({
                "cpuModel": model,
                "cores": sys.cpus().len(),
                "host": System::host_name().unwrap_or_default(),
                "os": os,
                "totalMem": sys.total_memory() as f64,
            })),
        })
    }

    fn read_disks(&self) {
        let list: Vec<Value> = native::disks().into_iter().map(|(d, t, f)| json!({ "drive": d, "total": t, "free": f })).collect();
        *self.disks.lock().unwrap() = Value::Array(list);
    }

    pub fn start(self: &Arc<Self>, app: AppHandle) {
        if self.running.swap(1, Ordering::SeqCst) == 1 {
            return;
        }
        let gen = self.gen.fetch_add(1, Ordering::SeqCst) + 1;
        let me = self.clone();
        let alive = move |m: &Arc<Stats>| m.gen.load(Ordering::SeqCst) == gen;

        // Disks every 15 s
        {
            let me = me.clone();
            let alive = alive.clone();
            tauri::async_runtime::spawn(async move {
                while alive(&me) {
                    me.read_disks();
                    tokio::time::sleep(std::time::Duration::from_secs(15)).await;
                }
            });
        }

        // GPU: one long-lived nvidia-smi that prints a CSV line every second
        if has_nvidia_smi() {
            let me = me.clone();
            let alive = alive.clone();
            tauri::async_runtime::spawn(async move {
                let mut c = crate::util::cmd("nvidia-smi");
                c.args([
                    "--query-gpu=name,utilization.gpu,memory.used,memory.total,temperature.gpu,power.draw,fan.speed",
                    "--format=csv,noheader,nounits",
                    "-lms",
                    "1000",
                ])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .stdin(std::process::Stdio::null())
                .kill_on_drop(true);
                let Ok(mut child) = c.spawn() else { return };
                let mut lines = tokio::io::BufReader::new(child.stdout.take().unwrap()).lines();
                let num = |s: &str| s.trim().parse::<f64>().ok().map(Value::from).unwrap_or(Value::Null);
                loop {
                    if !alive(&me) {
                        break;
                    }
                    let next = tokio::time::timeout(std::time::Duration::from_secs(3), lines.next_line()).await;
                    match next {
                        Ok(Ok(Some(line))) => {
                            let f: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
                            if f.len() < 5 {
                                continue;
                            }
                            *me.gpu.lock().unwrap() = json!({
                                "name": f[0], "util": num(f[1]), "vramUsed": num(f[2]), "vramTotal": num(f[3]),
                                "temp": num(f[4]), "power": f.get(5).map(|s| num(s)).unwrap_or(Value::Null),
                                "fan": f.get(6).map(|s| num(s)).unwrap_or(Value::Null),
                            });
                        }
                        Ok(_) => break,
                        Err(_) => continue,
                    }
                }
                let _ = child.kill().await;
            });
        }

        #[cfg(target_os = "macos")]
        {
            let me2 = me.clone();
            let alive2 = alive.clone();
            let me = me.clone();
            let alive = alive.clone();
            tauri::async_runtime::spawn(async move {
                let empty = Value::Object(Default::default());
                let v = crate::util::run("/usr/bin/sw_vers", &["-productVersion"], 5000).await.out.trim().to_string();
                if !v.is_empty() {
                    me.static_info.lock().unwrap()["os"] = json!(format!("macOS {v}"));
                }
                if let Ok(g) = native::helper("gpu", &empty, false).await {
                    *me.gpu.lock().unwrap() = json!({ "name": g.get("name"), "unified": g.get("unified"), "util": null, "vramUsed": null, "vramTotal": null, "temp": null, "power": null });
                }
                let re = regex::Regex::new(r#""Device Utilization %"\s*=\s*(\d+)"#).unwrap();
                while alive(&me) {
                    let t = crate::util::run("/usr/sbin/ioreg", &["-r", "-c", "IOAccelerator", "-l"], 8000).await.out;
                    let util = re.captures(&t).and_then(|c| c[1].parse::<f64>().ok());
                    if let Some(g) = me.gpu.lock().unwrap().as_object_mut() {
                        g.insert("util".into(), util.map(Value::from).unwrap_or(Value::Null));
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
            });
            tauri::async_runtime::spawn(async move {
                while alive2(&me2) {
                    native::refresh().await;
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            });
        }

        // The 1 s tick
        tauri::async_runtime::spawn(async move {
            let mut sys = System::new_with_specifics(RefreshKind::nothing().with_cpu(CpuRefreshKind::everything()).with_memory(MemoryRefreshKind::everything()));
            let mut prev_net: Option<(f64, f64, f64)> = native::net_counters().map(|n| (n.rx, n.tx, now_ms()));
            let mut adapter: Value = Value::Null;
            tokio::time::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL).await;
            while alive(&me) {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                if !alive(&me) {
                    break;
                }
                sys.refresh_cpu_all();
                sys.refresh_memory();
                let cores: Vec<f64> = sys.cpus().iter().map(|c| (c.cpu_usage() as f64).clamp(0.0, 100.0)).collect();
                let cpu = if cores.is_empty() { 0.0 } else { cores.iter().sum::<f64>() / cores.len() as f64 };
                let now = now_ms();
                let (mut down, mut up) = (0.0, 0.0);
                if let Some(n) = native::net_counters() {
                    if let Some((rx, tx, t)) = prev_net {
                        let dt = ((now - t) / 1000.0).max(0.001);
                        down = ((n.rx - rx) / dt).max(0.0);
                        up = ((n.tx - tx) / dt).max(0.0);
                    }
                    prev_net = Some((n.rx, n.tx, now));
                    adapter = serde_json::to_value(&n.adapter).unwrap_or(Value::Null);
                }
                let total = sys.total_memory() as f64;
                // On macOS, available_memory subtracts compressed pages and can saturate at zero
                // even with reclaimable inactive RAM. Use the OS-backed physical-used count instead.
                #[cfg(target_os = "macos")]
                let used = (sys.used_memory() as f64).min(total);
                #[cfg(not(target_os = "macos"))]
                let used = total - sys.available_memory() as f64;
                let payload = json!({
                    "t": now,
                    "cpu": cpu, "cores": cores, "cpuMhz": sys.cpus().first().map(|c| c.frequency()).unwrap_or(0),
                    "mem": { "total": total, "used": used },
                    "gpu": me.gpu.lock().unwrap().clone(),
                    "disks": me.disks.lock().unwrap().clone(),
                    "net": { "down": down, "up": up, "adapter": adapter },
                    "uptime": System::uptime(),
                });
                let _ = app.emit("stats", payload);
            }
        });
    }

    pub fn stop(&self) {
        self.running.store(0, Ordering::SeqCst);
        self.gen.fetch_add(1, Ordering::SeqCst);
    }
}
