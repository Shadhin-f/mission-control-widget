// Running processes grouped by executable, with memory and whether they own a visible app window.
const { execFile } = require('child_process');
const native = require('./native');

const PROTECTED = new Set([
  'system', 'system idle process', 'registry', 'smss.exe', 'csrss.exe', 'wininit.exe', 'winlogon.exe', 'services.exe',
  'lsass.exe', 'lsaiso.exe', 'svchost.exe', 'dwm.exe', 'fontdrvhost.exe', 'memory compression', 'secure system',
  'explorer.exe', 'sihost.exe', 'ctfmon.exe', 'audiodg.exe', 'spoolsv.exe', 'msmpeng.exe', 'searchhost.exe',
  'startmenuexperiencehost.exe', 'shellexperiencehost.exe', 'taskhostw.exe', 'runtimebroker.exe', 'conhost.exe',
]);

function tasklist() {
  return new Promise((resolve) => {
    execFile('tasklist', ['/fo', 'csv', '/nh'], { windowsHide: true, maxBuffer: 16 << 20 }, (err, out) => {
      if (err) return resolve([]);
      const rows = [];
      for (const line of out.split(/\r?\n/)) {
        const f = [...line.matchAll(/"([^"]*)"/g)].map((m) => m[1]);
        if (f.length < 5) continue;
        rows.push({ name: f[0], pid: Number(f[1]), mem: Number(f[4].replace(/[^\d]/g, '')) * 1024 });
      }
      resolve(rows);
    });
  });
}

async function list(ownPids = new Set()) {
  const [rows, windows] = await Promise.all([tasklist(), Promise.resolve(native.listAppWindows())]);
  const groups = new Map();
  for (const r of rows) {
    const key = r.name.toLowerCase();
    let g = groups.get(key);
    if (!g) { g = { name: r.name, pids: [], mem: 0, title: '', app: false, own: false }; groups.set(key, g); }
    g.pids.push(r.pid);
    g.mem += r.mem;
    if (windows.has(r.pid)) { g.app = true; if (!g.title) g.title = windows.get(r.pid); }
    if (ownPids.has(r.pid)) g.own = true;
  }
  const all = [...groups.values()].map((g) => ({ ...g, count: g.pids.length, protected: PROTECTED.has(g.name.toLowerCase()) || g.own }));
  all.sort((a, b) => b.mem - a.mem);
  return {
    apps: all.filter((g) => g.app),
    background: all.filter((g) => !g.app).slice(0, 80),
    total: rows.length,
    running: new Set(rows.map((r) => r.name.toLowerCase())),
  };
}

function kill(pids) {
  return new Promise((resolve) => {
    const args = ['/F', '/T'];
    for (const p of pids) args.push('/PID', String(p));
    execFile('taskkill', args, { windowsHide: true }, (err, stdout, stderr) => resolve({ ok: !err, message: (stderr || stdout || '').trim() }));
  });
}

module.exports = { list, kill, tasklist };
