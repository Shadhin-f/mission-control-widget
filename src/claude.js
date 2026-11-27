const { findUnixCli, stopTree } = require('./cli-platform');
// Bridge to the locally installed Claude Code CLI (uses your existing Claude Code login).
// Each message runs `claude -p` with stream-json output and resumes the same session.
const { spawn, execFile } = require('child_process');
const { EventEmitter } = require('events');
const fs = require('fs');
const os = require('os');
const path = require('path');

const SYSTEM = [
  `You are the assistant inside the user's \"Mission Control\" desktop widget on their ${process.platform === 'darwin' ? 'macOS Mac' : 'Windows 11 PC'}.`,
  'The user controls the PC through you: opening apps and folders, managing files, checking and changing system state.',
  process.platform === 'darwin' ? 'Use macOS shell commands and AppleScript for system actions. Open apps with open -a.' : 'Use PowerShell for system actions. Start GUI apps detached (Start-Process) so you do not wait on them.',
  'The chat window is small: answer in a few short sentences or a short list, no wide tables.',
  'Before anything destructive or hard to undo (deleting files, uninstalling, killing important processes, changing security or network settings), say exactly what you will do and wait for the user to confirm in chat.',
].join(' ');

// Native installer (~/.local/bin), anything on PATH (WinGet, Scoop, custom), then the npm shim.
function findClaude() {
  if (process.platform !== 'win32') return findUnixCli('claude');
  const candidates = [path.join(os.homedir(), '.local', 'bin', 'claude.exe')];
  for (const dir of (process.env.PATH || '').split(';')) if (dir) candidates.push(path.join(dir, 'claude.exe'));
  candidates.push(path.join(process.env.LOCALAPPDATA || '', 'Microsoft', 'WinGet', 'Links', 'claude.exe'));
  for (const dir of (process.env.PATH || '').split(';')) if (dir) candidates.push(path.join(dir, 'claude.cmd'));
  candidates.push(path.join(process.env.APPDATA || '', 'npm', 'claude.cmd'));
  for (const c of candidates) { try { if (fs.existsSync(c)) return c; } catch {} }
  return null;
}

function summarizeInput(name, input = {}) {
  const v = input.command || input.file_path || input.path || input.pattern || input.url || input.query || input.description || '';
  return String(v).replace(/\s+/g, ' ').slice(0, 140);
}

class ClaudeBridge extends EventEmitter {
  constructor() {
    super();
    this.exe = findClaude();
    this.sessionId = null;
    this.child = null;
  }

  get busy() { return !!this.child; }

  send(prompt, mode = 'auto') {
    if (!this.exe) this.exe = findClaude();
    if (!this.exe) { this.emit('event', { kind: 'error', text: 'Claude Code CLI not found. Install it from claude.com/claude-code and sign in once in a terminal.' }); return; }
    if (this.child) return;
    const args = ['-p', '--output-format', 'stream-json', '--verbose', '--include-partial-messages',
      '--permission-mode', mode, '--permission-prompts', 'none', '--append-system-prompt', SYSTEM];
    if (this.sessionId) args.push('--resume', this.sessionId);
    const isCmd = this.exe.endsWith('.cmd');
    const child = spawn(this.exe, args, { cwd: os.homedir(), detached: process.platform !== 'win32', windowsHide: true, shell: isCmd, env: { ...process.env } });
    this.child = child;
    this.emit('event', { kind: 'start' });
    let buf = '', stderr = '', gotResult = false;
    child.stdout.setEncoding('utf8');
    child.stdout.on('data', (d) => {
      buf += d;
      let i;
      while ((i = buf.indexOf('\n')) >= 0) {
        const line = buf.slice(0, i).trim(); buf = buf.slice(i + 1);
        if (!line) continue;
        try { const m = JSON.parse(line); if (m.type === 'result') gotResult = true; this.handle(m); } catch {}
      }
    });
    child.stderr.on('data', (d) => { stderr += d; });
    child.on('error', (e) => this.emit('event', { kind: 'error', text: e.message }));
    child.on('close', (code) => {
      this.child = null;
      if (!gotResult && code !== 0 && !this.stopped) this.emit('event', { kind: 'error', text: (stderr.trim() || `Claude exited with code ${code}`).slice(0, 600) });
      this.stopped = false;
      this.emit('event', { kind: 'end' });
    });
    child.stdin.end(prompt);
  }

