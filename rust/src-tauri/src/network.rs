// Live latency/loss monitor and the on-demand internet speed test (Cloudflare endpoints).
use crate::util::now_ms;
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

const TARGET: &str = "1.1.1.1";

async fn native_ping(timeout_ms: u32) -> Option<f64> {
    #[cfg(windows)]
    {
        let ip: std::net::Ipv4Addr = TARGET.parse().unwrap();
        tokio::task::spawn_blocking(move || crate::native_win::ping(ip, timeout_ms)).await.ok().flatten()
    }
    #[cfg(target_os = "macos")]
    {
        crate::native_mac::ping(TARGET, timeout_ms).await
    }
}

async fn tcp_ping() -> Option<f64> {
    let t0 = Instant::now();
    match tokio::time::timeout(Duration::from_millis(2000), tokio::net::TcpStream::connect((TARGET, 443))).await {
        Ok(Ok(_)) => Some(t0.elapsed().as_secs_f64() * 1000.0),
        _ => None,
    }
}

async fn probe() -> Option<f64> {
    if let Some(t) = native_ping(1500).await {
        return Some(t);
    }
    tcp_ping().await.map(|t| t.round()) // ICMP may be filtered on some networks
}

struct Summary {
    avg: Option<f64>,
    min: Option<f64>,
    jitter: f64,
    loss: f64,
    count: usize,
}

fn summarize(samples: &[Option<f64>]) -> Summary {
    let ok: Vec<f64> = samples.iter().flatten().copied().collect();
    let loss = if samples.is_empty() { 0.0 } else { (samples.len() - ok.len()) as f64 / samples.len() as f64 * 100.0 };
    let avg = if ok.is_empty() { None } else { Some(ok.iter().sum::<f64>() / ok.len() as f64) };
    let jitter = if ok.len() > 1 { ok.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f64>() / (ok.len() - 1) as f64 } else { 0.0 };
    Summary { avg, min: ok.iter().copied().reduce(f64::min), jitter, loss, count: samples.len() }
}

pub struct NetMon {
    gen: AtomicU64,
    live: AtomicBool,
    testing: AtomicBool,
    samples: Mutex<Vec<Option<f64>>>,
}

impl NetMon {
    pub fn new() -> Arc<Self> {
        Arc::new(Self { gen: AtomicU64::new(0), live: AtomicBool::new(false), testing: AtomicBool::new(false), samples: Mutex::new(Vec::new()) })
    }

    pub fn start_live(self: &Arc<Self>, app: AppHandle) {
        if self.live.swap(true, Ordering::SeqCst) {
            return;
        }
        let gen = self.gen.fetch_add(1, Ordering::SeqCst) + 1;
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            while me.gen.load(Ordering::SeqCst) == gen {
                let t = probe().await;
                let payload = {
                    let mut s = me.samples.lock().unwrap();
                    s.push(t);
                    if s.len() > 40 {
                        s.remove(0);
                    }
                    let sum = summarize(&s);
                    let recent = &s[s.len().saturating_sub(3)..];
                    let status = if recent.len() == 3 && recent.iter().all(|x| x.is_none()) {
                        "offline"
                    } else if t.is_none() || sum.loss > 5.0 || sum.avg.unwrap_or(0.0) > 150.0 {
                        "unstable"
                    } else {
                        "online"
                    };
                    json!({ "last": t, "avg": sum.avg, "min": sum.min, "jitter": sum.jitter, "loss": sum.loss, "count": sum.count, "status": status })
                };
                if me.gen.load(Ordering::SeqCst) != gen {
                    break;
                }
                let _ = app.emit("latency", payload);
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
        });
    }

    pub fn stop_live(&self) {
        self.live.store(false, Ordering::SeqCst);
        self.gen.fetch_add(1, Ordering::SeqCst);
    }

    pub async fn speed_test(&self, app: &AppHandle) -> Result<Value, String> {
        if self.testing.swap(true, Ordering::SeqCst) {
            return Err("A test is already running".into());
        }
        let r = run_test(app).await;
        self.testing.store(false, Ordering::SeqCst);
        r
    }
}

