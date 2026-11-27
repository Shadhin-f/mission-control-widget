// Thin Win32 bridge via koffi: fullscreen detection, app windows, NIC counters,
// native ICMP ping and the recycle bin. Everything here is cheap to call.
const koffi = require('koffi');

const user32 = koffi.load('user32.dll');
const shell32 = koffi.load('shell32.dll');
const iphlp = koffi.load('iphlpapi.dll');
const dwm = koffi.load('dwmapi.dll');

const RECT = koffi.struct('MC_RECT', { left: 'int32', top: 'int32', right: 'int32', bottom: 'int32' });
const MONITORINFO = koffi.struct('MC_MONITORINFO', { cbSize: 'uint32', rcMonitor: RECT, rcWork: RECT, dwFlags: 'uint32' });
const SHQUERYRBINFO = koffi.struct('MC_SHQUERYRBINFO', { cbSize: 'uint32', i64Size: 'int64', i64NumItems: 'int64' });
const EnumWindowsProc = koffi.proto('bool __stdcall MC_EnumWindowsProc(intptr hwnd, intptr lParam)');

const GetForegroundWindow = user32.func('intptr __stdcall GetForegroundWindow()');
const GetWindowRect = user32.func('bool __stdcall GetWindowRect(intptr hWnd, _Out_ MC_RECT *rect)');
const MonitorFromWindow = user32.func('intptr __stdcall MonitorFromWindow(intptr hwnd, uint32 flags)');
const GetMonitorInfoW = user32.func('bool __stdcall GetMonitorInfoW(intptr hMon, _Inout_ MC_MONITORINFO *mi)');
const GetClassNameW = user32.func('int __stdcall GetClassNameW(intptr hWnd, void *buf, int max)');
const GetWindowTextW = user32.func('int __stdcall GetWindowTextW(intptr hWnd, void *buf, int max)');
const GetWindowTextLengthW = user32.func('int __stdcall GetWindowTextLengthW(intptr hWnd)');
const IsWindowVisible = user32.func('bool __stdcall IsWindowVisible(intptr hWnd)');
const IsZoomed = user32.func('bool __stdcall IsZoomed(intptr hWnd)');
const IsIconic = user32.func('bool __stdcall IsIconic(intptr hWnd)');
const GetWindow = user32.func('intptr __stdcall GetWindow(intptr hWnd, uint32 cmd)');
const GetWindowLongW = user32.func('int32 __stdcall GetWindowLongW(intptr hWnd, int idx)');
const GetWindowThreadProcessId = user32.func('uint32 __stdcall GetWindowThreadProcessId(intptr hWnd, _Out_ uint32 *pid)');
const EnumWindows = user32.func('bool __stdcall EnumWindows(MC_EnumWindowsProc *cb, intptr lParam)');
const DwmGetWindowAttribute = dwm.func('int32 __stdcall DwmGetWindowAttribute(intptr hwnd, uint32 attr, _Out_ uint32 *val, uint32 size)');
const SHQueryUserNotificationState = shell32.func('int32 __stdcall SHQueryUserNotificationState(_Out_ int32 *state)');
const SHQueryRecycleBinW = shell32.func('int32 __stdcall SHQueryRecycleBinW(str16 root, _Inout_ MC_SHQUERYRBINFO *info)');
const SHEmptyRecycleBinW = shell32.func('int32 __stdcall SHEmptyRecycleBinW(intptr hwnd, str16 root, uint32 flags)');
const GetIfTable2 = iphlp.func('uint32 __stdcall GetIfTable2(_Out_ void **table)');
const FreeMibTable = iphlp.func('void __stdcall FreeMibTable(void *mem)');
const IcmpCreateFile = iphlp.func('intptr __stdcall IcmpCreateFile()');
const IcmpSendEcho = iphlp.func('uint32 __stdcall IcmpSendEcho(intptr h, uint32 dest, void *data, uint16 size, void *opts, void *reply, uint32 replySize, uint32 timeout)');

const textBuf = Buffer.alloc(1024);
function className(hwnd) {
  const n = GetClassNameW(hwnd, textBuf, 256);
  return n > 0 ? textBuf.toString('utf16le', 0, n * 2) : '';
}

const SHELL_CLASSES = new Set(['Progman', 'WorkerW', 'Shell_TrayWnd', 'Shell_SecondaryTrayWnd', 'Windows.UI.Core.CoreWindow']);

/** True when the foreground window (not ours, not the desktop) covers its whole monitor. */
function isForegroundFullscreen(ownHwnds = []) {
  try {
    const st = [0];
    if (SHQueryUserNotificationState(st) === 0 && (st[0] === 3 || st[0] === 4)) return true; // D3D fullscreen / presentation mode
    const hwnd = Number(GetForegroundWindow());
    if (!hwnd || ownHwnds.includes(hwnd)) return false;
    if (SHELL_CLASSES.has(className(hwnd))) return false;
    if (IsIconic(hwnd)) return false;
    const r = {};
    if (!GetWindowRect(hwnd, r)) return false;
    const mi = { cbSize: MONITORINFO.size, rcMonitor: {}, rcWork: {}, dwFlags: 0 };
    if (!GetMonitorInfoW(MonitorFromWindow(hwnd, 2), mi)) return false;
    const m = mi.rcMonitor;
    if (!(r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom)) return false;
    const WS_CAPTION = 0x00c00000;
    // A maximized captioned window can fill the screen when the taskbar auto-hides: not fullscreen.
    if ((GetWindowLongW(hwnd, -16) & WS_CAPTION) === WS_CAPTION && IsZoomed(hwnd)) return false;
    return true;
  } catch {
    return false;
  }
}