  // Normalises a stream-json rate_limit_event into { fiveHour, sevenDay } with 0..100 utilisation.
  static parseLimits(m) {
    const info = m.rate_limit_info || {};
    const w = info.unifiedWindows || {};
    const pick = (x) => (x && typeof x.utilization === 'number' ? { pct: x.utilization * 100, resetsAt: (x.resetsAt || 0) * 1000 } : null);
    const out = { fiveHour: pick(w.five_hour), sevenDay: pick(w.seven_day), status: info.status, at: Date.now() };
    if (!out.fiveHour && !out.sevenDay && typeof info.utilization === 'number') {
      const v = { pct: info.utilization * 100, resetsAt: (info.resetsAt || 0) * 1000 };
      if (info.rateLimitType === 'five_hour') out.fiveHour = v; else if (info.rateLimitType === 'seven_day') out.sevenDay = v;
    }
    return out.fiveHour || out.sevenDay ? out : null;
  }

  // Reads current plan limits with the smallest possible request (Haiku, no tools, tiny prompt).
  probeLimits() {
    if (this.probing) return;
    if (!this.exe) this.exe = findClaude();
    if (!this.exe) return;
    this.probing = true;
    const args = ['-p', '--model', 'haiku', '--effort', 'low', '--tools', '', '--system-prompt', 'Reply with OK.',
      '--strict-mcp-config', '--no-session-persistence', '--disable-slash-commands', '--output-format', 'stream-json', '--verbose'];
    const child = spawn(this.exe, args, { cwd: os.tmpdir(), windowsHide: true, shell: this.exe.endsWith('.cmd') });
    let buf = '', got = false;
    const finish = () => { this.probing = false; if (!got) this.emit('limits', null); };
    const timer = setTimeout(() => child.kill(), 30000);
    child.stdout.setEncoding('utf8');
    child.stdout.on('data', (d) => {
      buf += d;
      let i;
      while ((i = buf.indexOf('\n')) >= 0) {
        const line = buf.slice(0, i).trim(); buf = buf.slice(i + 1);
        if (!line.includes('rate_limit_event')) continue;
        try { const l = ClaudeBridge.parseLimits(JSON.parse(line)); if (l) { got = true; this.emit('limits', l); } } catch {}
      }
    });
    child.on('error', () => { clearTimeout(timer); finish(); });
    child.on('close', () => { clearTimeout(timer); finish(); });
    child.stdin.end('ok');
  }

  handle(m) {
    if (m.type === 'rate_limit_event') {
      const l = ClaudeBridge.parseLimits(m);
      if (l) this.emit('limits', l);
      return;
    }
    if (m.type === 'system' && m.subtype === 'init') {
      this.sessionId = m.session_id || this.sessionId;
      this.emit('event', { kind: 'init', model: m.model });
    } else if (m.type === 'stream_event') {
      const e = m.event || {};
      if (e.type === 'content_block_delta' && e.delta?.type === 'text_delta') this.emit('event', { kind: 'delta', text: e.delta.text });
      if (e.type === 'message_start') this.emit('event', { kind: 'segment' });
    } else if (m.type === 'assistant') {
      for (const b of m.message?.content || []) {
        if (b.type === 'tool_use') this.emit('event', { kind: 'tool', id: b.id, name: b.name, detail: summarizeInput(b.name, b.input) });
      }
    } else if (m.type === 'user') {
      const content = m.message?.content;
      if (Array.isArray(content)) for (const b of content) {
        if (b.type === 'tool_result') this.emit('event', { kind: 'tool_result', id: b.tool_use_id, error: !!b.is_error });
      }
    } else if (m.type === 'result') {
      this.sessionId = m.session_id || this.sessionId;
      this.emit('event', {
        kind: 'result', error: !!m.is_error, text: m.result || '', cost: m.total_cost_usd, ms: m.duration_ms,
        denied: (m.permission_denials || []).map((d) => `${d.tool_name}: ${summarizeInput(d.tool_name, d.tool_input)}`),
      });
    }
  }

  stop() {
    if (!this.child) return;
    this.stopped = true;
    stopTree(this.child);
  }

  reset() { this.stop(); this.sessionId = null; }
}

module.exports = ClaudeBridge;
