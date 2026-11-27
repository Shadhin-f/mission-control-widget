// Bridge to the OpenAI Codex CLI (uses your existing Codex / ChatGPT sign-in).
// Chat: `codex exec --json` (and `codex exec resume <thread>` for follow-ups).
// Limits: `codex app-server` JSON-RPC `account/rateLimits/read`, which costs no usage.
use crate::claude::EventFn;
use crate::util::{cli_cmd, home, now_ms, stop_tree};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};

fn preamble() -> String {
    let mac = cfg!(target_os = "macos");
    [
        format!("[Context: you are the assistant inside the user's \"Mission Control\" desktop widget on their {}.", if mac { "macOS Mac" } else { "Windows 11 PC" }),
        if mac { "Use macOS shell commands and AppleScript for system actions. Open apps with open -a." } else { "The user controls the PC through you: opening apps and folders, managing files, checking and changing system state. Use PowerShell." }.into(),
        "The chat window is small: answer in a few short sentences.".into(),
        "Before anything destructive or hard to undo, say exactly what you will do and wait for the user to confirm.]".into(),
        String::new(),
        String::new(),
    ]
    .join("\n")
}

fn ver_key(name: &str) -> [u64; 3] {
    let re = regex::Regex::new(r"(\d+)\.(\d+)\.(\d+)").unwrap();
    re.captures(name).map(|c| [c[1].parse().unwrap_or(0), c[2].parse().unwrap_or(0), c[3].parse().unwrap_or(0)]).unwrap_or([0; 3])
}

/// Bounded search for codex.exe inside an npm package folder (layout differs between releases)
#[cfg(windows)]
fn find_exe_in(dir: &Path, depth: u32) -> Option<PathBuf> {
    let entries: Vec<_> = std::fs::read_dir(dir).ok()?.flatten().collect();
    for e in &entries {
        if e.file_type().map(|t| t.is_file()).unwrap_or(false) && e.file_name().to_string_lossy().eq_ignore_ascii_case("codex.exe") {
            return Some(e.path());
        }
    }
    if depth == 0 {
        return None;
    }
    entries.iter().filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false)).find_map(|e| find_exe_in(&e.path(), depth - 1))
}

/// A real Codex CLI install: PATH (npm, WinGet, Scoop, standalone installer), or npm's global folder.
fn find_codex_cli() -> Option<PathBuf> {
    #[cfg(unix)]
    {
        crate::util::find_unix_cli("codex")
    }
    #[cfg(windows)]
    {
        let path = crate::util::env_path_dirs();
        if let Some(p) = path.iter().map(|d| d.join("codex.exe")).find(|p| p.is_file()) {
            return Some(p);
        }
        let app_data = dirs::data_dir().unwrap_or_default();
        let local = dirs::data_local_dir().unwrap_or_default();
        for p in [home().join(".local/bin/codex.exe"), local.join("Microsoft/WinGet/Links/codex.exe"), local.join("Programs/codex/codex.exe"), home().join("scoop/shims/codex.exe")] {
            if p.is_file() {
                return Some(p);
            }
        }
        // npm i -g @openai/codex: prefer the native binary over the .cmd shim
        for pkg in ["codex", "codex-win32-x64"] {
            if let Some(hit) = find_exe_in(&app_data.join("npm/node_modules/@openai").join(pkg), 5) {
                return Some(hit);
            }
        }
        path.iter().chain(std::iter::once(&app_data.join("npm"))).map(|d| d.join("codex.cmd")).find(|p| p.is_file())
    }
}

