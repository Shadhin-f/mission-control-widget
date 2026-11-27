// Bridge to the OpenAI Codex CLI (uses your existing Codex / ChatGPT sign-in).
// Chat: `codex exec --json` (and `codex exec resume <thread>` for follow-ups).
// Limits: `codex app-server` JSON-RPC `account/rateLimits/read`, which costs no usage.
const { spawn, execFile } = require('child_process');
const { EventEmitter } = require('events');
const fs = require('fs');
const os = require('os');
const path = require('path');

const PREAMBLE = [
  "[Context: you are the assistant inside the user's \"Mission Control\" desktop widget on their Windows 11 PC.",
  'The user controls the PC through you: opening apps and folders, managing files, checking and changing system state. Use PowerShell.',
  'The chat window is small: answer in a few short sentences.',
  'Before anything destructive or hard to undo, say exactly what you will do and wait for the user to confirm.]',
  '',
  '',
].join('\n');

const verKey = (name) => (name.match(/(\d+)\.(\d+)\.(\d+)/) || [0, 0, 0, 0]).slice(1).map(Number);

function findCodex() {
  for (const dir of (process.env.PATH || '').split(';')) {
    if (!dir) continue;
    for (const n of ['codex.exe', 'codex.cmd']) { const p = path.join(dir, n); if (fs.existsSync(p)) return p; }
  }
  const npm = path.join(process.env.APPDATA || '', 'npm', 'codex.cmd');
  if (fs.existsSync(npm)) return npm;
  // The OpenAI editor extension ships the Codex CLI
  for (const editor of ['.vscode', '.vscode-insiders', '.cursor', '.windsurf']) {
    const root = path.join(os.homedir(), editor, 'extensions');
    let dirs = [];
    try { dirs = fs.readdirSync(root).filter((d) => /^openai\.chatgpt-/.test(d)); } catch { continue; }
    dirs.sort((a, b) => { const x = verKey(a), y = verKey(b); for (let i = 0; i < 3; i++) if (x[i] !== y[i]) return y[i] - x[i]; return 0; });
    for (const d of dirs) { const p = path.join(root, d, 'bin', 'windows-x86_64', 'codex.exe'); if (fs.existsSync(p)) return p; }
  }
  return null;
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

function errorText(raw) {
  let s = String(raw || '');
  for (let i = 0; i < 2; i++) { try { const j = JSON.parse(s); s = j.error?.message || j.message || s; } catch { break; } }
  return s;
}

class CodexBridge extends EventEmitter {
  constructor() {
    super();
    this.exe = findCodex();
    this.threadId = null;
    this.child = null;
    this.model = null;
  }

  get busy() { return !!this.child; }

  send(prompt, mode = 'auto') {
    if (!this.exe) this.exe = findCodex();
    if (!this.exe) { this.emit('event', { kind: 'error', text: 'Codex CLI not found.' }); return; }
    if (this.child) return;
    this.model = pickModel();
    const args = this.threadId ? ['exec', 'resume', this.threadId] : ['exec'];
    args.push('--json', '--skip-git-repo-check', '-c', `sandbox_mode=${mode === 'plan' ? 'read-only' : 'workspace-write'}`);
    if (this.model) args.push('-m', this.model);
    args.push('-');
    const child = spawn(this.exe, args, { cwd: os.homedir(), windowsHide: true, shell: this.exe.endsWith('.cmd') });
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
    execFile('taskkill', ['/PID', String(this.child.pid), '/T', '/F'], { windowsHide: true }, () => {});
  }

  reset() { this.stop(); this.threadId = null; }

  // Reads plan limits through the app-server protocol (no model call, no usage).
  readLimits() {
    return new Promise((resolve) => {
      if (!this.exe) this.exe = findCodex();
      if (!this.exe) return resolve(null);
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
            const rl = m.result?.rateLimits;
            if (!rl) return finish(null);
            const windows = [rl.primary, rl.secondary].filter(Boolean).map((w) => ({
              label: windowLabel(w.windowDurationMins), pct: Number(w.usedPercent) || 0, resetsAt: (w.resetsAt || 0) * 1000,
            }));
            finish({ windows, plan: rl.planType || null, at: Date.now() });
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
