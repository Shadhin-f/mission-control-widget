const { run, helper } = require('./run');
const path = require('path');
const protectedNames = new Set(['kernel_task','launchd','WindowServer','loginwindow','runningboardd','systemstats','cfprefsd','securityd','trustd','coreaudiod','bluetoothd','airportd','powerd','distnoted','notifyd','opendirectoryd','tccd','amfid','syspolicyd','logd']);
async function tasklist() {
  const out = await run('/bin/ps', ['-axo', 'pid=,uid=,rss=,comm=']);
  return out.split('\n').flatMap(line => { const m = line.match(/^\s*(\d+)\s+(\d+)\s+(\d+)\s+(.+)$/); return m ? [{ pid: +m[1], uid: +m[2], mem: +m[3]*1024, name: path.basename(m[4]), command: m[4] }] : []; });
}
async function list(ownPids = new Set()) {
  const [rows, win] = await Promise.all([tasklist(), helper('windows')]);
  const windows = new Map(win.apps.map(a => [a.pid,a.name])); const groups = new Map();
  for (const r of rows) {
    const g = groups.get(r.name) || { name: r.name, pids: [], mem: 0, title: '', app: false, own: false, protected: false };
    g.pids.push(r.pid); g.mem += r.mem;
    if (windows.has(r.pid)) { g.app = true; g.title = windows.get(r.pid); }
    g.own ||= ownPids.has(r.pid) || r.pid === process.pid;
    g.protected ||= r.uid !== process.getuid() || protectedNames.has(r.name) || r.pid <= 1;
    groups.set(r.name,g);
  }
  const all = [...groups.values()].map(g => ({ ...g, protected: g.protected || g.own, count: g.pids.length })).sort((a,b) => b.mem-a.mem);
  return { apps: all.filter(g => g.app), background: all.filter(g => !g.app).slice(0,80), total: rows.length, running: new Set(rows.map(r => r.name.toLowerCase())) };
}
async function kill(pids) {
  if (!Array.isArray(pids) || !pids.length || pids.some(p => !Number.isSafeInteger(p) || p <= 1)) throw new Error('Invalid process IDs');
  const own = new Set(require('electron').app.getAppMetrics().map(m => m.pid));
  const groups = await list(own); const all = [...groups.apps,...groups.background];
  if (pids.some(p => !all.some(g => g.pids.includes(p) && !g.protected))) throw new Error('System and widget processes are protected');
  try { pids.forEach(p => process.kill(p, 'SIGTERM')); return { ok: true, message: 'Termination requested' }; } catch(e) { return { ok: false, message: e.message }; }
}
module.exports = { tasklist, list, kill };
