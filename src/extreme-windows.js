// Extreme mode (Windows only): restyles the native taskbar to match the widget using Windows' own
// accent-colour settings. No code injection and no extra process: Explorer keeps every taskbar feature
// (tray icons, jump lists, previews, notifications). The user's original values are backed up exactly
// (including values that didn't exist) and restored when the mode is turned off or the widget quits.
const { execFile, execFileSync } = require('child_process');
const koffi = require('koffi');

const user32 = koffi.load('user32.dll');
const shell32 = koffi.load('shell32.dll');
const EX_RECT = koffi.struct('EX_RECT', { left: 'int32', top: 'int32', right: 'int32', bottom: 'int32' });
const EX_APPBARDATA = koffi.struct('EX_APPBARDATA', { cbSize: 'uint32', hWnd: 'intptr', uCallbackMessage: 'uint32', uEdge: 'uint32', rc: EX_RECT, lParam: 'intptr' });
const SendMessageTimeoutW = user32.func('intptr __stdcall SendMessageTimeoutW(intptr h, uint32 msg, uintptr w, str16 l, uint32 f, uint32 t, _Out_ uintptr *r)');
const SHAppBarMessage = shell32.func('uintptr __stdcall SHAppBarMessage(uint32 msg, _Inout_ EX_APPBARDATA *data)');

const ACCENT = 'HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Accent';
const PERSONALIZE = 'HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize';
const DESKTOP = 'HKCU\\Control Panel\\Desktop';

// The widget's palette, lightest to darkest (RGBA). Windows draws the taskbar from the dark end
// (navy, blended with the wallpaper) and the running-app indicators from the light end (cyan).
const THEME_PALETTE = 'B8FBFF007FF6FF003EF3FF001FA9C400145E7A000A1A3000060E1C003EF3FF00';
const VALUES = [
  { key: ACCENT, name: 'AccentPalette', type: 'REG_BINARY', theme: THEME_PALETTE },
  { key: ACCENT, name: 'AccentColorMenu', type: 'REG_DWORD', theme: '0xffc4a91f' }, // accent #1FA9C4 (ABGR)
  { key: ACCENT, name: 'StartColorMenu', type: 'REG_DWORD', theme: '0xff301a0a' }, // Start menu navy #0A1A30
  { key: PERSONALIZE, name: 'ColorPrevalence', type: 'REG_DWORD', theme: '0x1' }, // "Show accent colour on Start and taskbar"
  { key: PERSONALIZE, name: 'SystemUsesLightTheme', type: 'REG_DWORD', theme: '0x0' }, // that option needs the dark system theme
  { key: DESKTOP, name: 'AutoColorization', type: 'REG_DWORD', theme: '0x0' }, // stop the wallpaper overriding the accent
];

const reg = (args) => new Promise((resolve) => execFile('reg.exe', args, { windowsHide: true }, (err, out) => resolve({ ok: !err, out: String(out || '') })));

async function readValue(v) {
  const r = await reg(['query', v.key, '/v', v.name]);
  if (!r.ok) return null; // value doesn't exist
  const m = r.out.match(new RegExp(`^\\s+${v.name}\\s+(REG_\\w+)\\s+(.*)$`, 'm'));
  return m ? { type: m[1], data: m[2].trim() } : null;
}

const regAddArgs = (v, type, data) => ['add', v.key, '/v', v.name, '/t', type, '/d', data, '/f'];
const regDeleteArgs = (v) => ['delete', v.key, '/v', v.name, '/f'];

// Tells Explorer and other apps the colours changed (async so the UI thread never waits on a busy app)
function broadcast() {
  return new Promise((resolve) => {
    try { SendMessageTimeoutW.async(0xffff, 0x1A, 0, 'ImmersiveColorSet', 2, 1000, [0], () => resolve()); } catch { resolve(); }
  });
}

// Current values, to back up before theming
async function backup() {
  const out = {};
  for (const v of VALUES) out[v.name] = await readValue(v);
  return out;
}

// True when the taskbar already carries our theme (e.g. after a crash), so the old backup must be kept
async function isThemed() {
  const p = await readValue(VALUES[0]);
  return !!p && p.data.toUpperCase() === THEME_PALETTE;
}

async function apply() {
  for (const v of VALUES) await reg(regAddArgs(v, v.type, v.theme));
  await broadcast();
}

async function restore(saved) {
  if (!saved) return;
  for (const v of VALUES) {
    const o = saved[v.name];
    await reg(o ? regAddArgs(v, o.type, o.data) : regDeleteArgs(v));
  }
  await broadcast();
}

// Synchronous restore for app quit, where async work can't finish
function restoreSync(saved) {
  if (!saved) return;
  for (const v of VALUES) {
    const o = saved[v.name];
    try { execFileSync('reg.exe', o ? regAddArgs(v, o.type, o.data) : regDeleteArgs(v), { windowsHide: true, stdio: 'ignore' }); } catch {}
  }
  try { const r = [0]; SendMessageTimeoutW(0xffff, 0x1A, 0, 'ImmersiveColorSet', 2, 300, r); } catch {}
}

// Taskbar position in physical pixels, its edge (0 left, 1 top, 2 right, 3 bottom) and auto-hide state
function taskbar() {
  try {
    const data = { cbSize: EX_APPBARDATA.size, hWnd: 0, uCallbackMessage: 0, uEdge: 0, rc: { left: 0, top: 0, right: 0, bottom: 0 }, lParam: 0 };
    const autohide = (Number(SHAppBarMessage(4 /* ABM_GETSTATE */, data)) & 1) === 1;
    if (!SHAppBarMessage(5 /* ABM_GETTASKBARPOS */, data)) return null;
    const r = data.rc;
    return { x: r.left, y: r.top, width: r.right - r.left, height: r.bottom - r.top, edge: data.uEdge, autohide };
  } catch {
    return null;
  }
}

module.exports = { backup, isThemed, apply, restore, restoreSync, taskbar, THEME_PALETTE };
