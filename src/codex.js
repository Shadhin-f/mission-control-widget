const { findUnixCli, stopTree } = require('./cli-platform');
// Bridge to the OpenAI Codex CLI (uses your existing Codex / ChatGPT sign-in).
// Chat: `codex exec --json` (and `codex exec resume <thread>` for follow-ups).
// Limits: `codex app-server` JSON-RPC `account/rateLimits/read`, which costs no usage.
const { spawn, execFile } = require('child_process');
const { EventEmitter } = require('events');
const fs = require('fs');
const os = require('os');
const path = require('path');

const PREAMBLE = [
  `[Context: you are the assistant inside the user's \"Mission Control\" desktop widget on their ${process.platform === 'darwin' ? 'macOS Mac' : 'Windows 11 PC'}.`,
  process.platform === 'darwin' ? 'Use macOS shell commands and AppleScript for system actions. Open apps with open -a.' : 'The user controls the PC through you: opening apps and folders, managing files, checking and changing system state. Use PowerShell.',
  'The chat window is small: answer in a few short sentences.',
  'Before anything destructive or hard to undo, say exactly what you will do and wait for the user to confirm.]',
  '',
  '',
].join('\n');

const verKey = (name) => (name.match(/(\d+)\.(\d+)\.(\d+)/) || [0, 0, 0, 0]).slice(1).map(Number);

// Bounded search for codex.exe inside an npm package folder (layout differs between Codex releases)
function findExeIn(dir, depth = 5) {
  let entries;
  try { entries = fs.readdirSync(dir, { withFileTypes: true }); } catch { return null; }
  for (const e of entries) if (e.isFile() && e.name.toLowerCase() === 'codex.exe') return path.join(dir, e.name);
  if (depth <= 0) return null;
  for (const e of entries) if (e.isDirectory()) { const hit = findExeIn(path.join(dir, e.name), depth - 1); if (hit) return hit; }
  return null;
}

// A real Codex CLI install: PATH (npm, WinGet, Scoop, standalone installer), or npm's global folder.
function findCodexCli() {
  if (process.platform !== 'win32') return findUnixCli('codex');
  for (const dir of (process.env.PATH || '').split(';')) {
    if (!dir) continue;
    const exe = path.join(dir, 'codex.exe');
    if (fs.existsSync(exe)) return exe;
  }
  const appData = process.env.APPDATA || '';
  const local = process.env.LOCALAPPDATA || '';
  for (const p of [
    path.join(os.homedir(), '.local', 'bin', 'codex.exe'),
    path.join(local, 'Microsoft', 'WinGet', 'Links', 'codex.exe'),
    path.join(local, 'Programs', 'codex', 'codex.exe'),
    path.join(os.homedir(), 'scoop', 'shims', 'codex.exe'),
  ]) if (fs.existsSync(p)) return p;
  // npm i -g @openai/codex: prefer the native binary over the .cmd shim
  for (const pkg of ['codex', 'codex-win32-x64']) {
    const hit = findExeIn(path.join(appData, 'npm', 'node_modules', '@openai', pkg));
    if (hit) return hit;
  }
  for (const dir of [...(process.env.PATH || '').split(';'), path.join(appData, 'npm')]) {
    if (!dir) continue;
    const shim = path.join(dir, 'codex.cmd');
    if (fs.existsSync(shim)) return shim;
  }
  return null;
}

// The OpenAI extension for VS Code / Cursor / Windsurf bundles a Codex CLI. Only used when the user opts in.
function findEditorCodex() {
  for (const editor of ['.vscode', '.vscode-insiders', '.cursor', '.windsurf']) {
    const root = path.join(os.homedir(), editor, 'extensions');
    let dirs = [];
    try { dirs = fs.readdirSync(root).filter((d) => /^openai\.chatgpt-/.test(d)); } catch { continue; }
    dirs.sort((a, b) => { const x = verKey(a), y = verKey(b); for (let i = 0; i < 3; i++) if (x[i] !== y[i]) return y[i] - x[i]; return 0; });
    for (const d of dirs) { const p = path.join(root, d, 'bin', process.platform === 'darwin' ? `darwin-${process.arch === 'arm64' ? 'aarch64' : 'x86_64'}` : 'windows-x86_64', process.platform === 'darwin' ? 'codex' : 'codex.exe'); if (fs.existsSync(p)) return p; }
  }
  return null;
}

function findCodex({ allowEditor = false } = {}) {
  return findCodexCli() || (allowEditor ? findEditorCodex() : null);
}

