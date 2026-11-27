const { app, BrowserWindow, ipcMain, screen, Tray, Menu, nativeImage, globalShortcut, powerMonitor } = require('electron');
const path = require('path');

const W = 800, H = 700;

if (!app.requestSingleInstanceLock()) {
  app.quit();
} else {
  const Settings = require('./src/settings');
  const Stats = require('./src/stats');
  const NetMon = require('./src/network');
  const native = require('./src/native');
  const cleaner = require('./src/cleaner');
  const procs = require('./src/processes');
  const ClaudeBridge = require('./src/claude');

  let win, tray, settings, ownHwnd = 0;
  let idle = false, cursorTimer = null, lastCursor = '';
  const stats = new Stats();
  const netmon = new NetMon();
  const claude = new ClaudeBridge();

  const send = (ch, data) => { if (win && !win.isDestroyed()) win.webContents.send(ch, data); };

  function applyLoginItem() {
    const opts = { openAtLogin: !!settings.openAtLogin, name: 'Mission Control' };
    if (!app.isPackaged) { opts.path = process.execPath; opts.args = [path.resolve(app.getAppPath())]; }
    app.setLoginItemSettings(opts);
  }

  // Small while collapsed (only the notch, bubble and toast) so idle compositing stays cheap.
  const SMALL = { w: 800, h: 150 };
  let size = 'small';
  function place() {
    if (!win) return;
    const { workArea } = screen.getPrimaryDisplay();
    const w = size === 'full' ? W : SMALL.w, h = size === 'full' ? H : SMALL.h;
    win.setBounds({ x: Math.round(workArea.x + (workArea.width - w) / 2), y: workArea.y, width: w, height: h });
  }

  function createWindow() {
    win = new BrowserWindow({
      width: SMALL.w, height: SMALL.h, frame: false, transparent: true, resizable: false, movable: false,
      minimizable: false, maximizable: false, fullscreenable: false, skipTaskbar: true, alwaysOnTop: true,
      hasShadow: false, show: false, type: 'toolbar', backgroundColor: '#00000000', title: 'Mission Control',
      webPreferences: { preload: path.join(__dirname, 'preload.js'), contextIsolation: true, nodeIntegration: false, spellcheck: false, backgroundThrottling: true },
    });
    place();
    win.setAlwaysOnTop(true, 'floating');
    win.setIgnoreMouseEvents(true, { forward: true });
    win.loadFile(path.join(__dirname, 'renderer', 'index.html'));
    win.once('ready-to-show', () => { win.showInactive(); });
    win.on('blur', () => send('win:blur'));
    if (process.env.MC_SHOTS) win.webContents.on('console-message', (e) => console.log(`[renderer] ${e.level} ${e.message} (${e.lineNumber})`));
    ownHwnd = Number(win.getNativeWindowHandle().readBigUInt64LE(0));
  }

  // ---- Idle mode: hide + stop all sampling while a fullscreen app is in front ----
  function startWork() {
    stats.start();
    netmon.startLive();
    if (!cursorTimer) cursorTimer = setInterval(() => {
      const p = screen.getCursorScreenPoint();
      const k = `${p.x},${p.y}`;
      if (k !== lastCursor) { lastCursor = k; send('cursor', p); }
    }, 50);
  }
  function stopWork() {
    stats.stop();
    netmon.stopLive();
    clearInterval(cursorTimer); cursorTimer = null;
  }
  function enterIdle() {
    if (idle) return;
    idle = true;
    stopWork();
    send('idle', true);
    win.hide();
    updateTray();
  }
  function exitIdle() {
    if (!idle) return;
    idle = false;
    win.showInactive();
    send('idle', false);
    startWork();
    updateTray();
  }
  setInterval(() => {
    if (!win) return;
    const full = settings.idleOnFullscreen && native.isForegroundFullscreen([ownHwnd]);
    if (full) enterIdle(); else exitIdle();
  }, 1500);

  // ---- Tray ----
  function trayIcon() {
    const s = 32, buf = Buffer.alloc(s * s * 4);
    for (let y = 0; y < s; y++) for (let x = 0; x < s; x++) {
      const dx = x - 15.5, dy = y - 15.5, d = Math.hypot(dx, dy);
      const a = Math.max(0, Math.min(1, 14 - d));
      const eye = (Math.abs(dx + 5) < 2 || Math.abs(dx - 5) < 2) && Math.abs(dy + 1) < 3.5;
      const t = d / 14;
      const i = (y * s + x) * 4;
      if (eye) { buf[i] = 40; buf[i + 1] = 18; buf[i + 2] = 10; } else { buf[i] = 255; buf[i + 1] = Math.round(240 - 90 * t); buf[i + 2] = Math.round(70 + 60 * t); }
      buf[i + 3] = Math.round(a * 255);
    }
    return nativeImage.createFromBitmap(buf, { width: s, height: s });
  }
  function updateTray() {
    if (!tray) return;
    tray.setToolTip(idle ? 'Mission Control (idle: fullscreen app)' : 'Mission Control');
    tray.setContextMenu(Menu.buildFromTemplate([
      { label: 'Open Mission Control', accelerator: 'Ctrl+Alt+M', click: () => openMode('panel') },
      { label: 'Ask Claude', accelerator: 'Ctrl+Alt+Space', click: () => openMode('chat') },
      { type: 'separator' },
      { label: 'Launch at startup', type: 'checkbox', checked: !!settings.openAtLogin, click: (m) => setSetting('openAtLogin', m.checked) },
      { label: 'Idle when an app is fullscreen', type: 'checkbox', checked: !!settings.idleOnFullscreen, click: (m) => setSetting('idleOnFullscreen', m.checked) },
      { label: 'Reload widget', click: () => win.reload() },
      { type: 'separator' },
      { label: 'Quit', click: () => app.quit() },
    ]));
  }

  function openMode(mode) {
    if (idle) return;
    win.showInactive();
    win.focus();
    send('ui:open', mode);
  }

  function setSetting(key, value) {
    settings[key] = value;
    Settings.save(settings);
    if (key === 'openAtLogin') applyLoginItem();
    send('settings', publicSettings());
    updateTray();
  }
  const publicSettings = () => ({ ...settings, claudeFound: !!claude.exe });

  // ---- IPC ----
  ipcMain.on('mouse:interactive', (_e, on) => { if (win) win.setIgnoreMouseEvents(!on, { forward: true }); });
  ipcMain.on('ui:focus', () => { if (win) win.focus(); });
  ipcMain.handle('ui:size', (_e, s) => { if (s !== size) { size = s; place(); } return true; });
  ipcMain.handle('settings:get', () => publicSettings());
  ipcMain.handle('settings:set', (_e, k, v) => { setSetting(k, v); return publicSettings(); });
  ipcMain.handle('static:get', () => stats.static);
  ipcMain.handle('procs:list', async () => {
    const own = new Set(app.getAppMetrics().map((m) => m.pid));
    const r = await procs.list(own);
    return { apps: r.apps, background: r.background, total: r.total };
  });
  ipcMain.handle('procs:kill', (_e, pids) => procs.kill(pids));
  ipcMain.handle('cleaner:scan', async () => {
    const rows = await procs.tasklist();
    return cleaner.scan(new Set(rows.map((r) => r.name.toLowerCase())), (p) => send('cleaner:progress', p));
  });
  ipcMain.handle('cleaner:clean', (_e, ids) => cleaner.clean(ids, (p) => send('cleaner:progress', p)));
  ipcMain.handle('speed:run', async () => {
    const r = await netmon.speedTest((p) => send('speed:progress', p));
    settings.speedHistory = [r, ...(settings.speedHistory || [])].slice(0, 8);
    Settings.save(settings);
    return r;
  });
  ipcMain.on('claude:send', (_e, prompt, mode) => claude.send(prompt, mode || settings.chatMode));
  ipcMain.on('claude:stop', () => claude.stop());
  ipcMain.on('claude:new', () => claude.reset());
  ipcMain.on('app:quit', () => app.quit());

  stats.on('stats', (s) => send('stats', s));
  netmon.on('live', (l) => send('latency', l));
  claude.on('event', (e) => send('claude', e));

  // ---- Claude plan limits (5-hour + weekly) ----
  // Updated for free by every widget chat turn. The auto probe only runs while a 5-hour window is
  // already open, so the widget never starts a new usage window on its own.
  claude.on('limits', (l) => {
    if (l) {
      const prev = settings.claudeLimits || {};
      settings.claudeLimits = { ...prev, ...Object.fromEntries(Object.entries(l).filter(([, v]) => v != null)) };
      Settings.save(settings);
    }
    send('limits', settings.claudeLimits);
  });
  const windowOpen = () => (settings.claudeLimits?.fiveHour?.resetsAt || 0) > Date.now();
  function autoProbe() {
    if (idle || !settings.autoLimits || claude.busy) return;
    if (!settings.claudeLimits || windowOpen()) claude.probeLimits();
  }
  setInterval(autoProbe, 10 * 60 * 1000);
  ipcMain.handle('limits:get', () => settings.claudeLimits);
  ipcMain.on('limits:refresh', () => claude.probeLimits());

  app.on('second-instance', () => openMode('panel'));

  app.whenReady().then(() => {
    app.setAppUserModelId('com.missioncontrol.widget');
    settings = Settings.load();
    applyLoginItem();
    createWindow();
    tray = new Tray(trayIcon());
    tray.on('click', () => openMode('panel'));
    updateTray();
    globalShortcut.register('Control+Alt+M', () => openMode('panel'));
    globalShortcut.register('Control+Alt+Space', () => openMode('chat'));
    screen.on('display-metrics-changed', place);
    screen.on('display-added', place);
    screen.on('display-removed', place);
    powerMonitor.on('suspend', stopWork);
    powerMonitor.on('resume', () => { if (!idle) startWork(); });
    setInterval(() => { if (!idle) send('user-idle', powerMonitor.getSystemIdleTime()); }, 5000);
    startWork();
    setTimeout(autoProbe, 8000);
    if (process.env.MC_SHOTS) devShots(process.env.MC_SHOTS);
    if (process.env.MC_DIAG) setInterval(async () => {
      const r = await win.webContents.executeJavaScript("JSON.stringify(document.getAnimations().map(a => (a.animationName || a.transitionProperty) + '@' + (a.effect.target.id || a.effect.target.className)))");
      console.log('[anims]', r);
    }, 3000);
  });

  // Dev only: MC_SHOTS=<dir> renders each view to PNG (no effect in normal use).
  async function devShots(dir) {
    const fs = require('fs');
    const wait = (ms) => new Promise((r) => setTimeout(r, ms));
    const js = (code) => win.webContents.executeJavaScript(code);
    const shot = async (name) => { const img = await win.webContents.capturePage(); fs.writeFileSync(path.join(dir, `${name}.png`), img.toPNG()); };
    await wait(4500);
    await js("document.getElementById('btn-pin').click()"); // keep panel open while the test steals focus
    await shot('1-collapsed');
    await js("document.getElementById('shell').classList.add('hover')"); await wait(900); await shot('2-hover');
    await js("document.getElementById('shell').classList.remove('hover')");
    send('ui:open', 'panel'); await wait(2500); await shot('3-overview');
    for (const t of (process.env.MC_TABS || 'network,apps,cleaner,settings').split(',')) {
      await js(`document.querySelector('.tab[data-tab="${t}"]').click()`); await wait(t === 'cleaner' ? 16000 : 3500); await shot(`4-${t}`);
      if (t === 'network' && process.env.MC_SPEED) {
        await js("document.getElementById('sp-run').click()"); await wait(9000); await shot('4-network-testing');
        await wait(20000); await shot('4-network-result');
      }
    }
    send('ui:open', 'chat'); await wait(1500); await shot('5-chat');
    if (process.env.MC_CHAT) {
      await js(`document.getElementById('chat-in').value = ${JSON.stringify(process.env.MC_CHAT)}; document.getElementById('chat-send').click()`);
      await wait(Number(process.env.MC_CHAT_WAIT || 25000)); await shot('6-chat-reply');
    }
    fs.writeFileSync(path.join(dir, 'done.txt'), 'ok');
  }

  app.on('will-quit', () => { globalShortcut.unregisterAll(); stopWork(); claude.stop(); });
  app.on('window-all-closed', () => app.quit());
}