fn client() -> reqwest::Client {
    reqwest::Client::builder().user_agent("MissionControl/1.0").build().unwrap_or_default()
}

async fn run_test(app: &AppHandle) -> Result<Value, String> {
    let progress = |v: Value| {
        let _ = app.emit("speed:progress", v);
    };
    let http = client();
    // /meta answers with {} unless the request carries the speed test's own Referer
    let meta: Value = match http.get("https://speed.cloudflare.com/meta").header("Referer", "https://speed.cloudflare.com/").timeout(Duration::from_secs(4)).send().await {
        Ok(r) => r.json().await.unwrap_or(Value::Null),
        Err(_) => Value::Null,
    };

    // 1) Latency / jitter / packet loss: 30 native ICMP echoes, 80 ms apart
    progress(json!({ "phase": "ping", "pct": 0 }));
    let pings = Arc::new(Mutex::new(Vec::<Option<f64>>::new()));
    let mut jobs = Vec::new();
    for _ in 0..30 {
        let pings = pings.clone();
        let app = app.clone();
        jobs.push(tauri::async_runtime::spawn(async move {
            let t = native_ping(1000).await;
            let n = {
                let mut p = pings.lock().unwrap();
                p.push(t);
                p.len()
            };
            let _ = app.emit("speed:progress", json!({ "phase": "ping", "pct": n as f64 / 30.0, "value": t }));
        }));
        tokio::time::sleep(Duration::from_millis(80)).await;
    }
    for j in jobs {
        let _ = j.await;
    }
    let mut ping = summarize(&pings.lock().unwrap());
    if ping.loss >= 100.0 {
        // ICMP blocked, fall back to TCP handshakes
        let mut tcp = Vec::new();
        for _ in 0..10 {
            tcp.push(tcp_ping().await);
        }
        ping = summarize(&tcp);
    }

    // 2) Download: 6 parallel streams for 10 s (10 MB chunks: the server can stall on huge single requests)
    let down = measure(app, "download", 10_000, 6, |http, add, stop| async move {
        let url = format!("https://speed.cloudflare.com/__down?bytes=10000000&r={}", rand::random::<f64>());
        let r = http.get(url).header("Cache-Control", "no-store").header("Referer", "https://speed.cloudflare.com/").send().await.map_err(|e| e.to_string())?;
        if !r.status().is_success() {
            return Err(format!("HTTP {}", r.status().as_u16()));
        }
        let mut s = r.bytes_stream();
        while let Some(chunk) = s.next().await {
            if stop.load(Ordering::SeqCst) {
                break;
            }
            add(chunk.map_err(|e| e.to_string())?.len());
        }
        Ok(())
    })
    .await;

    // 3) Upload: 4 parallel streams of 1 MB posts for 8 s
    let chunk: Arc<Vec<u8>> = Arc::new((0..(1 << 20)).map(|_| rand::random::<u8>()).collect());
    let up = measure(app, "upload", 8_000, 4, move |http, add, _stop| {
        let chunk = chunk.clone();
        async move {
            // Count bytes as the HTTP client pulls them for the socket, not when the server replies:
            // the reply can come back before the whole body has gone out, which over-counts.
            let body = futures_util::stream::unfold(0usize, move |off| {
                let (chunk, add) = (chunk.clone(), add.clone());
                async move {
                    if off >= chunk.len() {
                        return None;
                    }
                    let end = (off + 64 * 1024).min(chunk.len());
                    add(end - off);
                    Some((Ok::<Vec<u8>, std::io::Error>(chunk[off..end].to_vec()), end))
                }
            });
            let r = http
                .post("https://speed.cloudflare.com/__up")
                .header("Content-Type", "application/octet-stream")
                .header("Referer", "https://speed.cloudflare.com/")
                .header("Content-Length", (1usize << 20).to_string())
                .body(reqwest::Body::wrap_stream(body))
                .send()
                .await
                .map_err(|e| e.to_string())?;
            if !r.status().is_success() {
                return Err(format!("HTTP {}", r.status().as_u16()));
            }
            Ok(())
        }
    })
    .await;
    // A phase that moved no data at all failed; don't record it as "0 Mbps"
    if down <= 0.0 {
        return Err("The download test got no data from the server. Try again in a minute.".into());
    }
    if up <= 0.0 {
        return Err("The upload test could not reach the server. Try again in a minute.".into());
    }
    let server = match (meta.get("city").and_then(|v| v.as_str()), meta.get("colo").and_then(|v| v.as_str())) {
        (Some(city), Some(colo)) if !city.is_empty() => format!("{city} ({colo})"),
        _ => "Cloudflare".into(),
    };
    let result = json!({
        "t": now_ms(),
        "ping": ping.avg, "jitter": ping.jitter, "loss": ping.loss,
        "down": down, "up": up,
        "server": server,
        "isp": meta.get("asOrganization").and_then(|v| v.as_str()).unwrap_or(""),
    });
    progress(json!({ "phase": "done", "result": result }));
    Ok(result)
}

