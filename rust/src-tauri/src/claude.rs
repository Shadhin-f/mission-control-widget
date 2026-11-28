// Bridge to the locally installed Claude Code CLI (uses your existing Claude Code login).
// Each message runs `claude -p` with stream-json output and resumes the same session.
use crate::util::{cli_cmd, home, now_ms, stop_tree};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};

fn system_prompt() -> String {
    let mac = cfg!(target_os = "macos");
    [
        format!("You are the assistant inside the user's \"Mission Control\" desktop widget on their {}.", if mac { "macOS Mac" } else { "Windows 11 PC" }),
        "The user controls the PC through you: opening apps and folders, managing files, checking and changing system state.".into(),
        if mac { "Use macOS shell commands and AppleScript for system actions. Open apps with open -a." } else { "Use PowerShell for system actions. Start GUI apps detached (Start-Process) so you do not wait on them." }.into(),
        "The chat window is small: answer in a few short sentences or a short list, no wide tables.".into(),
        "Before anything destructive or hard to undo (deleting files, uninstalling, killing important processes, changing security or network settings), say exactly what you will do and wait for the user to confirm in chat.".into(),
    ]
    .join(" ")
}

/// Native installer (~/.local/bin), anything on PATH (WinGet, Scoop, custom), then the npm shim.
pub fn find_claude() -> Option<PathBuf> {
    #[cfg(unix)]
    {
        crate::util::find_unix_cli("claude")
    }
    #[cfg(windows)]
    {
        let path = crate::util::env_path_dirs();
        let mut c = vec![home().join(".local/bin/claude.exe")];
        c.extend(path.iter().map(|d| d.join("claude.exe")));
        if let Some(l) = dirs::data_local_dir() {
            c.push(l.join("Microsoft/WinGet/Links/claude.exe"));
        }
        c.extend(path.iter().map(|d| d.join("claude.cmd")));
        if let Some(r) = dirs::data_dir() {
            c.push(r.join("npm/claude.cmd"));
        }
        c.into_iter().find(|p| p.is_file())
    }
}

pub fn summarize_input(input: &Value) -> String {
    let v = ["command", "file_path", "path", "pattern", "url", "query", "description"]
        .iter()
        .find_map(|k| input.get(*k).and_then(|v| v.as_str()).filter(|s| !s.is_empty()))
        .unwrap_or("");
    let s = v.split_whitespace().collect::<Vec<_>>().join(" ");
    s.chars().take(140).collect()
}

/// Normalises a stream-json rate_limit_event into { fiveHour, sevenDay } with 0..100 utilisation.
pub fn parse_limits(m: &Value) -> Option<Value> {
    let info = m.get("rate_limit_info").cloned().unwrap_or(json!({}));
    let w = info.get("unifiedWindows").cloned().unwrap_or(json!({}));
    let pick = |x: Option<&Value>| -> Value {
        match x.and_then(|x| Some((x.get("utilization")?.as_f64()?, x.get("resetsAt").and_then(|r| r.as_f64()).unwrap_or(0.0)))) {
            Some((u, r)) => json!({ "pct": u * 100.0, "resetsAt": r * 1000.0 }),
            None => Value::Null,
        }
    };
    let mut five = pick(w.get("five_hour"));
    let mut seven = pick(w.get("seven_day"));
    if five.is_null() && seven.is_null() {
        if let Some(u) = info.get("utilization").and_then(|u| u.as_f64()) {
            let v = json!({ "pct": u * 100.0, "resetsAt": info.get("resetsAt").and_then(|r| r.as_f64()).unwrap_or(0.0) * 1000.0 });
            match info.get("rateLimitType").and_then(|t| t.as_str()) {
                Some("five_hour") => five = v,
                Some("seven_day") => seven = v,
                _ => {}
            }
        }
    }
    if five.is_null() && seven.is_null() {
        return None;
    }
    Some(json!({ "fiveHour": five, "sevenDay": seven, "status": info.get("status"), "at": now_ms() }))
}

pub type EventFn = Arc<dyn Fn(Value) + Send + Sync>;
pub type LimitsFn = Arc<dyn Fn(Option<Value>) + Send + Sync>;

