// System controls: drives the long-lived PowerShell helper (sysctl.ps1) plus a few netsh / powercfg calls.
// The helper starts on first use and stops after a few idle minutes, so it costs nothing when unused.
const { spawn, execFile } = require('child_process');
const path = require('path');

const IDLE_STOP_MS = 5 * 60 * 1000;
const HELPER_CMDS = new Set(['state', 'bt', 'brightness', 'setVolume', 'stepVolume', 'setMute', 'setDefault', 'setRadio', 'media', 'setBrightness', 'setDark', 'lock', 'sleep']);

function run(cmd, args, timeout = 15000) {
  return new Promise((resolve) => {
    execFile(cmd, args, { windowsHide: true, timeout, maxBuffer: 4 << 20 }, (err, stdout, stderr) => resolve({ ok: !err, out: String(stdout || ''), err: String(stderr || err?.message || '') }));
  });
}

class SysControl {
  constructor() {
    this.proc = null;
    this.pending = new Map();
    this.nextId = 1;
    this.idleTimer = null;
  }

  start() {
    if (this.proc) return;
    const p = spawn('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-MTA', '-File', path.join(__dirname, 'sysctl.ps1')],
      { windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'] });
    this.proc = p;
    let buf = '';
    p.stdout.setEncoding('utf8');
    p.stdout.on('data', (d) => {
      buf += d;
      let i;
      while ((i = buf.indexOf('\n')) >= 0) {
        const line = buf.slice(0, i).trim(); buf = buf.slice(i + 1);
        if (!line) continue;
        let msg; try { msg = JSON.parse(line); } catch { continue; }
        const job = this.pending.get(msg.id);
        if (!job) continue;
        this.pending.delete(msg.id);
        clearTimeout(job.timer);
        if (msg.ok) job.resolve(msg.data); else job.reject(new Error(msg.error || 'Failed'));
      }
    });
    p.stderr.on('data', () => {});
    p.on('exit', () => {
      if (this.proc === p) this.proc = null;
      for (const [, job] of this.pending) { clearTimeout(job.timer); job.reject(new Error('System helper stopped')); }
      this.pending.clear();
    });
  }

  stop() {
    clearTimeout(this.idleTimer);
    if (this.proc) { try { this.proc.stdin.end(); this.proc.kill(); } catch {} this.proc = null; }
  }

  helper(cmd, args = {}) {
    if (!HELPER_CMDS.has(cmd)) return Promise.reject(new Error(`Unknown command ${cmd}`));
    this.start();
    clearTimeout(this.idleTimer);
    this.idleTimer = setTimeout(() => this.stop(), IDLE_STOP_MS);
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => { this.pending.delete(id); reject(new Error('Timed out')); }, 20000);
      this.pending.set(id, { resolve, reject, timer });
      this.proc.stdin.write(`${JSON.stringify({ id, cmd, args })}\n`);
    });
  }

  // ---- Wi-Fi (netsh) ----
  async wifi() {
    const iface = await run('netsh', ['wlan', 'show', 'interfaces']);
    if (/no wireless interface|wlansvc.*not running/i.test(iface.out + iface.err)) return { available: false };
    const field = (txt, name) => (txt.match(new RegExp(`^\\s*${name}\\s*:\\s*(.+)$`, 'mi')) || [])[1]?.trim();
    const current = { state: field(iface.out, 'State'), ssid: field(iface.out, 'SSID'), signal: parseInt(field(iface.out, 'Signal') || '0', 10) };
    const profilesOut = await run('netsh', ['wlan', 'show', 'profiles']);
    const profiles = [...profilesOut.out.matchAll(/All User Profile\s*:\s*(.+)$/gim)].map((m) => m[1].trim());
    const nets = await run('netsh', ['wlan', 'show', 'networks', 'mode=bssid']);
    let networks = [];
    let needsLocation = false;
    if (/location/i.test(nets.out + nets.err) && !/SSID \d+/.test(nets.out)) needsLocation = true;
    else {
      for (const block of nets.out.split(/\r?\n(?=SSID \d+ :)/)) {
        const ssid = (block.match(/^SSID \d+ :\s*(.*)$/m) || [])[1]?.trim();
        if (ssid === undefined) continue;
        const signal = Math.max(0, ...[...block.matchAll(/Signal\s*:\s*(\d+)%/g)].map((m) => +m[1]));
        const auth = (block.match(/Authentication\s*:\s*(.+)$/m) || [])[1]?.trim() || '';
        if (ssid) networks.push({ ssid, signal, secure: !/open/i.test(auth), known: profiles.includes(ssid) });
      }
      networks.sort((a, b) => b.signal - a.signal);
    }
    return { available: true, current, networks, needsLocation };
  }
  async wifiConnect(ssid) {
    const r = await run('netsh', ['wlan', 'connect', `name=${ssid}`]);
    return { ok: r.ok && /success/i.test(r.out), message: (r.out || r.err).trim() };
  }
  async wifiDisconnect() { const r = await run('netsh', ['wlan', 'disconnect']); return { ok: r.ok }; }

  // ---- Power plans (powercfg) ----
  async powerPlans() {
    const r = await run('powercfg', ['/list']);
    return [...r.out.matchAll(/GUID:\s*([0-9a-f-]{36})\s*\((.+?)\)\s*(\*)?/gi)].map((m) => ({ guid: m[1], name: m[2], active: !!m[3] }));
  }
  async setPowerPlan(guid) {
    if (!/^[0-9a-f-]{36}$/i.test(guid)) throw new Error('Bad plan id');
    const r = await run('powercfg', ['/setactive', guid]);
    return { ok: r.ok };
  }

  // ---- Session ----
  async session(kind) {
    const args = { restart: ['/r', '/t', '0'], shutdown: ['/s', '/t', '0'], signout: ['/l'] }[kind];
    if (!args) throw new Error('Unknown action');
    const r = await run('shutdown', args);
    return { ok: r.ok };
  }
}

module.exports = SysControl;
