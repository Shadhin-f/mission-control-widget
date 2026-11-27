// Cache / junk scanner and cleaner. Deletes folder *contents* only and skips locked files.
const fs = require('fs');
const fsp = fs.promises;
const path = require('path');
const os = require('os');
const native = require('./native');

const L = process.env.LOCALAPPDATA || path.join(os.homedir(), 'AppData', 'Local');
const LOW = path.join(path.dirname(L), 'LocalLow');
const WIN = process.env.SystemRoot || 'C:\\Windows';

const CHROMIUM_SUB = ['Cache', 'Code Cache', 'GPUCache', 'DawnGraphiteCache', 'DawnWebGPUCache', path.join('Service Worker', 'CacheStorage')];
const CHROMIUM_TOP = ['ShaderCache', 'GrShaderCache', 'GraphiteDawnCache'];

function chromiumPaths(userData) {
  const out = [];
  if (!fs.existsSync(userData)) return out;
  for (const t of CHROMIUM_TOP) out.push(path.join(userData, t));
  try {
    for (const d of fs.readdirSync(userData)) {
      if (/^(Default|Profile \d+|Guest Profile|System Profile)$/.test(d)) for (const s of CHROMIUM_SUB) out.push(path.join(userData, d, s));
    }
  } catch {}
  return out.filter((p) => fs.existsSync(p));
}

function firefoxPaths() {
  const root = path.join(L, 'Mozilla', 'Firefox', 'Profiles');
  try { return fs.readdirSync(root).map((d) => path.join(root, d, 'cache2')).filter((p) => fs.existsSync(p)); } catch { return []; }
}

const TWO_HOURS = 2 * 3600 * 1000;

const CATEGORIES = [
  { id: 'usertemp', name: 'User temp files', desc: 'Leftovers from installers and apps', paths: () => [os.tmpdir()], minAge: TWO_HOURS },
  { id: 'wintemp', name: 'Windows temp', desc: 'System temp folder (some files need admin)', paths: () => [path.join(WIN, 'Temp')], minAge: TWO_HOURS, admin: true },
  { id: 'chrome', name: 'Chrome cache', desc: 'Web, code and GPU caches', paths: () => chromiumPaths(path.join(L, 'Google', 'Chrome', 'User Data')), proc: 'chrome.exe' },
  { id: 'edge', name: 'Edge cache', desc: 'Web, code and GPU caches', paths: () => chromiumPaths(path.join(L, 'Microsoft', 'Edge', 'User Data')), proc: 'msedge.exe' },
  { id: 'brave', name: 'Brave cache', desc: 'Web, code and GPU caches', paths: () => chromiumPaths(path.join(L, 'BraveSoftware', 'Brave-Browser', 'User Data')), proc: 'brave.exe' },
  { id: 'firefox', name: 'Firefox cache', desc: 'cache2 folders', paths: firefoxPaths, proc: 'firefox.exe' },
  { id: 'shader', name: 'GPU shader caches', desc: 'DirectX + NVIDIA caches, rebuilt on demand', paths: () => [path.join(L, 'D3DSCache'), path.join(L, 'NVIDIA', 'DXCache'), path.join(L, 'NVIDIA', 'GLCache'), path.join(LOW, 'NVIDIA', 'PerDriverVersion', 'DXCache')] },
  { id: 'inet', name: 'Windows web cache', desc: 'INetCache used by Windows components', paths: () => [path.join(L, 'Microsoft', 'Windows', 'INetCache')] },
  { id: 'crash', name: 'Crash dumps & error reports', desc: 'CrashDumps + Windows Error Reporting', paths: () => [path.join(L, 'CrashDumps'), path.join(L, 'Microsoft', 'Windows', 'WER', 'ReportArchive'), path.join(L, 'Microsoft', 'Windows', 'WER', 'ReportQueue')] },
  { id: 'thumbs', name: 'Thumbnail cache', desc: 'Explorer thumbnails (in-use files skipped)', paths: () => [path.join(L, 'Microsoft', 'Windows', 'Explorer')], filter: (n) => /^(thumbcache|iconcache)_.*\.db$/i.test(n) },
  { id: 'devcache', name: 'npm / pip caches', desc: 'Package download caches', paths: () => [path.join(L, 'npm-cache', '_cacache'), path.join(L, 'pip', 'cache')] },
  { id: 'wupdate', name: 'Windows Update downloads', desc: 'Already-installed update files (needs admin)', paths: () => [path.join(WIN, 'SoftwareDistribution', 'Download')], admin: true },
  { id: 'recycle', name: 'Recycle Bin', desc: 'Everything in the Recycle Bin on all drives', special: 'recycle' },
];

async function dirSize(p, limit = { files: 0 }) {
  let size = 0, files = 0;
  const stack = [p];
  while (stack.length) {
    const dir = stack.pop();
    let entries;
    try { entries = await fsp.readdir(dir, { withFileTypes: true }); } catch { continue; }
    const stats = await Promise.all(entries.map(async (e) => {
      const full = path.join(dir, e.name);
      if (e.isSymbolicLink()) return null;
      if (e.isDirectory()) { stack.push(full); return null; }
      try { return await fsp.lstat(full); } catch { return null; }
    }));
    for (const s of stats) if (s) { size += s.size; files++; }
  }
  return { size, files };
}

async function entriesOf(cat, p) {
  let names;
  try { names = await fsp.readdir(p); } catch { return []; }
  if (cat.filter) names = names.filter(cat.filter);
  return names.map((n) => path.join(p, n));
}

async function scanCategory(cat) {
  if (cat.special === 'recycle') {
    const r = native.recycleBinInfo();
    return { size: r.size, files: r.items };
  }
  let size = 0, files = 0;
  for (const p of cat.paths()) {
    for (const e of await entriesOf(cat, p)) {
      let st;
      try { st = await fsp.lstat(e); } catch { continue; }
      if (cat.minAge && Date.now() - st.mtimeMs < cat.minAge) continue;
      if (st.isDirectory()) { const d = await dirSize(e); size += d.size; files += d.files; } else { size += st.size; files++; }
    }
  }
  return { size, files };
}

async function scan(runningNames, onProgress) {
  const out = [];
  for (const cat of CATEGORIES) {
    const paths = cat.special ? ['bin'] : cat.paths();
    if (!paths.length) continue; // app not installed
    onProgress?.({ id: cat.id, name: cat.name });
    const r = await scanCategory(cat);
    out.push({ id: cat.id, name: cat.name, desc: cat.desc, admin: !!cat.admin, size: r.size, files: r.files, running: cat.proc ? runningNames.has(cat.proc) : false });
  }
  return out;
}

async function clean(ids, onProgress) {
  const results = [];
  for (const id of ids) {
    const cat = CATEGORIES.find((c) => c.id === id);
    if (!cat) continue;
    onProgress?.({ id, name: cat.name });
    const before = await scanCategory(cat);
    if (cat.special === 'recycle') {
      native.emptyRecycleBin();
    } else {
      for (const p of cat.paths()) {
        for (const e of await entriesOf(cat, p)) {
          try {
            if (cat.minAge) { const st = await fsp.lstat(e); if (Date.now() - st.mtimeMs < cat.minAge) continue; }
            await fsp.rm(e, { recursive: true, force: true, maxRetries: 0 });
          } catch {} // locked / access denied: skip
        }
      }
    }
    const after = await scanCategory(cat);
    results.push({ id, name: cat.name, freed: Math.max(0, before.size - after.size), left: after.size });
  }
  return results;
}

module.exports = { scan, clean };