// Use the model from ~/.codex/config.toml when the account can use it; otherwise the first model Codex lists.
function pickModel() {
  const home = path.join(os.homedir(), '.codex');
  let configured = null, listed = [];
  try { configured = (fs.readFileSync(path.join(home, 'config.toml'), 'utf8').match(/^\s*model\s*=\s*"([^"]+)"/m) || [])[1] || null; } catch {}
  try { listed = (JSON.parse(fs.readFileSync(path.join(home, 'models_cache.json'), 'utf8')).models || []).filter((m) => m.visibility === 'list').map((m) => m.slug); } catch {}
  if (!listed.length || (configured && listed.includes(configured))) return null; // let Codex use its own setting
  return listed[0];
}

function windowLabel(mins) {
  if (!mins) return '?';
  if (mins % 1440 === 0) return `${mins / 1440}d`;
  if (mins % 60 === 0) return `${mins / 60}h`;
  return `${mins}m`;
}

// Normalises Codex rate-limit windows (app-server camelCase or session-log snake_case) to
// [{ label, pct, resetsAt(ms), mins }], shortest window first: paid plans give 5h then 7d (weekly).
function normalizeWindows(list, baseMs = Date.now()) {
  return list.filter(Boolean).map((w) => {
    const mins = w.windowDurationMins ?? w.window_minutes ?? null;
    const pct = Number(w.usedPercent ?? w.used_percent ?? 0) || 0;
    let resetsAt = w.resetsAt ?? w.resets_at ?? null;
    if (resetsAt) resetsAt *= 1000;
    else if (w.resets_in_seconds != null) resetsAt = baseMs + w.resets_in_seconds * 1000;
    return { label: windowLabel(mins), pct, resetsAt: resetsAt || 0, mins: mins || 0 };
  }).sort((a, b) => a.mins - b.mins);
}

// Fallback for Codex versions without app-server rate limits: the newest `token_count` event
// in ~/.codex/sessions carries the same windows. Only that event's rate_limits field is read.
function limitsFromSessions() {
  const root = path.join(os.homedir(), '.codex', 'sessions');
  const files = [];
  const now = new Date();
  for (let d = 0; d < 8 && files.length < 6; d++) {
    const day = new Date(now - d * 86400000);
    const dir = path.join(root, String(day.getFullYear()), String(day.getMonth() + 1).padStart(2, '0'), String(day.getDate()).padStart(2, '0'));
    let names = [];
    try { names = fs.readdirSync(dir).filter((n) => n.endsWith('.jsonl')); } catch { continue; }
    for (const n of names) { try { files.push({ p: path.join(dir, n), t: fs.statSync(path.join(dir, n)).mtimeMs }); } catch {} }
  }
  files.sort((a, b) => b.t - a.t);
  for (const f of files.slice(0, 6)) {
    let text;
    try {
      const fd = fs.openSync(f.p, 'r');
      const size = fs.fstatSync(fd).size, len = Math.min(size, 512 * 1024);
      const buf = Buffer.alloc(len);
      fs.readSync(fd, buf, 0, len, size - len);
      fs.closeSync(fd);
      text = buf.toString('utf8');
    } catch { continue; }
    const lines = text.split('\n').filter((l) => l.includes('"rate_limits"'));
    for (let i = lines.length - 1; i >= 0; i--) {
      try {
        const m = JSON.parse(lines[i]);
        const rl = m.payload?.rate_limits || m.rate_limits;
        if (!rl) continue;
        const base = m.timestamp ? Date.parse(m.timestamp) : f.t;
        const windows = normalizeWindows([rl.primary, rl.secondary], base);
        if (windows.length) return { windows, plan: rl.plan_type || null, at: base, source: 'sessions' };
      } catch {}
    }
  }
  return null;
}

function errorText(raw) {
  let s = String(raw || '');
  for (let i = 0; i < 2; i++) { try { const j = JSON.parse(s); s = j.error?.message || j.message || s; } catch { break; } }
  return s;
}

class CodexBridge extends EventEmitter {
  constructor() {
    super();
    this.exe = null;
    this.editorExe = null;
    this.threadId = null;
    this.child = null;
    this.model = null;
  }

  // Finds Codex. The copy bundled with an editor extension only counts when allowEditor is on.
  detect(allowEditor = false) {
    this.editorExe = findEditorCodex();
    this.exe = findCodex({ allowEditor });
    return this.exe;
  }

  get busy() { return !!this.child; }

  send(prompt, mode = 'auto') {
    if (!this.exe) { this.emit('event', { kind: 'error', text: 'Codex CLI not found.' }); this.emit('event', { kind: 'end' }); return; }
    if (this.child) return;
    this.model = pickModel();
    const args = this.threadId ? ['exec', 'resume', this.threadId] : ['exec'];
    args.push('--json', '--skip-git-repo-check', '-c', `sandbox_mode=${mode === 'plan' ? 'read-only' : 'workspace-write'}`);
    if (this.model) args.push('-m', this.model);
    args.push('-');
    const child = spawn(this.exe, args, { cwd: os.homedir(), detached: process.platform !== 'win32', windowsHide: true, shell: this.exe.endsWith('.cmd') });
    this.child = child;
    this.emit('event', { kind: 'start' });
    const started = Date.now();
    let buf = '', stderr = '', last = '', failed = false;
    child.stdout.setEncoding('utf8');
    child.stdout.on('data', (d) => {
      buf += d;
      let i;
      while ((i = buf.indexOf('\n')) >= 0) {
        const line = buf.slice(0, i).trim(); buf = buf.slice(i + 1);
        if (!line) continue;
        let m; try { m = JSON.parse(line); } catch { continue; }
        if (m.type === 'thread.started') { this.threadId = m.thread_id || this.threadId; this.emit('event', { kind: 'init', model: this.model ? `Codex · ${this.model}` : 'Codex' }); }
        else if (m.type === 'item.started' || m.type === 'item.completed') this.item(m.type === 'item.completed', m.item || {}, (t) => { last = t; });
        else if (m.type === 'turn.completed') this.emit('event', { kind: 'result', error: false, text: last, ms: Date.now() - started });
        else if (m.type === 'turn.failed' || m.type === 'error') {
          if (failed) continue;
          failed = true;
          this.emit('event', { kind: 'error', text: errorText(m.error?.message || m.message) });
        }
      }
    });
    child.stderr.on('data', (d) => { stderr += d; });
    child.on('error', (e) => this.emit('event', { kind: 'error', text: e.message }));
    child.on('close', (code) => {
      this.child = null;
      if (code !== 0 && !failed && !this.stopped) this.emit('event', { kind: 'error', text: (stderr.trim().split('\n').pop() || `Codex exited with code ${code}`).slice(0, 400) });
      this.stopped = false;
      this.emit('event', { kind: 'end' });
      this.emit('turn-finished');
    });
    child.stdin.end(this.threadId ? prompt : PREAMBLE + prompt);
  }

  item(done, it, setLast) {
    const id = it.id;
    if (it.type === 'agent_message' && done) {
      setLast(it.text || '');
      this.emit('event', { kind: 'segment' });
      this.emit('event', { kind: 'delta', text: it.text || '' });
    } else if (it.type === 'command_execution') {
      if (!done) this.emit('event', { kind: 'tool', id, name: 'Shell', detail: String(it.command || '').replace(/\s+/g, ' ').slice(0, 140) });
      else this.emit('event', { kind: 'tool_result', id, error: it.exit_code != null && it.exit_code !== 0 });
    } else if (it.type === 'file_change') {
      const files = (it.changes || []).map((c) => path.basename(c.path || '')).join(', ');
      if (!done) this.emit('event', { kind: 'tool', id, name: 'Edit', detail: files });
      else this.emit('event', { kind: 'tool_result', id, error: it.status === 'failed' });
    } else if (it.type === 'mcp_tool_call' || it.type === 'web_search') {
      if (!done) this.emit('event', { kind: 'tool', id, name: it.type === 'web_search' ? 'Web search' : (it.tool || 'Tool'), detail: String(it.query || it.server || '') });
      else this.emit('event', { kind: 'tool_result', id, error: it.status === 'failed' });
    }
  }

  stop() {
    if (!this.child) return;
    this.stopped = true;
    stopTree(this.child);
  }

  reset() { this.stop(); this.threadId = null; }

  // Plan limits: app-server first (no model call, no usage), else the newest session log.
  async readLimits() {
    if (!this.exe) return null;
    return (await this.limitsFromAppServer()) || limitsFromSessions();
  }

  limitsFromAppServer() {
    return new Promise((resolve) => {
      const p = spawn(this.exe, ['app-server'], { cwd: os.tmpdir(), windowsHide: true, shell: this.exe.endsWith('.cmd') });
      let buf = '', done = false;
      const finish = (v) => { if (done) return; done = true; clearTimeout(timer); try { p.kill(); } catch {} resolve(v); };
      const timer = setTimeout(() => finish(null), 20000);
      const send = (m) => { try { p.stdin.write(`${JSON.stringify(m)}\n`); } catch {} };
      p.stdout.setEncoding('utf8');
      p.stdout.on('data', (d) => {
        buf += d;
        let i;
        while ((i = buf.indexOf('\n')) >= 0) {
          const line = buf.slice(0, i); buf = buf.slice(i + 1);
          let m; try { m = JSON.parse(line); } catch { continue; }
          if (m.id === 1) { send({ method: 'initialized' }); send({ id: 2, method: 'account/rateLimits/read' }); }
          else if (m.id === 2) {
            // An error here (older Codex without this method) falls back to the session logs
            const rl = m.result?.rateLimits || m.result?.rateLimitsByLimitId?.codex;
            if (!rl) return finish(null);
            const windows = normalizeWindows([rl.primary, rl.secondary]);
            finish(windows.length ? { windows, plan: rl.planType || null, at: Date.now(), source: 'app-server' } : null);
          }
        }
      });
      p.on('error', () => finish(null));
      p.on('close', () => finish(null));
      send({ id: 1, method: 'initialize', params: { clientInfo: { name: 'mission-control', title: 'Mission Control', version: '1.0.0' } } });
    });
  }
}

module.exports = CodexBridge;
module.exports.normalizeWindows = normalizeWindows;
module.exports.limitsFromSessions = limitsFromSessions;
