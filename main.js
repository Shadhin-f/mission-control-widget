const { app, BrowserWindow, ipcMain, screen, Tray, Menu, nativeImage, globalShortcut, powerMonitor, shell } = require('electron');
const path = require('path');
const fs = require('fs');
const { execFile } = require('child_process');

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
  const CodexBridge = require('./src/codex');
  const SysControl = require('./src/syscontrol');
  const sys = new SysControl();

  let win, tray, settings, ownHwnd = 0;
  let idle = false, cursorTimer = null, lastCursor = '';
  const stats = new Stats();
  const netmon = new NetMon();
  const claude = new ClaudeBridge();
  const codex = new CodexBridge();
  // Which AI CLIs are installed decides whether the chat and the usage chip exist at all
  if (process.env.MC_NO_AI) { claude.exe = null; codex.exe = null; } // dev: preview the no-AI layout
  const aiAvailable = () => ({ claude: !!claude.exe, codex: !!codex.exe });
  const anyAi = () => !!(claude.exe || codex.exe);
  const bridgeFor = (p) => (p === 'codex' ? codex : claude);

  const send = (ch, data) => { if (win && !win.isDestroyed()) win.webContents.send(ch, data); };

  // Launch at sign-in through a Task Scheduler "at log on" task: it starts right at sign-in, while Run-key
  // entries and Startup-folder shortcuts wait behind every other startup app (minutes on a busy PC).
  // Falls back to a Startup-folder shortcut if the task cannot be registered.
  const TASK = 'Mission Control';
  const startupLnk = () => path.join(app.getPath('appData'), 'Microsoft', 'Windows', 'Start Menu', 'Programs', 'Startup', 'Mission Control.lnk');
  function ps(script, env = {}) {
    return new Promise((resolve) => {
      execFile('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-Command', script],
        { windowsHide: true, env: { ...process.env, ...env }, timeout: 30000 }, (err, stdout) => resolve({ ok: !err, out: (stdout || '').trim() }));
    });
  }
  function writeShortcut(appDir) {
    try {
      const lnk = startupLnk();
      shell.writeShortcutLink(lnk, fs.existsSync(lnk) ? 'replace' : 'create', {
        target: process.execPath, args: app.isPackaged ? '' : `"${appDir}"`, cwd: appDir,
        description: 'Mission Control widget', icon: process.execPath, iconIndex: 0,
      });
    } catch (e) { console.error('Could not write the startup shortcut:', e); }
  }
  function removeShortcut() { try { const l = startupLnk(); if (fs.existsSync(l)) fs.unlinkSync(l); } catch {} }
  async function applyLoginItem() {
    const appDir = path.resolve(app.getAppPath());
    const args = app.isPackaged ? '' : `"${appDir}"`;
    // Remove the Run-key entry older versions created (Windows skips long Run commands anyway)
    app.setLoginItemSettings({ openAtLogin: false, name: 'Mission Control', path: process.execPath, args: app.isPackaged ? [] : [appDir] });
    const env = { MC_EXE: process.execPath, MC_ARGS: args, MC_DIR: appDir, MC_TASK: TASK };
    if (!settings.openAtLogin) {
      removeShortcut();
      await ps('Unregister-ScheduledTask -TaskName $env:MC_TASK -Confirm:$false -ErrorAction SilentlyContinue', env);
      return;
    }
    // Skip the (slow) registration when the task already points at this copy
    const check = await ps("$t = Get-ScheduledTask -TaskName $env:MC_TASK -ErrorAction SilentlyContinue; if ($t -and $t.Actions[0].Execute -eq $env:MC_EXE -and $t.Actions[0].Arguments -eq $env:MC_ARGS -and $t.Settings.Priority -eq 4) { 'same' }", env);
    if (check.out === 'same') { removeShortcut(); return; }
    const reg = await ps([
      '$u = "$env:USERDOMAIN\\$env:USERNAME"',
      '$a = New-ScheduledTaskAction -Execute $env:MC_EXE -Argument $env:MC_ARGS -WorkingDirectory $env:MC_DIR',
      '$t = New-ScheduledTaskTrigger -AtLogOn -User $u',
      '$s = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit ([TimeSpan]::Zero) -Priority 4 -MultipleInstances IgnoreNew',
      '$p = New-ScheduledTaskPrincipal -UserId $u -LogonType Interactive -RunLevel Limited',
      "Register-ScheduledTask -TaskName $env:MC_TASK -Description 'Starts the Mission Control widget at sign-in' -Action $a -Trigger $t -Settings $s -Principal $p -Force -ErrorAction Stop | Out-Null",
      "'ok'",
    ].join('; '), env);
    if (reg.out.endsWith('ok')) removeShortcut();
    else writeShortcut(appDir); // fallback: slower, but always works
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
    sys.stop();
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
      { label: claude.exe ? 'Ask Claude' : 'Ask Codex', accelerator: 'Ctrl+Alt+Space', visible: anyAi(), click: () => openMode('chat') },
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
    if (mode === 'chat' && !anyAi()) mode = 'panel'; // no Claude Code or Codex installed: no chat
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
  const publicSettings = () => ({ ...settings, claudeFound: !!claude.exe, ai: aiAvailable() });

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
  ipcMain.on('ai:send', (_e, provider, prompt, mode) => bridgeFor(provider).send(prompt, mode || settings.chatMode));
  ipcMain.on('ai:stop', () => { claude.stop(); codex.stop(); });
  ipcMain.on('ai:new', () => { claude.reset(); codex.reset(); });
  ipcMain.on('app:quit', () => app.quit());

  // ---- System controls ----
  const SETTINGS_PAGES = {
    bluetooth: 'ms-settings:bluetooth', wifi: 'ms-settings:network-wifi', network: 'ms-settings:network',
    sound: 'ms-settings:sound', display: 'ms-settings:display', power: 'ms-settings:powersleep',
    location: 'ms-settings:privacy-location', devices: 'ms-settings:connecteddevices',
    nightlight: 'ms-settings:nightlight', notifications: 'ms-settings:notifications',
  };
  ipcMain.handle('sys:helper', (_e, cmd, args) => sys.helper(String(cmd), args || {}));
  ipcMain.handle('sys:wifi', () => sys.wifi());
  ipcMain.handle('sys:wifiConnect', (_e, ssid) => sys.wifiConnect(String(ssid)));
  ipcMain.handle('sys:wifiDisconnect', () => sys.wifiDisconnect());
  ipcMain.handle('sys:plans', () => sys.powerPlans());
  ipcMain.handle('sys:setPlan', (_e, guid) => sys.setPowerPlan(String(guid)));
  ipcMain.handle('sys:session', (_e, kind) => sys.session(String(kind)));
  ipcMain.on('sys:open', (_e, page) => { if (SETTINGS_PAGES[page]) shell.openExternal(SETTINGS_PAGES[page]); });

  stats.on('stats', (s) => send('stats', s));
  netmon.on('live', (l) => send('latency', l));
  claude.on('event', (e) => send('ai', { ...e, provider: 'claude' }));
  codex.on('event', (e) => send('ai', { ...e, provider: 'codex' }));

  // ---- AI plan limits: Claude (5-hour + weekly) and Codex (whatever windows the plan has) ----
  // Both are sent as { windows: [{ label, pct, resetsAt }], at } so the UI treats them alike.
  const claudeWindows = (l) => (!l ? null : {
    windows: [l.fiveHour && { label: '5h', ...l.fiveHour }, l.sevenDay && { label: '7d', ...l.sevenDay }].filter(Boolean),
    at: l.at,
  });
  const allLimits = () => ({
    claude: claude.exe ? claudeWindows(settings.claudeLimits) : null,
    codex: codex.exe ? settings.codexLimits || null : null,
  });
  const sendLimits = () => send('limits', allLimits());
  // Claude: updated for free by every widget chat turn. The auto probe only runs while a 5-hour window
  // is already open, so the widget never starts a new usage window on its own.
  claude.on('limits', (l) => {
    if (l) {
      const prev = settings.claudeLimits || {};
      settings.claudeLimits = { ...prev, ...Object.fromEntries(Object.entries(l).filter(([, v]) => v != null)) };
      Settings.save(settings);
    }
    sendLimits();
  });
  // Codex: read through app-server, which costs no usage, so it can refresh freely.
  async function refreshCodexLimits() {
    if (!codex.exe || idle) return;
    const l = await codex.readLimits();
    if (l) { settings.codexLimits = l; Settings.save(settings); }
    sendLimits();
  }
  codex.on('turn-finished', () => setTimeout(refreshCodexLimits, 1500));
  const windowOpen = () => (settings.claudeLimits?.fiveHour?.resetsAt || 0) > Date.now();
  function autoProbe() {
    if (idle) return;
    if (claude.exe && settings.autoLimits && !claude.busy && (!settings.claudeLimits || windowOpen())) claude.probeLimits();
  }
  setInterval(autoProbe, 10 * 60 * 1000);
  setInterval(refreshCodexLimits, 5 * 60 * 1000);
  ipcMain.handle('limits:get', () => allLimits());
  ipcMain.on('limits:refresh', () => { if (claude.exe) claude.probeLimits(); refreshCodexLimits(); });

  app.on('second-instance', () => openMode('panel'));

  app.whenReady().then(() => {
    app.setAppUserModelId('com.missioncontrol.widget');
    settings = Settings.load();
    createWindow();
    // Keep sign-in fast: verify the startup task once things settle. Dev capture runs never touch it,
    // so a test copy can't steal startup from the real install.
    if (!process.env.MC_SHOTS && !process.env.MC_DIAG) setTimeout(applyLoginItem, 20000);
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
    setTimeout(refreshCodexLimits, 6000);
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
      if (t === 'controls') {
        await wait(2500); await shot('4-controls-loaded');
        // Preview of the "playing" look (the next poll restores the real state)
        await js("controls.stop(); const m = document.getElementById('media'); m.classList.remove('none'); m.classList.add('playing'); document.getElementById('m-title').textContent = 'Demo track'; document.getElementById('m-artist').textContent = 'Demo artist'");
        await wait(700); await shot('4-controls-playing'); await js('controls.start()');
        await js("controls.openDrawer('out')"); await wait(1500); await shot('4-controls-drawer');
        await js('controls.closeDrawer()'); await wait(700);
      }
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

  app.on('will-quit', () => { globalShortcut.unregisterAll(); stopWork(); claude.stop(); codex.stop(); sys.stop(); });
  app.on('window-all-closed', () => app.quit());
}