pub struct Claude {
    pub exe: Mutex<Option<PathBuf>>,
    session: Mutex<Option<String>>,
    child: Mutex<Option<u32>>,
    stopped: AtomicBool,
    probing: AtomicBool,
    on_event: EventFn,
    on_limits: LimitsFn,
    /// (model, effort) for this chat instead of Claude Code's defaults; cleared by `reset`
    pub model_override: Mutex<Option<(String, String)>>,
}

impl Claude {
    pub fn new(on_event: EventFn, on_limits: LimitsFn) -> Arc<Self> {
        Arc::new(Self { exe: Mutex::new(find_claude()), session: Mutex::new(None), child: Mutex::new(None), stopped: AtomicBool::new(false), probing: AtomicBool::new(false), on_event, on_limits, model_override: Mutex::new(None) })
    }

    pub fn found(&self) -> bool {
        self.exe.lock().unwrap().is_some()
    }
    pub fn busy(&self) -> bool {
        self.child.lock().unwrap().is_some()
    }

    fn emit(&self, v: Value) {
        (self.on_event)(v)
    }

    pub fn send(self: &Arc<Self>, prompt: String, mode: String) {
        if self.exe.lock().unwrap().is_none() {
            *self.exe.lock().unwrap() = find_claude();
        }
        let Some(exe) = self.exe.lock().unwrap().clone() else {
            self.emit(json!({ "kind": "error", "text": "Claude Code CLI not found. Install it from claude.com/claude-code and sign in once in a terminal." }));
            return;
        };
        if self.busy() {
            return;
        }
        let mut c = cli_cmd(&exe);
        c.args(["-p", "--output-format", "stream-json", "--verbose", "--include-partial-messages", "--permission-mode", &mode, "--permission-prompts", "none", "--append-system-prompt", &system_prompt()]);
        if let Some(s) = self.session.lock().unwrap().clone() {
            c.args(["--resume", &s]);
        }
        if let Some((m, e)) = self.model_override.lock().unwrap().clone() {
            c.args(["--model", &m, "--effort", &e]);
        }
        c.current_dir(home()).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
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
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(prompt.as_bytes()).await;
                drop(stdin);
            }
            let mut stderr = child.stderr.take().unwrap();
            let err_task = tauri::async_runtime::spawn(async move {
                let mut s = String::new();
                let _ = stderr.read_to_string(&mut s).await;
                s
            });
            let mut lines = tokio::io::BufReader::new(child.stdout.take().unwrap()).lines();
            let mut got_result = false;
            while let Ok(Some(line)) = lines.next_line().await {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if let Ok(m) = serde_json::from_str::<Value>(line) {
                    if m["type"] == "result" {
                        got_result = true;
                    }
                    me.handle(&m);
                }
            }
            let status = child.wait().await.ok();
            let stderr = err_task.await.unwrap_or_default();
            *me.child.lock().unwrap() = None;
            let code = status.and_then(|s| s.code());
            if !got_result && code != Some(0) && !me.stopped.load(Ordering::SeqCst) {
                let t = if stderr.trim().is_empty() { format!("Claude exited with code {}", code.map(|c| c.to_string()).unwrap_or("?".into())) } else { stderr.trim().to_string() };
                me.emit(json!({ "kind": "error", "text": t.chars().take(600).collect::<String>() }));
            }
            me.stopped.store(false, Ordering::SeqCst);
            me.emit(json!({ "kind": "end" }));
        });
    }

    /// Reads current plan limits with the smallest possible request (Haiku, no tools, tiny prompt).
    pub fn probe_limits(self: &Arc<Self>) {
        if self.probing.swap(true, Ordering::SeqCst) {
            return;
        }
        if self.exe.lock().unwrap().is_none() {
            *self.exe.lock().unwrap() = find_claude();
        }
        let Some(exe) = self.exe.lock().unwrap().clone() else {
            self.probing.store(false, Ordering::SeqCst);
            return;
        };
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            let mut c = cli_cmd(&exe);
            c.args(["-p", "--model", "haiku", "--effort", "low", "--tools", "", "--system-prompt", "Reply with OK.", "--strict-mcp-config", "--no-session-persistence", "--disable-slash-commands", "--output-format", "stream-json", "--verbose"])
                .current_dir(std::env::temp_dir())
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .kill_on_drop(true);
            let mut got = false;
            if let Ok(mut child) = c.spawn() {
                if let Some(mut stdin) = child.stdin.take() {
                    let _ = stdin.write_all(b"ok").await;
                }
                let mut lines = tokio::io::BufReader::new(child.stdout.take().unwrap()).lines();
                let read = async {
                    while let Ok(Some(line)) = lines.next_line().await {
                        if !line.contains("rate_limit_event") {
                            continue;
                        }
                        if let Some(l) = serde_json::from_str::<Value>(&line).ok().and_then(|m| parse_limits(&m)) {
                            got = true;
                            (me.on_limits)(Some(l));
                        }
                    }
                };
                let _ = tokio::time::timeout(std::time::Duration::from_secs(30), read).await;
                let _ = child.kill().await;
            }
            me.probing.store(false, Ordering::SeqCst);
            if !got {
                (me.on_limits)(None);
            }
        });
    }

    fn handle(&self, m: &Value) {
        let t = m["type"].as_str().unwrap_or("");
        match t {
            "rate_limit_event" => {
                if let Some(l) = parse_limits(m) {
                    (self.on_limits)(Some(l));
                }
            }
            "system" if m["subtype"] == "init" => {
                if let Some(s) = m["session_id"].as_str() {
                    *self.session.lock().unwrap() = Some(s.into());
                }
                self.emit(json!({ "kind": "init", "model": m["model"] }));
            }
            "stream_event" => {
                let e = &m["event"];
                if e["type"] == "content_block_delta" && e["delta"]["type"] == "text_delta" {
                    self.emit(json!({ "kind": "delta", "text": e["delta"]["text"] }));
                }
                if e["type"] == "message_start" {
                    self.emit(json!({ "kind": "segment" }));
                }
            }
            "assistant" => {
                for b in m["message"]["content"].as_array().into_iter().flatten() {
                    if b["type"] == "tool_use" {
                        self.emit(json!({ "kind": "tool", "id": b["id"], "name": b["name"], "detail": summarize_input(&b["input"]) }));
                    }
                }
            }
            "user" => {
                for b in m["message"]["content"].as_array().into_iter().flatten() {
                    if b["type"] == "tool_result" {
                        self.emit(json!({ "kind": "tool_result", "id": b["tool_use_id"], "error": b["is_error"].as_bool().unwrap_or(false) }));
                    }
                }
            }
            "result" => {
                if let Some(s) = m["session_id"].as_str() {
                    *self.session.lock().unwrap() = Some(s.into());
                }
                let denied: Vec<String> = m["permission_denials"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|d| format!("{}: {}", d["tool_name"].as_str().unwrap_or(""), summarize_input(&d["tool_input"])))
                    .collect();
                self.emit(json!({
                    "kind": "result", "error": m["is_error"].as_bool().unwrap_or(false), "text": m["result"].as_str().unwrap_or(""),
                    "cost": m["total_cost_usd"], "ms": m["duration_ms"], "denied": denied,
                }));
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
        *self.session.lock().unwrap() = None;
        *self.model_override.lock().unwrap() = None;
    }

    /// One quick answer from Haiku with no tools and no saved session (it can only read the prompt).
    /// Separate from the chat, so it never interrupts a conversation. None on any failure or timeout.
    #[allow(dead_code)] // used by the native edition's health checks
    pub async fn quick(&self, system: &str, prompt: &str, timeout_s: u64) -> Option<String> {
        let exe = self.exe.lock().unwrap().clone().or_else(find_claude)?;
        let mut c = cli_cmd(&exe);
        c.args(["-p", "--model", "haiku", "--effort", "low", "--tools", "", "--system-prompt", system, "--strict-mcp-config", "--no-session-persistence", "--disable-slash-commands", "--output-format", "json"])
            .current_dir(std::env::temp_dir())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let mut child = c.spawn().ok()?;
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(prompt.as_bytes()).await;
        }
        let mut out = String::new();
        let mut so = child.stdout.take()?;
        let read = tokio::time::timeout(std::time::Duration::from_secs(timeout_s), so.read_to_string(&mut out)).await;
        let _ = child.kill().await;
        read.ok()?.ok()?;
        let v: Value = serde_json::from_str(out.trim()).ok()?;
        if v["is_error"].as_bool().unwrap_or(false) {
            return None;
        }
        v["result"].as_str().map(String::from)
    }
}
