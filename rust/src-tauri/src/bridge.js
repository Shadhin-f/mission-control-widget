// Mission Control (Rust edition): exposes the same `window.mc` API that the Electron preload provides,
// so ../../renderer runs unchanged. Injected before the page's own scripts.
(() => {
  const core = () => window.__TAURI_INTERNALS__;
  // Tauri rejects with plain strings; the renderer expects Error objects like Electron's
  const inv = (cmd, args = {}) => core().invoke(cmd, args).catch((e) => { throw new Error(typeof e === 'string' ? e : (e && e.message) || String(e)); });
  const fire = (cmd, args) => { inv(cmd, args).catch(() => {}); };
  const on = (ev) => (cb) => {
    const go = () => {
      const T = window.__TAURI__;
      if (!T || !T.event) return setTimeout(go, 10);
      T.event.listen(ev, (e) => cb(e.payload));
    };
    go();
  };

  // ---- Click-through ----
  // Electron forwards mouse moves to a click-through window; Tauri does not. The Rust side streams the
  // cursor position, and while the window ignores the mouse we hit-test it here and wake the page's own
  // mousemove handler when the pointer reaches part of the widget (.hit).
  let interactive = false;
  const onCursor = on('cursor');
  onCursor((p) => {
    if (interactive || !document.body) return;
    const x = p.x - window.screenX, y = p.y - window.screenY;
    if (x < 0 || y < 0 || x >= window.innerWidth || y >= window.innerHeight) return;
    const el = document.elementFromPoint(x, y);
    if (el && el.closest && (el.closest('.hit') || document.querySelector('#cl-confirm.show'))) {
      el.dispatchEvent(new MouseEvent('mousemove', { bubbles: true, clientX: x, clientY: y, screenX: p.x, screenY: p.y }));
    }
  });

  // ---- Extreme mode glow line ----
  // The renderer opens a child window and writes its HTML; here that window lives in Rust and the page
  // only sends the HTML and the class changes.
  const nativeOpen = window.open.bind(window);
  window.open = (url, name, features) => {
    if (name !== 'mc-edge') return nativeOpen(url, name, features);
    let html = '', closed = false;
    fire('edge_open');
    const classList = {
      add: (c) => fire('edge_class', { name: String(c), on: true }),
      remove: (c) => fire('edge_class', { name: String(c), on: false }),
      toggle: (c, force) => fire('edge_class', { name: String(c), on: !!force }),
    };
    const el = { classList };
    return {
      get closed() { return closed; },
      close() { if (!closed) { closed = true; fire('edge_close'); } },
      document: {
        open() { html = ''; },
        write(h) { html += h; },
        close() { fire('edge_html', { html }); },
        getElementById(id) { return closed || id !== 'e' ? null : el; },
        getAnimations() { return []; },
      },
    };
  };

  window.mc = {
    setInteractive: (v) => { interactive = !!v; fire('set_interactive', { on: !!v }); },
    focus: () => fire('ui_focus'),
    setSize: (size) => inv('ui_size', { size }),
    getSettings: () => inv('settings_get'),
    setSetting: (k, v) => inv('settings_set', { key: k, value: v === undefined ? null : v }),
    getStatic: () => inv('static_get'),
    listProcs: () => inv('procs_list'),
    killProcs: (pids) => inv('procs_kill', { pids }),
    scanCache: () => inv('cleaner_scan'),
    cleanCache: (ids) => inv('cleaner_clean', { ids }),
    runSpeedTest: () => inv('speed_run'),
    aiSend: (provider, prompt, mode) => fire('ai_send', { provider, prompt, mode: mode || null }),
    aiStop: () => fire('ai_stop'),
    aiNew: () => fire('ai_new'),
    quit: () => fire('app_quit'),
    sys: (cmd, args) => inv('sys_helper', { cmd, args: args || {} }),
    wifi: () => inv('sys_wifi'),
    wifiConnect: (ssid) => inv('sys_wifi_connect', { ssid: String(ssid) }),
    wifiDisconnect: () => inv('sys_wifi_disconnect'),
    powerPlans: () => inv('sys_plans'),
    setPowerPlan: (guid) => inv('sys_set_plan', { guid: String(guid) }),
    session: (kind) => inv('sys_session', { kind: String(kind) }),
    openSettings: (page) => fire('sys_open', { page: String(page) }),
    getLimits: () => inv('limits_get'),
    refreshLimits: () => fire('limits_refresh'),
    onLimits: on('limits'),
    onStats: on('stats'),
    onLatency: on('latency'),
    onCursor,
    onIdle: on('idle'),
    onUserIdle: on('user-idle'),
    onBlur: on('win:blur'),
    onOpen: on('ui:open'),
    onSettings: on('settings'),
    onEdgeHot: on('edge:hot'),
    onSpeed: on('speed:progress'),
    onCleaner: on('cleaner:progress'),
    onAi: on('ai'),
  };
})();
