const { run, helper } = require('./run');
const fs = require('fs');
const path = require('path');
const os = require('os');
let counters = null, windows = { fullscreen: false, apps: [] }, pending = false, last = 0;
// Never block Electron's main thread with OS queries. Refresh one cached snapshot at a time.
function refresh() {
  if (pending || Date.now() - last < 1000) return;
  pending = true; last = Date.now();
  Promise.allSettled([
    run('/usr/sbin/netstat', ['-ibn']).then(out => {
      const seen = new Set(); let rx = 0, tx = 0, adapter = null;
      for (const line of out.split('\n').slice(1)) {
        const f = line.trim().split(/\s+/), name = f[0];
        if (!/^en\d+$/.test(name) || seen.has(name) || !f[2]?.startsWith('<Link')) continue;
        seen.add(name); const r = Number(f[6]) || 0, t = Number(f[9]) || 0;
        rx += r; tx += t;
        if (!adapter || r+t > adapter.total) adapter = { name, total: r+t, linkBps: 0, wifi: name === wifiInterface };
      }
      counters = { rx, tx, adapter };
    }),
    helper('windows').then(w => { windows = w; }),
  ]).finally(() => { pending = false; });
}
let wifiInterface = 'en0';
helper('wifi').then(w => { wifiInterface = w.interface; }).catch(() => {});
async function ping(ip, timeout = 1500) {
  if (!/^[\d.]+$/.test(ip)) return null;
  try { const out = await run('/sbin/ping', ['-n', '-c', '1', '-W', String(timeout), ip], timeout + 1000); return Number(out.match(/time[=<]([\d.]+)/)?.[1]) || null; } catch { return null; }
}
const trash = path.join(os.homedir(), '.Trash');
// Only this user's Trash; no shell commands, symlink traversal, or privileged deletion.
function recycleBinInfo() {
  let size = 0, items = 0;
  const stack = [trash];
  while (stack.length) {
    const p = stack.pop();
    try { for (const e of fs.readdirSync(p, { withFileTypes: true })) { const full = path.join(p,e.name); const s = fs.lstatSync(full); items++; if (e.isDirectory() && !e.isSymbolicLink()) stack.push(full); else size += s.size; } } catch {}
  }
  return { size, items };
}
function emptyRecycleBin() {
  try { for (const n of fs.readdirSync(trash)) fs.rmSync(path.join(trash,n), { recursive: true, force: true }); return true; } catch { return false; }
}
module.exports = { netCounters() { refresh(); return counters; }, isForegroundFullscreen() { refresh(); return windows.fullscreen; }, listAppWindows() { refresh(); return new Map(windows.apps.map(a => [a.pid,a.name])); }, ping, recycleBinInfo, emptyRecycleBin };