type Add = Arc<dyn Fn(usize) + Send + Sync>;

/// Runs `streams` loops of `worker` for `ms`. Returns Mbps, ignoring the first 1.5 s ramp-up.
async fn measure<F, Fut>(app: &AppHandle, phase: &'static str, ms: u64, streams: usize, worker: F) -> f64
where
    F: Fn(reqwest::Client, Add, Arc<AtomicBool>) -> Fut + Send + Sync + 'static + Clone,
    Fut: std::future::Future<Output = Result<(), String>> + Send + 'static,
{
    struct Acc {
        bytes: f64,
        steady: f64,
        steady_start: Option<Instant>,
        window: std::collections::VecDeque<(Instant, usize)>,
    }
    let start = Instant::now();
    let acc = Arc::new(Mutex::new(Acc { bytes: 0.0, steady: 0.0, steady_start: None, window: Default::default() }));
    let stop = Arc::new(AtomicBool::new(false));
    let add: Add = {
        let acc = acc.clone();
        Arc::new(move |b: usize| {
            let mut a = acc.lock().unwrap();
            let now = Instant::now();
            a.bytes += b as f64;
            if now.duration_since(start) > Duration::from_millis(1500) {
                if a.steady_start.is_none() {
                    a.steady_start = Some(now);
                }
                a.steady += b as f64;
            }
            a.window.push_back((now, b));
        })
    };
    // Progress ticker
    let ticker = {
        let acc = acc.clone();
        let stop = stop.clone();
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            while !stop.load(Ordering::SeqCst) {
                let mbps = {
                    let mut a = acc.lock().unwrap();
                    let now = Instant::now();
                    while a.window.front().map(|w| now.duration_since(w.0) > Duration::from_secs(1)).unwrap_or(false) {
                        a.window.pop_front();
                    }
                    a.window.iter().map(|w| w.1).sum::<usize>() as f64 * 8.0 / 1e6
                };
                let pct = (start.elapsed().as_millis() as f64 / ms as f64).min(1.0);
                let _ = app.emit("speed:progress", json!({ "phase": phase, "pct": pct, "mbps": mbps }));
                tokio::time::sleep(Duration::from_millis(150)).await;
            }
        })
    };
    let http = client();
    let mut loops = Vec::new();
    for _ in 0..streams {
        let (w, add, stop, http) = (worker.clone(), add.clone(), stop.clone(), http.clone());
        loops.push(tauri::async_runtime::spawn(async move {
            while !stop.load(Ordering::SeqCst) {
                if w(http.clone(), add.clone(), stop.clone()).await.is_err() && !stop.load(Ordering::SeqCst) {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
            }
        }));
    }
    tokio::time::sleep(Duration::from_millis(ms)).await;
    stop.store(true, Ordering::SeqCst);
    // Abort in-flight requests (the JS version aborts its fetches here)
    for l in &loops {
        l.abort();
    }
    let _ = ticker.await;
    let end = Instant::now();
    let a = acc.lock().unwrap();
    if let Some(s) = a.steady_start {
        if end.duration_since(s) > Duration::from_millis(500) {
            return a.steady * 8.0 / end.duration_since(s).as_secs_f64() / 1e6;
        }
    }
    a.bytes * 8.0 / end.duration_since(start).as_secs_f64() / 1e6
}