/// The OpenAI extension for VS Code / Cursor / Windsurf bundles a Codex CLI. Only used when opted in.
fn find_editor_codex() -> Option<PathBuf> {
    let (plat, exe) = if cfg!(target_os = "macos") {
        (if cfg!(target_arch = "aarch64") { "darwin-aarch64" } else { "darwin-x86_64" }, "codex")
    } else {
        ("windows-x86_64", "codex.exe")
    };
    for editor in [".vscode", ".vscode-insiders", ".cursor", ".windsurf"] {
        let root = home().join(editor).join("extensions");
        let Ok(rd) = std::fs::read_dir(&root) else { continue };
        let mut dirs: Vec<String> = rd.flatten().map(|e| e.file_name().to_string_lossy().to_string()).filter(|d| d.starts_with("openai.chatgpt-")).collect();
        dirs.sort_by(|a, b| ver_key(b).cmp(&ver_key(a)));
        for d in dirs {
            let p = root.join(&d).join("bin").join(plat).join(exe);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// The model from ~/.codex/config.toml when the account can use it; otherwise the first listed model.
fn pick_model() -> Option<String> {
    let h = home().join(".codex");
    let configured = std::fs::read_to_string(h.join("config.toml")).ok().and_then(|t| regex::Regex::new(r#"(?m)^\s*model\s*=\s*"([^"]+)""#).unwrap().captures(&t).map(|c| c[1].to_string()));
    let listed: Vec<String> = std::fs::read_to_string(h.join("models_cache.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v["models"].as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter(|m| m["visibility"] == "list")
        .filter_map(|m| m["slug"].as_str().map(String::from))
        .collect();
    if listed.is_empty() || configured.as_ref().map(|c| listed.contains(c)).unwrap_or(false) {
        return None; // let Codex use its own setting
    }
    listed.into_iter().next()
}

fn window_label(mins: f64) -> String {
    if mins <= 0.0 {
        return "?".into();
    }
    let m = mins as i64;
    if m % 1440 == 0 {
        format!("{}d", m / 1440)
    } else if m % 60 == 0 {
        format!("{}h", m / 60)
    } else {
        format!("{m}m")
    }
}

/// Codex rate-limit windows (camelCase or snake_case) -> [{ label, pct, resetsAt(ms), mins }], shortest first.
pub fn normalize_windows(list: &[&Value], base_ms: f64) -> Vec<Value> {
    let num = |w: &Value, a: &str, b: &str| w.get(a).and_then(|v| v.as_f64()).or_else(|| w.get(b).and_then(|v| v.as_f64()));
    let mut out: Vec<(f64, Value)> = list
        .iter()
        .filter(|w| w.is_object())
        .map(|w| {
            let mins = num(w, "windowDurationMins", "window_minutes").unwrap_or(0.0);
            let pct = num(w, "usedPercent", "used_percent").unwrap_or(0.0);
            let resets = match num(w, "resetsAt", "resets_at") {
                Some(r) if r > 0.0 => r * 1000.0,
                _ => w.get("resets_in_seconds").and_then(|v| v.as_f64()).map(|s| base_ms + s * 1000.0).unwrap_or(0.0),
            };
            (mins, json!({ "label": window_label(mins), "pct": pct, "resetsAt": resets, "mins": mins }))
        })
        .collect();
    out.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    out.into_iter().map(|x| x.1).collect()
}

/// Fallback for Codex versions without app-server rate limits: the newest `token_count` event in
/// ~/.codex/sessions carries the same windows. Only that event's rate_limits field is read.
pub fn limits_from_sessions() -> Option<Value> {
    use chrono::{Datelike, Duration, Local};
    let root = home().join(".codex/sessions");
    let mut files: Vec<(PathBuf, std::time::SystemTime)> = Vec::new();
    let now = Local::now();
    for d in 0..8 {
        if files.len() >= 6 {
            break;
        }
        let day = now - Duration::days(d);
        let dir = root.join(day.year().to_string()).join(format!("{:02}", day.month())).join(format!("{:02}", day.day()));
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            if e.file_name().to_string_lossy().ends_with(".jsonl") {
                if let Ok(t) = e.metadata().and_then(|m| m.modified()) {
                    files.push((e.path(), t));
                }
            }
        }
    }
    files.sort_by(|a, b| b.1.cmp(&a.1));
    for (p, t) in files.into_iter().take(6) {
        let Ok(bytes) = std::fs::read(&p) else { continue };
        let tail = &bytes[bytes.len().saturating_sub(512 * 1024)..];
        let text = String::from_utf8_lossy(tail);
        let file_ms = t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0);
        for line in text.lines().filter(|l| l.contains("\"rate_limits\"")).collect::<Vec<_>>().into_iter().rev() {
            let Ok(m) = serde_json::from_str::<Value>(line) else { continue };
            let rl = if m["payload"]["rate_limits"].is_object() { &m["payload"]["rate_limits"] } else { &m["rate_limits"] };
            if !rl.is_object() {
                continue;
            }
            let base = m["timestamp"].as_str().and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()).map(|d| d.timestamp_millis() as f64).unwrap_or(file_ms);
            let windows = normalize_windows(&[&rl["primary"], &rl["secondary"]], base);
            if !windows.is_empty() {
                return Some(json!({ "windows": windows, "plan": rl["plan_type"], "at": base, "source": "sessions" }));
            }
        }
    }
    None
}

fn error_text(raw: &str) -> String {
    let mut s = raw.to_string();
    for _ in 0..2 {
        match serde_json::from_str::<Value>(&s) {
            Ok(j) => {
                let next = j["error"]["message"].as_str().or(j["message"].as_str()).map(String::from);
                match next {
                    Some(n) => s = n,
                    None => break,
                }
            }
            Err(_) => break,
        }
    }
    s
}

pub struct Codex {
    pub exe: Mutex<Option<PathBuf>>,
    pub editor_exe: Mutex<Option<PathBuf>>,
    thread: Mutex<Option<String>>,
    child: Mutex<Option<u32>>,
    stopped: AtomicBool,
    on_event: EventFn,
    pub on_turn_finished: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

impl Codex {
    pub fn new(on_event: EventFn) -> Arc<Self> {
        Arc::new(Self { exe: Mutex::new(None), editor_exe: Mutex::new(None), thread: Mutex::new(None), child: Mutex::new(None), stopped: AtomicBool::new(false), on_event, on_turn_finished: Mutex::new(None) })
    }

    /// Finds Codex. The copy bundled with an editor extension only counts when allow_editor is on.
    pub fn detect(&self, allow_editor: bool) {
        let editor = find_editor_codex();
        *self.exe.lock().unwrap() = find_codex_cli().or(if allow_editor { editor.clone() } else { None });
        *self.editor_exe.lock().unwrap() = editor;
    }

    pub fn found(&self) -> bool {
        self.exe.lock().unwrap().is_some()
    }

    fn emit(&self, v: Value) {
        (self.on_event)(v)
    }

    pub fn send(self: &Arc<Self>, prompt: String, mode: String) {
        let Some(exe) = self.exe.lock().unwrap().clone() else {
            self.emit(json!({ "kind": "error", "text": "Codex CLI not found." }));
            self.emit(json!({ "kind": "end" }));
            return;
        };
        if self.child.lock().unwrap().is_some() {
            return;
        }
        let model = pick_model();
        let thread = self.thread.lock().unwrap().clone();
        let mut c = cli_cmd(&exe);
        match &thread {
            Some(t) => c.args(["exec", "resume", t]),
            None => c.arg("exec"),
        };
        c.args(["--json", "--skip-git-repo-check", "-c", &format!("sandbox_mode={}", if mode == "plan" { "read-only" } else { "workspace-write" })]);
        if let Some(m) = &model {
            c.args(["-m", m]);
        }
        c.arg("-").current_dir(home()).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        #[cfg(unix)]
        c.process_group(0);
        let mut child = match c.spawn() {
            Ok(ch) => ch,
            Err(e) => {
                self.emit(json!({ "kind": "error", "text": e.to_string() }));
                self.emit(json!({ "kind": "end" }));
                return;
            }
        };
        *self.child.lock().unwrap() = child.id();
        self.emit(json!({ "kind": "start" }));
        let input = if thread.is_some() { prompt } else { preamble() + &prompt };
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            let started = now_ms();
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(input.as_bytes()).await;
            }
            let mut stderr = child.stderr.take().unwrap();
            let err_task = tauri::async_runtime::spawn(async move {
                let mut s = String::new();
                let _ = stderr.read_to_string(&mut s).await;
                s
            });
            let mut lines = tokio::io::BufReader::new(child.stdout.take().unwrap()).lines();
            let (mut last, mut failed) = (String::new(), false);
            while let Ok(Some(line)) = lines.next_line().await {
                let Ok(m) = serde_json::from_str::<Value>(line.trim()) else { continue };
                match m["type"].as_str().unwrap_or("") {
                    "thread.started" => {
                        if let Some(t) = m["thread_id"].as_str() {
                            *me.thread.lock().unwrap() = Some(t.into());
                        }
                        me.emit(json!({ "kind": "init", "model": model.as_ref().map(|m| format!("Codex · {m}")).unwrap_or("Codex".into()) }));
                    }
                    t @ ("item.started" | "item.completed") => me.item(t == "item.completed", &m["item"], &mut last),
                    "turn.completed" => me.emit(json!({ "kind": "result", "error": false, "text": last, "ms": now_ms() - started })),
                    "turn.failed" | "error" => {
                        if !failed {
                            failed = true;
                            let raw = m["error"]["message"].as_str().or(m["message"].as_str()).unwrap_or("");
                            me.emit(json!({ "kind": "error", "text": error_text(raw) }));
                        }
                    }
                    _ => {}
                }
            }
            let code = child.wait().await.ok().and_then(|s| s.code());
            let stderr = err_task.await.unwrap_or_default();
            *me.child.lock().unwrap() = None;
            if code != Some(0) && !failed && !me.stopped.load(Ordering::SeqCst) {
                let t = stderr.trim().lines().last().map(String::from).unwrap_or_else(|| format!("Codex exited with code {}", code.map(|c| c.to_string()).unwrap_or("?".into())));
                me.emit(json!({ "kind": "error", "text": t.chars().take(400).collect::<String>() }));
            }
            me.stopped.store(false, Ordering::SeqCst);
            me.emit(json!({ "kind": "end" }));
            let cb = me.on_turn_finished.lock().unwrap().clone();
            if let Some(cb) = cb {
                cb();
            }
        });
    }

    fn item(&self, done: bool, it: &Value, last: &mut String) {
        let id = &it["id"];
        let failed = it["status"] == "failed";
        match it["type"].as_str().unwrap_or("") {
            "agent_message" if done => {
                *last = it["text"].as_str().unwrap_or("").to_string();
                self.emit(json!({ "kind": "segment" }));
                self.emit(json!({ "kind": "delta", "text": last.clone() }));
            }
            "command_execution" => {
                if !done {
                    let cmd = it["command"].as_str().unwrap_or("").split_whitespace().collect::<Vec<_>>().join(" ");
                    self.emit(json!({ "kind": "tool", "id": id, "name": "Shell", "detail": cmd.chars().take(140).collect::<String>() }));
                } else {
                    let err = it["exit_code"].as_i64().map(|c| c != 0).unwrap_or(false);
                    self.emit(json!({ "kind": "tool_result", "id": id, "error": err }));
                }
            }
            "file_change" => {
                if !done {
                    let files: Vec<String> = it["changes"].as_array().into_iter().flatten().map(|c| Path::new(c["path"].as_str().unwrap_or("")).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default()).collect();
                    self.emit(json!({ "kind": "tool", "id": id, "name": "Edit", "detail": files.join(", ") }));
                } else {
                    self.emit(json!({ "kind": "tool_result", "id": id, "error": failed }));
                }
            }
            t @ ("mcp_tool_call" | "web_search") => {
                if !done {
                    let name = if t == "web_search" { "Web search".to_string() } else { it["tool"].as_str().unwrap_or("Tool").to_string() };
                    let detail = it["query"].as_str().or(it["server"].as_str()).unwrap_or("");
                    self.emit(json!({ "kind": "tool", "id": id, "name": name, "detail": detail }));
                } else {
                    self.emit(json!({ "kind": "tool_result", "id": id, "error": failed }));
                }
            }
            _ => {}
        }
    }

    pub fn stop(&self) {
        if let Some(pid) = *self.child.lock().unwrap() {
            self.stopped.store(true, Ordering::SeqCst);
            stop_tree(pid);
        }
    }

    pub fn reset(&self) {
        self.stop();
        *self.thread.lock().unwrap() = None;
    }

    /// Plan limits: app-server first (no model call, no usage), else the newest session log.
    pub async fn read_limits(&self) -> Option<Value> {
        let exe = self.exe.lock().unwrap().clone()?;
        if let Some(v) = limits_from_app_server(&exe).await {
            return Some(v);
        }
        tokio::task::spawn_blocking(limits_from_sessions).await.ok().flatten()
    }
}

async fn limits_from_app_server(exe: &Path) -> Option<Value> {
    let mut c = cli_cmd(exe);
    c.arg("app-server").current_dir(std::env::temp_dir()).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).kill_on_drop(true);
    let mut child = c.spawn().ok()?;
    let mut stdin = child.stdin.take()?;
    let mut lines = tokio::io::BufReader::new(child.stdout.take()?).lines();
    let init = json!({ "id": 1, "method": "initialize", "params": { "clientInfo": { "name": "mission-control", "title": "Mission Control", "version": "1.0.0" } } });
    stdin.write_all(format!("{init}\n").as_bytes()).await.ok()?;
    let work = async {
        while let Ok(Some(line)) = lines.next_line().await {
            let Ok(m) = serde_json::from_str::<Value>(&line) else { continue };
            if m["id"] == 1 {
                let _ = stdin.write_all(b"{\"method\":\"initialized\"}\n").await;
                let _ = stdin.write_all(b"{\"id\":2,\"method\":\"account/rateLimits/read\"}\n").await;
            } else if m["id"] == 2 {
                // An error here (older Codex without this method) falls back to the session logs
                let rl = if m["result"]["rateLimits"].is_object() { &m["result"]["rateLimits"] } else { &m["result"]["rateLimitsByLimitId"]["codex"] };
                if !rl.is_object() {
                    return None;
                }
                let windows = normalize_windows(&[&rl["primary"], &rl["secondary"]], now_ms());
                return if windows.is_empty() { None } else { Some(json!({ "windows": windows, "plan": rl["planType"], "at": now_ms(), "source": "app-server" })) };
            }
        }
        None
    };
    let r = tokio::time::timeout(std::time::Duration::from_secs(20), work).await.ok().flatten();
    let _ = child.kill().await;
    r
}