/** Map pid -> title for every real top-level app window (what the taskbar would show). */
function listAppWindows() {
  const res = new Map();
  try {
    EnumWindows((hwnd) => {
      try {
        if (!IsWindowVisible(hwnd)) return true;
        if (GetWindow(hwnd, 4)) return true; // owned popup
        if (GetWindowLongW(hwnd, -20) & 0x80) return true; // WS_EX_TOOLWINDOW
        const len = GetWindowTextLengthW(hwnd);
        if (!len) return true;
        const cloaked = [0];
        DwmGetWindowAttribute(hwnd, 14, cloaked, 4);
        if (cloaked[0]) return true;
        const pid = [0];
        GetWindowThreadProcessId(hwnd, pid);
        const n = GetWindowTextW(hwnd, textBuf, 256);
        if (!res.has(pid[0])) res.set(pid[0], textBuf.toString('utf16le', 0, n * 2));
      } catch {}
      return true;
    }, 0);
  } catch {}
  return res;
}

// MIB_IF_ROW2 layout (x64, 1352 bytes)
const ROW = 1352, OFF_ALIAS = 28, OFF_TYPE = 1128, OFF_FLAGS = 1152, OFF_OPER = 1156, OFF_TXSPEED = 1192, OFF_IN = 1208, OFF_OUT = 1280;

/** Sum of bytes in/out on physical, connected adapters, plus the busiest adapter's name and link speed. */
function netCounters() {
  const out = [null];
  if (GetIfTable2(out) !== 0 || !out[0]) return null;
  const ptr = out[0];
  try {
    const n = koffi.decode(ptr, 0, 'uint32');
    let rx = 0, tx = 0, best = null;
    for (let i = 0; i < n; i++) {
      const base = 8 + i * ROW;
      const flags = koffi.decode(ptr, base + OFF_FLAGS, 'uint8');
      const hardware = flags & 1, filter = flags & 2;
      if (!hardware || filter) continue;
      if (koffi.decode(ptr, base + OFF_OPER, 'int32') !== 1) continue; // IfOperStatusUp
      const type = koffi.decode(ptr, base + OFF_TYPE, 'uint32');
      if (type === 24) continue; // loopback
      const inB = Number(koffi.decode(ptr, base + OFF_IN, 'uint64'));
      const outB = Number(koffi.decode(ptr, base + OFF_OUT, 'uint64'));
      rx += inB; tx += outB;
      if (!best || inB + outB > best.total) {
        const chars = koffi.decode(ptr, base + OFF_ALIAS, 'uint16', 257);
        let name = '';
        for (const c of chars) { if (!c) break; name += String.fromCharCode(c); }
        best = { total: inB + outB, name, linkBps: Number(koffi.decode(ptr, base + OFF_TXSPEED, 'uint64')), wifi: type === 71 };
      }
    }
    return { rx, tx, adapter: best ? { name: best.name, linkBps: best.linkBps, wifi: best.wifi } : null };
  } finally {
    FreeMibTable(ptr);
  }
}

let icmp = null;
function ipToUlong(ip) {
  const p = ip.split('.').map(Number);
  return (p[0] | (p[1] << 8) | (p[2] << 16) | (p[3] << 24)) >>> 0;
}
/** Native ICMP echo on a worker thread. Resolves RTT in ms, or null on loss. */
function ping(ip, timeout = 1500) {
  return new Promise((resolve) => {
    try {
      if (!icmp) icmp = IcmpCreateFile();
      const data = Buffer.alloc(32, 0x61);
      const reply = Buffer.alloc(256);
      IcmpSendEcho.async(icmp, ipToUlong(ip), data, 32, null, reply, reply.length, timeout, (err, n) => {
        if (err || !n) return resolve(null);
        if (reply.readUInt32LE(4) !== 0) return resolve(null); // IP_SUCCESS
        resolve(reply.readUInt32LE(8));
      });
    } catch {
      resolve(null);
    }
  });
}

function recycleBinInfo() {
  try {
    const info = { cbSize: SHQUERYRBINFO.size, i64Size: 0, i64NumItems: 0 };
    if (SHQueryRecycleBinW(null, info) !== 0) return { size: 0, items: 0 };
    return { size: Number(info.i64Size), items: Number(info.i64NumItems) };
  } catch {
    return { size: 0, items: 0 };
  }
}
function emptyRecycleBin() {
  try { return SHEmptyRecycleBinW(0, null, 1 | 2 | 4) === 0; } catch { return false; }
}

module.exports = { isForegroundFullscreen, listAppWindows, netCounters, ping, recycleBinInfo, emptyRecycleBin, _sizes: { MONITORINFO: MONITORINFO.size, SHQUERYRBINFO: SHQUERYRBINFO.size } };
