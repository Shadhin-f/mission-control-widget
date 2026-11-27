/* Mission Control renderer */
const $ = (s, r = document) => r.querySelector(s);
const $$ = (s, r = document) => [...r.querySelectorAll(s)];
/* global mc */ // exposed by preload.js

// ---------------- formatting ----------------
const GB = 1024 ** 3;
function fmtBytes(b, d = 1) {
  if (b == null) return '--';
  const u = ['B', 'KB', 'MB', 'GB', 'TB'];
  let i = 0;
  while (b >= 1024 && i < u.length - 1) { b /= 1024; i++; }
  return `${b.toFixed(i === 0 ? 0 : b >= 100 ? 0 : d)} ${u[i]}`;
}
const fmtRate = (b) => (b == null ? '--' : `${fmtBytes(b, b >= 1024 ** 2 ? 1 : 0)}/s`);
const fmtMbps = (m) => (m == null ? '--' : m >= 100 ? m.toFixed(0) : m.toFixed(1));
function fmtUptime(s) {
  const d = Math.floor(s / 86400), h = Math.floor((s % 86400) / 3600), m = Math.floor((s % 3600) / 60);
  return d ? `${d}d ${h}h ${m}m` : `${h}h ${m}m`;
}
const esc = (s) => String(s).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
const hueOf = (s) => { let h = 0; for (const c of s) h = (h * 31 + c.charCodeAt(0)) % 360; return h; };

// ---------------- state ----------------
const state = {
  mode: 'collapsed', tab: 'overview', pinned: false, hover: false,
  stats: null, latency: null, settings: {}, static: {},
  hist: { cpu: [], ram: [], gpu: [], down: [], up: [] }, lastPush: performance.now(),
  latHist: [], userIdle: 0, claudeBusy: false, speedRunning: false,
};
const HIST = 90;

// ---------------- number tweening ----------------
function tweenNum(el, to, fmt = (v) => Math.round(v)) {
  if (!el) return;
  const from = el._v ?? to;
  el._v = to;
  if (Math.abs(from - to) < 0.05 || document.hidden) { el.textContent = fmt(to); return; }
  const t0 = performance.now(), dur = 650;
  cancelAnimationFrame(el._raf);
  const step = (t) => {
    const k = Math.min(1, (t - t0) / dur), e = 1 - Math.pow(1 - k, 3);
    el.textContent = fmt(from + (to - from) * e);
    if (k < 1) el._raf = requestAnimationFrame(step);
  };
  el._raf = requestAnimationFrame(step);
}

// ---------------- click-through: only the widget itself catches the mouse ----------------
let interactive = false;
function setInteractive(v) { if (v !== interactive) { interactive = v; mc.setInteractive(v); } }
document.addEventListener('mousemove', (e) => {
  const hit = !!(e.target && e.target.closest && e.target.closest('.hit'));
  setInteractive(hit || $('#cl-confirm').classList.contains('show'));
  const overNotch = hit && state.mode === 'collapsed';
  if (overNotch !== state.hover) setHover(overNotch);
});
document.addEventListener('mouseleave', () => { setInteractive(false); setHover(false); });

let hoverTimer;
function setHover(v) {
  state.hover = v;
  clearTimeout(hoverTimer);
  if (v) hoverTimer = setTimeout(() => { if (state.hover && state.mode === 'collapsed') { $('#shell').classList.add('hover'); hideBubble(); } pet.refresh(); }, 140);
  else hoverTimer = setTimeout(() => { $('#shell').classList.remove('hover'); pet.refresh(); }, 260);
}

// ---------------- modes ----------------
// The window is small while collapsed (cheap to composite) and grows before expanding.
let shrinkTimer;
async function setMode(m) {
  if (m === 'chat' && !state.provider) m = 'panel'; // no Claude Code or Codex installed
  if (m === state.mode) return;
  state.mode = m;
  clearTimeout(shrinkTimer);
  if (m === 'collapsed') shrinkTimer = setTimeout(() => { if (state.mode === 'collapsed') mc.setSize('small'); }, 750);
  else { await mc.setSize('full'); await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))); }
  if (state.mode !== m) return;
  const shell = $('#shell');
  shell.classList.remove('mode-panel', 'mode-chat', 'hover');
  if (m !== 'collapsed') shell.classList.add(`mode-${m}`);
  if (m === 'panel') fitPanel(); else shell.style.height = '';
  if (m !== 'collapsed') { mc.focus(); hideBubble(); }
  if (m === 'chat') setTimeout(() => $('#chat-in').focus(), 350);
  if (m === 'panel') { onTab(state.tab); requestAnimationFrame(moveInk); }
  if (m !== 'panel') { stopAppsPolling(); controls.stop(); }
  pet.refresh();
  loop.kick();
}
document.addEventListener('click', (e) => {
  const b = e.target.closest('[data-open]');
  if (b) setMode(b.dataset.open);
});
$('#pet').addEventListener('click', () => setMode(state.mode === 'collapsed' ? 'panel' : 'collapsed'));
$('#pet').addEventListener('contextmenu', (e) => { e.preventDefault(); setMode(state.mode === 'chat' ? 'collapsed' : 'chat'); });
document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape') {
    if ($('#cl-confirm').classList.contains('show')) closeConfirm();
    else if (controls.drawer) controls.closeDrawer();
    else setMode('collapsed');
  }
});
mc.onBlur(() => { if (state.settings.collapseOnBlur && !state.pinned && !$('#cl-confirm').classList.contains('show')) setMode('collapsed'); });
mc.onOpen((m) => setMode(m));
$('#btn-pin').addEventListener('click', () => { state.pinned = !state.pinned; $('#btn-pin').classList.toggle('on', state.pinned); toast(state.pinned ? 'Pinned open' : 'Unpinned'); });

// The panel's height follows the active tab's content (no empty space), animated by the shell's spring transition
const BAR_H = 46, TABS_H = 40, MAX_PANEL_H = 668;
function fitPanel() {
  if (state.mode !== 'panel') return;
  const page = $(`.page[data-page="${state.tab}"]`);
  if (!page) return;
  $('#shell').style.height = `${Math.min(MAX_PANEL_H, BAR_H + TABS_H + Math.ceil(page.offsetHeight))}px`;
}
let fitQueued = false;
const pageObserver = new ResizeObserver(() => {
  if (fitQueued) return;
  fitQueued = true;
  requestAnimationFrame(() => { fitQueued = false; fitPanel(); });
});
$$('.page').forEach((p) => pageObserver.observe(p));

// ---------------- tabs ----------------
function moveInk() {
  const t = $(`.tab[data-tab="${state.tab}"]`), ink = $('.tab-ink');
  if (!t) return;
  ink.style.width = `${t.offsetWidth - 20}px`;
  ink.style.transform = `translateX(${t.offsetLeft + 10}px)`;
}
const TAB_ORDER = ['overview', 'controls', 'network', 'apps', 'cleaner', 'settings'];
$$('.tab').forEach((t) => t.addEventListener('click', () => {
  const prev = state.tab;
  state.tab = t.dataset.tab;
  $$('.tab').forEach((x) => x.classList.toggle('active', x === t));
  const dir = TAB_ORDER.indexOf(state.tab) > TAB_ORDER.indexOf(prev);
  $$('.page').forEach((p) => {
    const on = p.dataset.page === state.tab;
    p.classList.toggle('left', !on && (p.dataset.page === prev ? dir : !dir));
    p.classList.toggle('active', on);
  });
  moveInk();
  fitPanel();
  onTab(state.tab);
}));
function onTab(tab) {
  if (tab === 'apps') startAppsPolling(); else stopAppsPolling();
  if (tab === 'controls') controls.start(); else controls.stop();
  if (tab === 'cleaner' && !cleanerState.scanned && !cleanerState.busy) scanCache();
  loop.kick();
}

// ---------------- pet ----------------
const pet = {
  el: $('#pet'), eyes: $('#pet .eyes'),
  tx: 0, ty: 0, x: 0, y: 0, raf: 0,
  mood: 'calm',
  refresh() {
    const s = state.stats;
    let mood = 'calm';
    const ramPct = s ? (s.mem.used / s.mem.total) * 100 : 0;
    if (state.claudeBusy) mood = 'think';
    else if (state.speedRunning) mood = 'excited';
    else if (state.hover || state.mode !== 'collapsed') mood = 'happy';
    else if (s && (s.cpu > 88 || ramPct > 93)) mood = 'stressed';
    else if (state.userIdle > 180) mood = 'sleep';
    if (mood !== this.mood) { this.el.classList.remove(this.mood); this.el.classList.add(mood); this.mood = mood; }
    // calm colour drifts cyan -> violet with CPU load
    // Hue in 4 coarse steps so the colour (and its glow transition) only changes on real load shifts
    const step = s && mood === 'calm' ? Math.min(3, Math.floor(s.cpu / 20)) : -1;
    if (step !== this.hueStep) {
      this.hueStep = step;
      if (step >= 0) this.el.style.setProperty('--pc', `hsl(${186 + step * 22}, 100%, ${62 + step * 2}%)`);
      else this.el.style.removeProperty('--pc');
      document.documentElement.style.setProperty('--pc', getComputedStyle(this.el).getPropertyValue('--pc'));
    }
    this.el.classList.toggle('net', !!(s && s.net.down > 2 * 1024 * 1024));
    this.busy = (mood !== 'calm' && mood !== 'sleep') || this.el.classList.contains('net');
    this.el.classList.toggle('alive', this.busy || !!this.fidgeting);
  },
  look(p) {
    const r = this.el.getBoundingClientRect();
    const cx = window.screenX + r.left + r.width / 2, cy = window.screenY + r.top + r.height / 2;
    const dx = p.x - cx, dy = p.y - cy, d = Math.hypot(dx, dy) || 1;
    const k = Math.min(1, d / 260);
    let tx = (dx / d) * 3.6 * k, ty = (dy / d) * 2.2 * k;
    if (this.mood === 'think') { tx = 2.5; ty = -1.8; }
    // Ignore sub-pixel changes so distant mouse movement does not keep the window repainting
    if (Math.abs(tx - this.tx) < 0.4 && Math.abs(ty - this.ty) < 0.4) return;
    this.tx = tx; this.ty = ty;
    if (!this.raf) this.raf = requestAnimationFrame(() => this.step());
  },
  step() {
    this.x += (this.tx - this.x) * 0.28;
    this.y += (this.ty - this.y) * 0.28;
    const done = Math.abs(this.tx - this.x) < 0.08 && Math.abs(this.ty - this.y) < 0.08;
    if (done) { this.x = this.tx; this.y = this.ty; }
    this.eyes.style.transform = `translate(${this.x.toFixed(1)}px, ${this.y.toFixed(1)}px)`;
    this.raf = done ? 0 : requestAnimationFrame(() => this.step());
  },
  blink() {
    if (this.mood !== 'sleep' && !document.hidden) {
      this.el.classList.add('blink');
      setTimeout(() => this.el.classList.remove('blink'), 130);
      if (Math.random() < 0.25) setTimeout(() => { this.el.classList.add('blink'); setTimeout(() => this.el.classList.remove('blink'), 110); }, 260);
    }
    setTimeout(() => this.blink(), 2400 + Math.random() * 4200);
  },
};
setTimeout(() => pet.blink(), 1500);
// Periodic fidget: one breath/bob cycle every ~15 s so the pet feels alive without animating constantly
(function fidget() {
  if (!document.hidden) {
    pet.fidgeting = true; pet.refresh();
    setTimeout(() => { pet.fidgeting = false; pet.refresh(); }, 3400);
  }
  setTimeout(fidget, 20000 + Math.random() * 15000);
})();
mc.onCursor((p) => pet.look(p));
mc.onUserIdle((s) => { state.userIdle = s; pet.refresh(); });

// ---------------- speech bubble + toast ----------------
const cooldown = {};
let bubbleTimer;
function say(text, key, ms = 4200, cool = 120000) {
  if (state.mode !== 'collapsed') return;
  const now = Date.now();
  if (key && cooldown[key] && now - cooldown[key] < cool) return;
  if (key) cooldown[key] = now;
  const b = $('#bubble');
  b.textContent = text;
  b.classList.add('show');
  clearTimeout(bubbleTimer);
  bubbleTimer = setTimeout(hideBubble, ms);
}
function hideBubble() { $('#bubble').classList.remove('show'); }
let toastTimer;
function toast(text) {
  const t = $('#toast');
  t.style.top = state.mode === 'collapsed' ? '104px' : `${$('#shell').offsetHeight - 52}px`;
  t.style.bottom = 'auto';
  t.textContent = text;
  t.classList.add('show');
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => t.classList.remove('show'), 2600);
}

// ---------------- canvas line charts with smooth scrolling ----------------
class Chart {
  constructor(canvas, series, opts = {}) {
    this.c = canvas; this.ctx = canvas.getContext('2d'); this.series = series; this.opts = opts; this.max = opts.max || 1;
  }
  fit() {
    const dpr = window.devicePixelRatio || 1, w = this.c.clientWidth, h = this.c.clientHeight || this.c.height;
    if (this.c.width !== Math.round(w * dpr)) { this.c.width = Math.round(w * dpr); this.c.height = Math.round(h * dpr); }
    this.w = w; this.h = h; this.dpr = dpr;
  }
  draw(frac) {
    if (!this.c.clientWidth) return;
    this.fit();
    const { ctx, w, h } = this;
    ctx.setTransform(this.dpr, 0, 0, this.dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);
    ctx.strokeStyle = 'rgba(110,200,255,0.07)'; ctx.lineWidth = 1;
    for (let i = 1; i < 4; i++) { const y = Math.round((h * i) / 4) + 0.5; ctx.beginPath(); ctx.moveTo(0, y); ctx.lineTo(w, y); ctx.stroke(); }
    if (!this.opts.max) {
      let m = 1;
      for (const s of this.series) for (const v of state.hist[s.key]) m = Math.max(m, v);
      const target = m * 1.25;
      this.max += (target - this.max) * 0.08;
    }
    const step = w / (HIST - 2);
    for (const s of this.series) {
      const d = state.hist[s.key];
      if (d.length < 2) continue;
      const pts = d.map((v, i) => [w + (1 - frac) * step - (d.length - 1 - i) * step, h - 4 - (Math.min(v, this.max * 1.5) / this.max) * (h - 10)]);
      ctx.beginPath();
      ctx.moveTo(pts[0][0], pts[0][1]);
      for (let i = 1; i < pts.length; i++) {
        const [x0, y0] = pts[i - 1], [x1, y1] = pts[i], mx = (x0 + x1) / 2;
        ctx.bezierCurveTo(mx, y0, mx, y1, x1, y1);
      }
      ctx.strokeStyle = s.color; ctx.lineWidth = 1.8; ctx.shadowColor = s.color; ctx.shadowBlur = 8;
      ctx.stroke();
      ctx.shadowBlur = 0;
      ctx.lineTo(pts[pts.length - 1][0], h); ctx.lineTo(pts[0][0], h); ctx.closePath();
      const g = ctx.createLinearGradient(0, 0, 0, h);
      g.addColorStop(0, s.fill); g.addColorStop(1, 'rgba(0,0,0,0)');
      ctx.fillStyle = g; ctx.fill();
    }
    if (this.opts.label) {
      ctx.fillStyle = 'rgba(125,146,175,0.8)'; ctx.font = '10px Bahnschrift, sans-serif';
      ctx.fillText(this.opts.label(this.max), 6, 12);
    }
  }
}
const loadChart = new Chart($('#ch-load'), [
  { key: 'cpu', color: '#3ef3ff', fill: 'rgba(62,243,255,0.16)' },
  { key: 'ram', color: '#9b7bff', fill: 'rgba(155,123,255,0.10)' },
  { key: 'gpu', color: '#44ffa8', fill: 'rgba(68,255,168,0.08)' },
], { max: 100 });
const netChart = new Chart($('#ch-net'), [
  { key: 'down', color: '#3ef3ff', fill: 'rgba(62,243,255,0.2)' },
  { key: 'up', color: '#ff4fd8', fill: 'rgba(255,79,216,0.12)' },
], { label: (m) => `peak scale ${fmtRate(m)}` });

const loop = {
  running: false,
  kick() { if (!this.running) { this.running = true; requestAnimationFrame((t) => this.frame(t)); } },
  frame() {
    const visible = state.mode === 'panel' && (state.tab === 'overview' || state.tab === 'network') && !document.hidden;
    if (!visible) { this.running = false; return; }
    const frac = Math.min(1, (performance.now() - state.lastPush) / 1000);
    if (state.tab === 'overview') loadChart.draw(frac); else netChart.draw(frac);
    requestAnimationFrame((t) => this.frame(t));
  },
};

// ---------------- stats ----------------
function setGauge(id, pct, label) {
  const g = $(id);
  const p = Math.max(0, Math.min(100, pct || 0));
  g.querySelector('.arc').style.strokeDashoffset = String(251.3 * (1 - p / 100));
  tweenNum(g.querySelector('.gv b'), p, (v) => `${Math.round(v)}%`);
  g.classList.toggle('warn', p >= 75 && p < 90);
  g.classList.toggle('crit', p >= 90);
  if (label !== undefined) g.querySelector('.gsub').textContent = label;
}
function setChip(id, pct) {
  const c = $(id);
  c.querySelector('.v').textContent = pct == null ? '--' : `${Math.round(pct)}%`;
  const k = Math.round(Math.max(0, Math.min(100, pct || 0))) / 100;
  const m = c.querySelector('.meter b');
  if (m._k !== k) { m._k = k; m.style.transform = `scaleX(${k})`; } // skip no-op updates: no repaint
  c.classList.toggle('warn', pct >= 75 && pct < 90);
  c.classList.toggle('crit', pct >= 90);
}

let coresBuilt = false, hotSince = 0;
mc.onStats((s) => {
  state.stats = s;
  const ramPct = (s.mem.used / s.mem.total) * 100;
  const gpu = s.gpu;
  const push = (k, v) => { const a = state.hist[k]; a.push(v); if (a.length > HIST) a.shift(); };
  push('cpu', s.cpu); push('ram', ramPct); push('gpu', gpu?.util ?? 0); push('down', s.net.down); push('up', s.net.up);
  state.lastPush = performance.now();

  // notch
  setChip('#c-cpu', s.cpu);
  setChip('#c-ram', ramPct);
  setChip('#c-gpu', gpu?.util ?? null);
  $('#c-net .dn span').textContent = fmtRate(s.net.down);
  $('#c-net .up span').textContent = fmtRate(s.net.up);

  // pet reactions
  if (s.cpu > 90) { if (!hotSince) hotSince = Date.now(); if (Date.now() - hotSince > 8000) say(`CPU is maxed out (${Math.round(s.cpu)}%). Check the Apps tab?`, 'cpu'); } else hotSince = 0;
  if (ramPct > 92) say(`RAM is almost full: ${fmtBytes(s.mem.used)} of ${fmtBytes(s.mem.total)}`, 'ram');
  const lowDisk = s.disks.find((d) => d.free / d.total < 0.06);
  if (lowDisk) say(`${lowDisk.drive} is almost full (${fmtBytes(lowDisk.free)} left). Try the Cleaner.`, 'disk', 5000, 30 * 60000);
  pet.refresh();

  if (state.mode !== 'panel' || document.hidden) return;

  // overview
  setGauge('#g-cpu', s.cpu, `${state.static.cores || s.cores.length} threads`);
  setGauge('#g-ram', ramPct, `${(s.mem.used / GB).toFixed(1)} / ${(s.mem.total / GB).toFixed(1)} GB`);
  if (gpu) {
    setGauge('#g-gpu', gpu.util, `${gpu.temp ?? '--'}°C${gpu.power != null ? ` · ${Math.round(gpu.power)} W` : ''}`);
    setGauge('#g-vram', (gpu.vramUsed / gpu.vramTotal) * 100, `${(gpu.vramUsed / 1024).toFixed(1)} / ${(gpu.vramTotal / 1024).toFixed(0)} GB`);
  } else { setGauge('#g-gpu', 0, 'No NVIDIA data'); setGauge('#g-vram', 0, '--'); }
  if (!coresBuilt) { $('#cores').innerHTML = s.cores.map(() => '<i><b></b></i>').join(''); coresBuilt = true; }
  $$('#cores b').forEach((b, i) => { b.style.height = `${s.cores[i] || 0}%`; });
  $('#cpu-mhz').textContent = `${(s.cpuMhz / 1000).toFixed(2)} GHz base`;
  renderDisks(s.disks);
  $('#ov-down').textContent = fmtRate(s.net.down);
  $('#ov-up').textContent = fmtRate(s.net.up);
  renderSys(s);

  // network
  $('#nw-down').textContent = `↓ ${fmtRate(s.net.down)}`;
  $('#nw-up').textContent = `↑ ${fmtRate(s.net.up)}`;
  const a = s.net.adapter;
  if (a) $('#adapter').textContent = `${a.wifi ? 'Wi-Fi' : 'Wired'} · ${a.name} · link ${a.linkBps >= 1e9 ? `${(a.linkBps / 1e9).toFixed(1)} Gbps` : `${Math.round(a.linkBps / 1e6)} Mbps`}`;
});

function renderDisks(disks) {
  const box = $('#disks');
  if (box.childElementCount !== disks.length) {
    box.innerHTML = disks.map((d) => `<div class="disk" data-d="${d.drive}"><b>${d.drive}</b><div class="bar"><i></i></div><span></span></div>`).join('');
  }
  let tot = 0, free = 0;
  for (const d of disks) {
    const row = box.querySelector(`[data-d="${d.drive}"]`);
    if (!row) continue;
    const used = 1 - d.free / d.total;
    row.querySelector('i').style.width = `${(used * 100).toFixed(1)}%`;
    row.querySelector('span').textContent = `${fmtBytes(d.free)} free of ${fmtBytes(d.total, 0)}`;
    row.classList.toggle('low', d.free / d.total < 0.1);
    tot += d.total; free += d.free;
  }
  $('#disk-total').textContent = `${fmtBytes(free)} free total`;
}

function renderSys(s) {
  const st = state.static;
  const rows = [
    ['Uptime', fmtUptime(s.uptime)],
    ['CPU', st.cpuModel || ''],
    ['GPU', s.gpu?.name || '--'],
    ['Host', `${st.host || ''} · ${st.os || ''}`],
  ];
  $('#sysinfo').innerHTML = rows.map(([k, v]) => `<span>${k}</span><span title="${esc(v)}">${esc(v)}</span>`).join('');
}

// ---------------- latency ----------------
let wasOffline = false;
mc.onLatency((l) => {
  state.latency = l;
  state.latHist.push(l.last); if (state.latHist.length > 40) state.latHist.shift();
  const label = { online: 'ONLINE', unstable: 'UNSTABLE', offline: 'OFFLINE' }[l.status];
  for (const id of ['#ov-status', '#nw-status']) { const p = $(id); p.textContent = label; p.className = `status-pill ${l.status}`; }
  const ping = l.last == null ? 'lost' : `${Math.round(l.last)} ms`;
  $('#ov-ping').textContent = ping;
  $('#ov-loss').textContent = `${l.loss.toFixed(1)}%`;
  $('#lt-ping').textContent = l.avg == null ? '--' : `${Math.round(l.avg)} ms`;
  $('#lt-jit').textContent = `${l.jitter.toFixed(1)} ms`;
  $('#lt-loss').textContent = `${l.loss.toFixed(1)}%`;
  const bars = $('#lat-bars');
  if (bars.childElementCount !== 40) bars.innerHTML = '<i></i>'.repeat(40);
  const max = Math.max(30, ...state.latHist.filter((x) => x != null));
  const pad = 40 - state.latHist.length;
  [...bars.children].forEach((b, i) => {
    const v = state.latHist[i - pad];
    b.className = v === null ? 'lost' : v > 100 ? 'slow' : '';
    b.style.height = v == null ? '2px' : `${Math.max(4, (v / max) * 100)}%`;
    b.style.visibility = i < pad ? 'hidden' : 'visible';
  });
  if (l.status === 'offline' && !wasOffline) { wasOffline = true; say('Internet connection lost. I will keep checking.', 'off', 5000, 60000); }
  if (l.status === 'online' && wasOffline) { wasOffline = false; say(`Back online · ${ping}`, 'on', 3500, 30000); }
});

// ---------------- speed test ----------------
function setDial(mbps) {
  const frac = Math.min(1, Math.sqrt(Math.max(0, mbps) / 1000));
  $('#dial-arc').style.strokeDashoffset = String(251.3 * (1 - frac));
  tweenNum($('#dial-num'), mbps, (v) => fmtMbps(v));
}
function renderHistory() {
  const h = state.settings.speedHistory || [];
  $('#sp-hist').innerHTML = h.length ? h.map((r) => {
    const d = new Date(r.t);
    return `<div><span>${d.toLocaleDateString([], { month: 'short', day: 'numeric' })} ${d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}</span><span>↓ <b>${fmtMbps(r.down)}</b></span><span>↑ <b>${fmtMbps(r.up)}</b></span><span><b>${Math.round(r.ping ?? 0)}</b> ms</span></div>`;
  }).join('') : '<div><span>No tests yet</span></div>';
}
function showResult(r) {
  $('#sp-down').textContent = `${fmtMbps(r.down)} Mbps`;
  $('#sp-up').textContent = `${fmtMbps(r.up)} Mbps`;
  $('#sp-ping').textContent = r.ping == null ? '--' : `${Math.round(r.ping)} ms`;
  $('#sp-jit').textContent = `${(r.jitter ?? 0).toFixed(1)} ms`;
  $('#sp-loss').textContent = `${(r.loss ?? 0).toFixed(1)}%`;
  $('#sp-isp').textContent = r.isp || '--';
  $('#sp-server').textContent = r.server || 'Cloudflare';
}
$('#sp-run').addEventListener('click', async () => {
  if (state.speedRunning) return;
  state.speedRunning = true; pet.refresh();
  const btn = $('#sp-run');
  btn.disabled = true; btn.textContent = 'Testing...';
  for (const id of ['#sp-down', '#sp-up', '#sp-ping', '#sp-jit', '#sp-loss']) $(id).textContent = '--';
  try {
    const r = await mc.runSpeedTest();
    state.settings.speedHistory = [r, ...(state.settings.speedHistory || [])].slice(0, 8);
    renderHistory();
    setDial(r.down);
    $('#dial-phase').textContent = 'Download result';
    say(`Speed test: ↓ ${fmtMbps(r.down)} / ↑ ${fmtMbps(r.up)} Mbps · ${Math.round(r.ping ?? 0)} ms`, null, 6000);
  } catch (e) {
    $('#dial-phase').textContent = 'Failed';
    toast(`Speed test failed: ${e.message || e}`);
  } finally {
    state.speedRunning = false; pet.refresh();
    btn.disabled = false; btn.textContent = 'Run again';
  }
});
mc.onSpeed((p) => {
  if (p.phase === 'ping') {
    $('#dial-phase').textContent = 'Measuring ping';
    $('#dial-unit').textContent = 'ms';
    if (p.value != null) tweenNum($('#dial-num'), p.value, (v) => Math.round(v));
    $('#dial-arc').style.strokeDashoffset = String(251.3 * (1 - p.pct * 0.15));
  } else if (p.phase === 'download' || p.phase === 'upload') {
    $('#dial-phase').textContent = p.phase === 'download' ? '↓ Download' : '↑ Upload';
    $('#dial-unit').textContent = 'Mbps';
    setDial(p.mbps || 0);
  } else if (p.phase === 'done') {
    showResult(p.result);
  }
});

// ---------------- apps ----------------
let appsTimer = null, appsData = null;
const killArm = new Map();
function startAppsPolling() { if (appsTimer) return; loadApps(); appsTimer = setInterval(loadApps, 3000); }
function stopAppsPolling() { clearInterval(appsTimer); appsTimer = null; }
async function loadApps() {
  if (killArm.size) return; // do not re-render while a kill is being confirmed
  appsData = await mc.listProcs();
  renderApps();
}
$('#app-filter').addEventListener('input', renderApps);
function renderApps() {
  if (!appsData || !state.stats) return;
  const f = $('#app-filter').value.trim().toLowerCase();
  const total = state.stats.mem.total;
  const match = (g) => !f || g.name.toLowerCase().includes(f) || (g.title || '').toLowerCase().includes(f);
  const apps = appsData.apps.filter(match), bg = appsData.background.filter(match);
  const maxMem = Math.max(1, ...appsData.apps.map((g) => g.mem), ...appsData.background.map((g) => g.mem));
  const row = (g) => {
    const label = g.name.replace(/\.exe$/i, '');
    return `<div class="proc" data-name="${esc(g.name)}">
      <div class="ic" style="--h:${hueOf(g.name)}">${esc(label[0]?.toUpperCase() || '?')}</div>
      <div class="nm"><b>${esc(label)}</b><small>${esc(g.title || (g.count > 1 ? `${g.count} processes` : `PID ${g.pids[0]}`))}${g.title && g.count > 1 ? ` · ${g.count} processes` : ''}</small></div>
      <div class="bar"><i style="width:${((g.mem / maxMem) * 100).toFixed(1)}%"></i></div>
      <div class="mem">${fmtBytes(g.mem)}</div>
      <button class="kill" ${g.protected ? 'disabled title="Protected system process"' : 'title="End task"'} data-kill="${esc(g.name)}">✕</button>
    </div>`;
  };
  $('#app-list').innerHTML =
    `<div class="sec">Apps · ${apps.length}</div>${apps.map(row).join('')}` +
    `<div class="sec">Background · ${bg.length}</div>${bg.map(row).join('')}`;
  // memory summary bar: top 5 apps + rest
  const top = [...appsData.apps, ...appsData.background].sort((a, b) => b.mem - a.mem).slice(0, 5);
  const used = state.stats.mem.used;
  const colors = ['#3ef3ff', '#9b7bff', '#ff4fd8', '#44ffa8', '#ffb547'];
  let rest = used;
  let html = top.map((g, i) => { rest -= g.mem; return `<i title="${esc(g.name)} ${fmtBytes(g.mem)}" style="width:${(g.mem / total) * 100}%;background:${colors[i]}"></i>`; }).join('');
  html += `<i style="width:${(Math.max(0, rest) / total) * 100}%;background:rgba(255,255,255,.18)"></i>`;
  $('#mem-bar').innerHTML = html;
  $('#mem-text').textContent = `${fmtBytes(used)} / ${fmtBytes(total)} · ${appsData.total} processes`;
}
$('#app-list').addEventListener('click', async (e) => {
  const b = e.target.closest('[data-kill]');
  if (!b || b.disabled) return;
  const name = b.dataset.kill;
  if (!b.classList.contains('confirm')) {
    b.classList.add('confirm'); b.textContent = 'END?';
    killArm.set(name, setTimeout(() => { killArm.delete(name); b.classList.remove('confirm'); b.textContent = '✕'; }, 3000));
    return;
  }
  clearTimeout(killArm.get(name)); killArm.delete(name);
  const g = [...appsData.apps, ...appsData.background].find((x) => x.name === name);
  if (!g) return;
  const r = await mc.killProcs(g.pids);
  toast(r.ok ? `Ended ${name.replace(/\.exe$/i, '')}` : `Could not end ${name}: ${r.message.split('\n')[0]}`);
  loadApps();
});

// ---------------- cleaner ----------------
const cleanerState = { items: [], selected: new Set(), busy: false, scanned: false, freed: {} };
const DEFAULT_ON = new Set(['usertemp', 'chrome', 'edge', 'brave', 'firefox', 'shader', 'inet', 'crash', 'thumbs']);
async function scanCache() {
  cleanerState.busy = true;
  $('#cl-scan').disabled = true; $('#cl-clean').disabled = true;
  $('#cl-list').innerHTML = '<div class="scanning">SCANNING<b id="cl-now">...</b></div>';
  try {
    cleanerState.items = await mc.scanCache();
    cleanerState.scanned = true;
    if (!cleanerState.selected.size) for (const i of cleanerState.items) if (DEFAULT_ON.has(i.id) && i.size > 0) cleanerState.selected.add(i.id);
    renderCleaner();
  } finally {
    cleanerState.busy = false;
    $('#cl-scan').disabled = false; $('#cl-scan').textContent = 'Rescan';
    updateCleanTotals();
  }
}
mc.onCleaner((p) => { const n = $('#cl-now'); if (n) n.textContent = p.name; $$('.citem').forEach((c) => c.classList.toggle('busy', c.dataset.id === p.id && cleanerState.busy)); });
function renderCleaner() {
  const max = Math.max(1, ...cleanerState.items.map((i) => i.size));
  $('#cl-list').innerHTML = cleanerState.items.map((i) => `
    <div class="citem ${cleanerState.selected.has(i.id) ? 'on' : ''}" data-id="${i.id}">
      <i class="ck"></i>
      <div><b>${esc(i.name)}</b>${i.admin ? '<span class="tag">ADMIN</span>' : ''}${i.running ? '<span class="tag">BROWSER OPEN</span>' : ''}<small>${esc(i.desc)} · ${i.files.toLocaleString()} ${i.id === 'recycle' ? 'items' : 'files'}</small></div>
      <div class="bar"><i data-w="${((i.size / max) * 100).toFixed(1)}"></i></div>
      <div class="sz ${cleanerState.freed[i.id] ? 'freed' : ''}">${cleanerState.freed[i.id] ? `-${fmtBytes(cleanerState.freed[i.id])}` : fmtBytes(i.size)}</div>
    </div>`).join('');
  requestAnimationFrame(() => $$('#cl-list .bar i').forEach((b) => { b.style.width = `${b.dataset.w}%`; }));
  updateCleanTotals();
}
function selectedBytes() { return cleanerState.items.filter((i) => cleanerState.selected.has(i.id)).reduce((a, i) => a + i.size, 0); }
function updateCleanTotals() {
  if (!cleanerState.scanned) return;
  const all = cleanerState.items.reduce((a, i) => a + i.size, 0);
  $('#cl-total').textContent = `${fmtBytes(all)} reclaimable`;
  $('#cl-sub').textContent = `${fmtBytes(selectedBytes())} selected in ${cleanerState.selected.size} locations · in-use files are skipped`;
  $('#cl-clean').disabled = cleanerState.busy || !cleanerState.selected.size;
}
$('#cl-list').addEventListener('click', (e) => {
  const it = e.target.closest('.citem');
  if (!it || cleanerState.busy) return;
  const id = it.dataset.id;
  if (cleanerState.selected.has(id)) cleanerState.selected.delete(id); else cleanerState.selected.add(id);
  it.classList.toggle('on');
  updateCleanTotals();
});
$('#cl-scan').addEventListener('click', () => { cleanerState.freed = {}; scanCache(); });
$('#cl-clean').addEventListener('click', () => {
  const sel = cleanerState.items.filter((i) => cleanerState.selected.has(i.id));
  $('#cf-title').textContent = `Delete ${fmtBytes(selectedBytes())}?`;
  $('#cf-text').innerHTML = `This permanently deletes files from: <br><b style="color:var(--text);font-size:12px">${sel.map((i) => esc(i.name)).join(', ')}</b>.<br>They do not go to the Recycle Bin and cannot be restored. Caches rebuild themselves; open browsers keep their in-use files.`;
  $('#cl-confirm').classList.add('show');
  setInteractive(true);
});
function closeConfirm() { $('#cl-confirm').classList.remove('show'); }
$('#cf-cancel').addEventListener('click', closeConfirm);
$('#cf-ok').addEventListener('click', async () => {
  closeConfirm();
  const ids = [...cleanerState.selected];
  cleanerState.busy = true; updateCleanTotals();
  $('#cl-scan').disabled = true; $('#cl-clean').textContent = 'Cleaning...';
  try {
    const res = await mc.cleanCache(ids);
    let freed = 0;
    for (const r of res) { cleanerState.freed[r.id] = r.freed; freed += r.freed; const it = cleanerState.items.find((i) => i.id === r.id); if (it) it.size = r.left; }
    cleanerState.selected.clear();
    renderCleaner();
    toast(`Freed ${fmtBytes(freed)}`);
    say(`Cleaned up ${fmtBytes(freed)}. Nice and tidy.`, null, 4000);
  } catch (e) {
    toast(`Cleaning failed: ${e.message || e}`);
  } finally {
    cleanerState.busy = false;
    $('#cl-scan').disabled = false; $('#cl-clean').textContent = 'Clean selected';
    $$('.citem').forEach((c) => c.classList.remove('busy'));
    updateCleanTotals();
  }
});

// ---------------- settings ----------------
function applySettings(s) {
  state.settings = s;
  $$('[data-set]').forEach((el) => { if (el.type === 'checkbox') el.checked = !!s[el.dataset.set]; else el.value = s[el.dataset.set]; });
  $('#chat-mode').value = s.chatMode || 'auto';
  applyAi();
  renderLimits();
  renderHistory();
  if (s.speedHistory?.[0]) showResult(s.speedHistory[0]);
}
$$('[data-set]').forEach((el) => el.addEventListener('change', async () => {
  const v = el.type === 'checkbox' ? el.checked : el.value;
  applySettings(await mc.setSetting(el.dataset.set, v));
}));
$('#chat-mode').addEventListener('change', async (e) => applySettings(await mc.setSetting('chatMode', e.target.value)));
$('#btn-quit').addEventListener('click', () => mc.quit());
mc.onSettings(applySettings);

// ---------------- Claude chat ----------------
const chat = { cur: null, seg: null, segText: '', tools: new Map(), streamed: false, renderPending: false };
function md(text) {
  const parts = text.split(/```(?:[\w+-]*)\n?/);
  return parts.map((p, i) => {
    if (i % 2) return `<pre><code>${esc(p.replace(/\n$/, ''))}</code></pre>`;
    let h = esc(p)
      .replace(/`([^`\n]+)`/g, '<code>$1</code>')
      .replace(/\*\*([^*\n]+)\*\*/g, '<b>$1</b>');
    h = h.split(/\n{2,}/).map((block) => {
      const lines = block.split('\n');
      if (lines.every((l) => /^\s*([-*]|\d+\.)\s+/.test(l) || !l.trim())) return `<ul>${lines.filter((l) => l.trim()).map((l) => `<li>${l.replace(/^\s*([-*]|\d+\.)\s+/, '')}</li>`).join('')}</ul>`;
      return `<p>${lines.join('<br>')}</p>`;
    }).join('');
    return h;
  }).join('');
}
function scrollChat() { const m = $('#msgs'); m.scrollTop = m.scrollHeight; }
function addMsg(cls, html) {
  $('#chat-empty')?.remove();
  const d = document.createElement('div');
  d.className = `msg ${cls}`;
  d.innerHTML = html;
  $('#msgs').appendChild(d);
  scrollChat();
  return d;
}
function newSeg() {
  if (chat.seg && !chat.segText.trim()) return;
  chat.seg = document.createElement('div');
  chat.seg.className = 'seg';
  chat.segText = '';
  chat.cur.appendChild(chat.seg);
}
function renderSeg() {
  chat.renderPending = false;
  if (chat.seg) chat.seg.innerHTML = md(chat.segText);
  scrollChat();
}
function setBusy(b) {
  state.claudeBusy = b;
  $('#chat-send').classList.toggle('stop', b);
  $('#chat-send').title = b ? 'Stop' : 'Send (Enter)';
  pet.refresh();
}
function sendChat(text) {
  text = text.trim();
  if (!text || state.claudeBusy) return;
  addMsg('user', esc(text));
  $('#chat-in').value = ''; autosize();
  chat.cur = addMsg('bot', '<div class="typing"><i></i><i></i><i></i></div>');
  chat.seg = null; chat.segText = ''; chat.streamed = false; chat.tools.clear();
  setBusy(true);
  mc.aiSend(state.provider, text, $('#chat-mode').value);
}
mc.onAi((e) => {
  const cur = chat.cur;
  if (e.kind === 'init') { $('#chat-model').textContent = e.model || 'PC control'; return; }
  if (!cur && e.kind !== 'end') return;
  const typing = cur?.querySelector('.typing');
  if (e.kind === 'segment') { if (chat.seg) newSeg(); }
  else if (e.kind === 'delta') {
    typing?.remove();
    if (!chat.seg) newSeg();
    chat.segText += e.text; chat.streamed = true;
    if (!chat.renderPending) { chat.renderPending = true; requestAnimationFrame(renderSeg); }
  } else if (e.kind === 'tool') {
    typing?.remove();
    const t = document.createElement('div');
    t.className = 'tool';
    t.innerHTML = `<i></i><b>${esc(e.name)}</b><span>${esc(e.detail)}</span>`;
    t.title = e.detail;
    cur.appendChild(t);
    chat.tools.set(e.id, t);
    chat.seg = null;
    scrollChat();
  } else if (e.kind === 'tool_result') {
    const t = chat.tools.get(e.id);
    if (t) t.classList.add(e.error ? 'fail' : 'done');
  } else if (e.kind === 'result') {
    typing?.remove();
    if (!chat.streamed && e.text) { newSeg(); chat.segText = e.text; renderSeg(); }
    if (e.denied?.length) {
      const d = document.createElement('div');
      d.className = 'denied';
      d.innerHTML = `Blocked by the "${esc($('#chat-mode').value)}" permission mode:<br>${e.denied.map(esc).join('<br>')}`;
      cur.appendChild(d);
    }
    const meta = document.createElement('div');
    meta.className = 'meta';
    meta.textContent = [e.ms ? `${(e.ms / 1000).toFixed(1)}s` : '', e.cost ? `$${e.cost.toFixed(3)}` : ''].filter(Boolean).join(' · ');
    cur.appendChild(meta);
    $$('.tool:not(.done):not(.fail)', cur).forEach((t) => t.classList.add('done'));
    if (state.mode !== 'chat') say(`${e.provider === 'codex' ? 'Codex' : 'Claude'}: ${(e.text || 'done').replace(/\s+/g, ' ').slice(0, 90)}${(e.text || '').length > 90 ? '...' : ''}`, null, 6000);
    scrollChat();
  } else if (e.kind === 'error') {
    typing?.remove();
    addMsg('err', esc(e.text));
  } else if (e.kind === 'end') {
    cur?.querySelector('.typing')?.remove();
    chat.cur = null;
    setBusy(false);
  }
});
function autosize() { const t = $('#chat-in'); t.style.height = 'auto'; t.style.height = `${Math.min(110, t.scrollHeight)}px`; }
$('#chat-in').addEventListener('input', autosize);
$('#chat-in').addEventListener('keydown', (e) => {
  if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); sendChat($('#chat-in').value); }
});
$('#chat-send').addEventListener('click', () => { if (state.claudeBusy) mc.aiStop(); else sendChat($('#chat-in').value); });
function newChat(text = 'New conversation. What should I do?') {
  mc.aiNew();
  $('#msgs').innerHTML = '';
  const empty = document.createElement('div');
  empty.className = 'empty'; empty.id = 'chat-empty';
  empty.innerHTML = `<p>${esc(text)}</p>`;
  $('#msgs').appendChild(empty);
  setBusy(false);
}
$('#chat-new').addEventListener('click', () => newChat());
// Switch between Claude Code and Codex (only shown when both are installed)
$('#chat-prov').addEventListener('click', async (e) => {
  const b = e.target.closest('[data-prov]');
  if (!b || b.dataset.prov === state.provider) return;
  if (state.claudeBusy) { toast('Wait for the current reply to finish'); return; }
  applySettings(await mc.setSetting('chatProvider', b.dataset.prov));
  newChat(`Now chatting with ${b.dataset.prov === 'codex' ? 'Codex' : 'Claude Code'}. What should I do?`);
});

// Which AI CLIs exist decides what the widget shows: chat, provider switch, usage chip
function applyAi() {
  const ai = state.settings.ai || {};
  const list = ['claude', 'codex'].filter((p) => ai[p]);
  document.body.classList.toggle('no-ai', !list.length);
  document.body.classList.toggle('has-claude', !!ai.claude);
  const pref = state.settings.chatProvider;
  const prev = state.provider;
  state.provider = list.includes(pref) ? pref : list[0] || null;
  $('#chat-prov').style.display = list.length > 1 ? '' : 'none';
  $$('#chat-prov button').forEach((b) => b.classList.toggle('on', b.dataset.prov === state.provider));
  const short = state.provider === 'codex' ? 'Codex' : 'Claude';
  $('#chat-name').textContent = state.provider === 'codex' ? 'Codex' : 'Claude Code';
  $('#q-ask').textContent = `Ask ${short}`;
  $$('.ai-name').forEach((n) => { n.textContent = short; });
  $('#chat').classList.toggle('codex', state.provider === 'codex');
  if (prev !== state.provider) $('#chat-model').textContent = 'PC control';
  if (!list.length && state.mode === 'chat') setMode('collapsed');
}
$('.sugs').addEventListener('click', (e) => { if (e.target.tagName === 'BUTTON') sendChat(e.target.textContent); });

// ---------------- system controls (Controls tab) ----------------
const ICONS = {
  speaker: '<svg viewBox="0 0 24 24"><path d="M4 9.5h3.5L12 5.5v13l-4.5-4H4z"/><path d="M15.5 9a4 4 0 0 1 0 6M18 6.5a7.5 7.5 0 0 1 0 11"/></svg>',
  mic: '<svg viewBox="0 0 24 24"><rect x="9" y="3" width="6" height="11" rx="3"/><path d="M5.5 11a6.5 6.5 0 0 0 13 0M12 17.5V21"/></svg>',
  headphones: '<svg viewBox="0 0 24 24"><path d="M4 15v-3a8 8 0 0 1 16 0v3"/><rect x="3.5" y="14" width="4" height="6.5" rx="1.5"/><rect x="16.5" y="14" width="4" height="6.5" rx="1.5"/></svg>',
  phone: '<svg viewBox="0 0 24 24"><rect x="7" y="2.5" width="10" height="19" rx="2.5"/><path d="M11 18.5h2"/></svg>',
  keyboard: '<svg viewBox="0 0 24 24"><rect x="2.5" y="6.5" width="19" height="11" rx="2"/><path d="M6 10h.01M9 10h.01M12 10h.01M15 10h.01M18 10h.01M7.5 14h9"/></svg>',
  bt: '<svg viewBox="0 0 24 24"><path d="M7 7l10 10-5 4.5V2.5L17 7 7 17"/></svg>',
  wifi: '<svg viewBox="0 0 24 24"><path d="M2.5 9a14 14 0 0 1 19 0M5.5 12.5a9.5 9.5 0 0 1 13 0M8.6 16a5 5 0 0 1 6.8 0M12 19.2h.01"/></svg>',
  lock: '<svg viewBox="0 0 24 24"><rect x="5" y="10.5" width="14" height="10" rx="2"/><path d="M8 10.5V8a4 4 0 0 1 8 0v2.5"/></svg>',
};
const shortName = (n) => (n || '').replace(/\s*\(.*\)\s*$/, '') || n || 'Device';
const SETTINGS_FOR = { wifi: 'wifi', bt: 'bluetooth', out: 'sound', in: 'sound' };

const controls = {
  active: false, timers: [], st: null, bt: null, wifi: null, mons: [], plans: [],
  inflight: false, dragging: null, drawer: null, drawerSig: '', hold: 0,

  start() {
    if (this.active) return;
    this.active = true;
    this.refresh(); this.refreshBt(); this.refreshWifi(); this.refreshBrightness(); this.refreshPlans();
    this.timers = [
      setInterval(() => this.refresh(), 1500),
      setInterval(() => this.refreshBt(), 8000),
      setInterval(() => this.refreshWifi(), 10000),
      setInterval(() => this.refreshBrightness(), 30000),
    ];
    requestAnimationFrame(() => this.measure());
  },
  stop() { this.active = false; this.timers.forEach(clearInterval); this.timers = []; },

  async refresh() {
    if (this.inflight || Date.now() < this.hold) return;
    this.inflight = true;
    try { this.st = await mc.sys('state'); this.render(); } catch (e) { this.fail(e); } finally { this.inflight = false; }
  },
  async refreshBt() { try { this.bt = (await mc.sys('bt')).devices || []; this.render(); } catch {} },
  async refreshWifi() { try { this.wifi = await mc.wifi(); this.render(); } catch {} },
  async refreshBrightness() { try { this.mons = (await mc.sys('brightness')).monitors || []; this.renderBrightness(); } catch { this.mons = []; this.renderBrightness(); } },
  async refreshPlans() { try { this.plans = await mc.powerPlans(); this.renderPlans(); } catch {} },
  fail(e) { const s = $('#t-sound small'); if (s && !this.st) s.textContent = 'Unavailable'; console.warn(e); },

  out() { const l = this.st?.audio?.out || []; return l.find((d) => d.isDefault) || l[0]; },
  inp() { const l = this.st?.audio?.in || []; return l.find((d) => d.isDefault) || l[0]; },
  radio(kind) { return (this.st?.radios || []).find((r) => r.kind === kind); },

  tile(id, on, sub, extra = {}) {
    const t = $(id);
    t.classList.toggle('on', !!on);
    for (const [k, v] of Object.entries(extra)) t.classList.toggle(k, !!v);
    t.querySelector('small').textContent = sub;
  },

  render() {
    const st = this.st;
    if (!st) return;
    // Sound + volume
    const o = this.out();
    if (o) {
      this.tile('#t-sound', !o.muted, o.muted ? 'Muted' : `${o.volume}% · ${shortName(o.name)}`, { muted: o.muted });
      this.setRange('vol', o.volume / 100, o.muted);
      $('#sl-vol .sl-name span').textContent = shortName(o.name);
      $('#sl-vol .sl-v').textContent = o.muted ? 'Muted' : `${o.volume}%`;
    } else this.tile('#t-sound', false, 'No output device');
    // Mic
    const i = this.inp();
    if (i) {
      this.tile('#t-mic', !i.muted, i.muted ? 'Muted' : `On · ${i.volume}%`, { muted: i.muted });
      this.setRange('mic', i.volume / 100, i.muted);
      $('#sl-mic .sl-name span').textContent = shortName(i.name);
      $('#sl-mic .sl-v').textContent = i.muted ? 'Muted' : `${i.volume}%`;
    } else this.tile('#t-mic', false, 'No microphone');
    // Radios
    const wifi = this.radio('WiFi'), bt = this.radio('Bluetooth');
    const wifiOn = wifi?.state === 'On';
    const ssid = this.wifi?.current?.state === 'connected' ? this.wifi.current.ssid : null;
    this.tile('#t-wifi', wifiOn, !wifi ? 'No adapter' : wifiOn ? (ssid || 'Not connected') : 'Off');
    const btOn = bt?.state === 'On';
    const conn = (this.bt || []).filter((d) => d.connected).length;
    this.tile('#t-bt', btOn, !bt ? 'No adapter' : btOn ? (conn ? `${conn} connected` : 'On') : 'Off');
    // Theme
    this.tile('#t-dark', st.dark, st.dark ? 'On' : 'Off');
    // Wired network
    const a = state.stats?.net?.adapter;
    const online = state.latency?.status === 'online';
    if (a && !a.wifi) this.tile('#t-net', online, `${a.linkBps >= 1e9 ? `${(a.linkBps / 1e9).toFixed(0)} Gbps` : `${Math.round(a.linkBps / 1e6)} Mbps`} · ${online ? 'Online' : state.latency?.status || '--'}`);
    else this.tile('#t-net', false, 'Not connected');
    // Media
    const m = st.media, card = $('#media');
    card.classList.toggle('none', !m);
    card.classList.toggle('playing', m?.status === 'Playing');
    $('#m-title').textContent = m?.title || (m ? 'Unknown title' : 'Nothing playing');
    const appName = m ? (m.app || '').replace(/\.exe$/i, '').split('!')[0].split('.').pop() : '';
    $('#m-artist').textContent = m ? (m.artist || appName || 'Unknown artist') : 'Play something in any app';
    $('#m-app').textContent = appName;
    if (this.drawer) this.renderDrawer();
  },

  // ---- sliders ----
  measure() { $$('.page[data-page="controls"] .range').forEach((r) => r.style.setProperty('--w', `${r.clientWidth}px`)); },
  setRange(key, v, off) {
    if (this.dragging === key) return;
    const r = $(`.range[data-range="${key}"]`);
    if (!r) return;
    r.style.setProperty('--v', Math.max(0, Math.min(1, v)).toFixed(3));
    r.classList.toggle('off', !!off);
  },
  sendRange: (() => {
    let timer = null, last = 0;
    return function (key, v, final) {
      const go = () => { last = Date.now(); controls.applyRange(key, v); };
      clearTimeout(timer);
      if (final || Date.now() - last > (key.startsWith('b') ? 250 : 60)) go(); else timer = setTimeout(go, 80);
    };
  })(),
  async applyRange(key, v) {
    this.hold = Date.now() + 600; // don't let a poll snap the slider back mid-drag
    const pct = Math.round(v * 100);
    try {
      if (key === 'vol' && this.out()) { await mc.sys('setVolume', { id: this.out().id, value: pct }); this.out().volume = pct; this.out().muted = false; }
      else if (key === 'mic' && this.inp()) { await mc.sys('setVolume', { id: this.inp().id, value: pct }); this.inp().volume = pct; this.inp().muted = false; }
      else if (key.startsWith('b')) {
        const m = this.mons.find((x) => `b${x.index}` === key);
        if (m) { const val = Math.round(m.min + v * (m.max - m.min)); await mc.sys('setBrightness', { index: m.index, value: val }); m.cur = val; }
      }
    } catch (e) { toast(`Could not change it: ${e.message}`); }
    if (!key.startsWith('b')) this.render();
  },

  renderBrightness() {
    const box = $('#sl-bright');
    if (!this.mons.length) { box.innerHTML = '<div class="sl"><div class="sl-h"><span class="sl-name"><em>Brightness</em></span></div><div class="sl-empty">Not available: turn on DDC/CI in your monitor\'s menu to control it here.</div></div>'; return; }
    box.innerHTML = this.mons.map((m, n) => {
      const pct = m.max > m.min ? (m.cur - m.min) / (m.max - m.min) : 0;
      return `<div class="sl"><div class="sl-h"><span class="sl-name"><em>Brightness</em><span>${esc(this.mons.length > 1 ? `Display ${n + 1}` : shortName(m.name))}</span></span><b class="sl-v">${Math.round(pct * 100)}%</b></div>
        <div class="range bright" data-range="b${m.index}" style="--v:${pct.toFixed(3)}"><i class="fill"></i><i class="knob"></i></div></div>`;
    }).join('');
    this.measure();
  },

  renderPlans() {
    const seg = $('#plans');
    if (!this.plans.length) { seg.innerHTML = '<button disabled>No power plans found</button>'; return; }
    seg.innerHTML = `<i class="seg-ink"></i>${this.plans.map((p) => `<button data-plan="${p.guid}" class="${p.active ? 'on' : ''}" title="${esc(p.name)}">${esc(p.name)}</button>`).join('')}`;
    $('#plan-name').textContent = this.plans.find((p) => p.active)?.name || '--';
    requestAnimationFrame(() => {
      const on = seg.querySelector('button.on'), ink = seg.querySelector('.seg-ink');
      if (on && ink) { ink.style.width = `${on.offsetWidth}px`; ink.style.transform = `translateX(${on.offsetLeft - 3}px)`; }
    });
  },

  // ---- drawer (device / network pickers) ----
  openDrawer(kind) {
    this.drawer = kind; this.drawerSig = '';
    $('#dr-title').textContent = { wifi: 'Wi-Fi networks', bt: 'Bluetooth devices', out: 'Output device', in: 'Input device' }[kind];
    this.renderDrawer();
    $('#drawer').classList.add('open');
    if (kind === 'bt') this.refreshBt();
    if (kind === 'wifi') this.refreshWifi();
  },
  closeDrawer() { this.drawer = null; $('#drawer').classList.remove('open'); },
  renderDrawer() {
    const k = this.drawer;
    let html = '';
    if (k === 'out' || k === 'in') {
      const list = (k === 'out' ? this.st?.audio?.out : this.st?.audio?.in) || [];
      html = list.map((d, n) => `<div class="dev ${d.isDefault ? 'sel' : ''}" data-dev="${esc(d.id)}" style="animation-delay:${n * 40}ms">
          <i class="d-ico">${k === 'out' ? (/head|ear|bud|pods/i.test(d.name) ? ICONS.headphones : ICONS.speaker) : ICONS.mic}</i>
          <div class="d-txt"><b>${esc(shortName(d.name))}</b><small>${esc((d.name.match(/\((.*)\)$/) || [])[1] || '')}${d.muted ? ' · muted' : ` · ${d.volume}%`}</small></div><i class="radio"></i></div>`).join('') || '<div class="dr-empty">No devices found</div>';
    } else if (k === 'bt') {
      const r = this.radio('Bluetooth');
      if (r && r.state !== 'On') html += '<div class="dr-note">Bluetooth is off.<button data-act="bt-on">Turn on</button></div>';
      html += (this.bt || []).map((d, n) => `<div class="dev ${d.connected ? 'conn' : ''}" style="animation-delay:${n * 40}ms">
          <i class="d-ico">${d.kind === 'AudioVideo' ? ICONS.headphones : d.kind === 'Phone' ? ICONS.phone : d.kind === 'Peripheral' ? ICONS.keyboard : ICONS.bt}</i>
          <div class="d-txt"><b>${esc(d.name)}</b><small>${d.connected ? 'Connected' : 'Paired'}</small></div></div>`).join('');
      if (this.bt && !this.bt.length) html += '<div class="dr-empty">No paired devices</div>';
      if (!this.bt) html += '<div class="dr-empty">Looking for devices...</div>';
      html += '<div class="dr-note" style="color:var(--dim);background:none;border-color:var(--line)">Windows only lets its own Settings app connect or pair Bluetooth devices.<button data-act="bt-settings">Connect or add a device</button></div>';
    } else if (k === 'wifi') {
      const w = this.wifi, r = this.radio('WiFi');
      if (!w || !w.available) html = '<div class="dr-empty">No Wi-Fi adapter found</div>';
      else if (r && r.state !== 'On') html = '<div class="dr-note">Wi-Fi is off.<button data-act="wifi-on">Turn on</button></div>';
      else {
        if (w.current?.state === 'connected') html += `<div class="dev conn"><i class="d-ico">${ICONS.wifi}</i><div class="d-txt"><b>${esc(w.current.ssid)}</b><small>Connected · ${w.current.signal}%</small></div><button class="d-act" data-act="wifi-off">Disconnect</button></div>`;
        if (w.needsLocation) html += '<div class="dr-note">Windows needs Location turned on to list Wi-Fi networks.<button data-act="location">Open location settings</button></div>';
        html += (w.networks || []).filter((n) => n.ssid !== w.current?.ssid).map((n, i) => {
          const bars = Math.ceil(n.signal / 25);
          return `<div class="dev" style="animation-delay:${i * 40}ms"><i class="d-ico">${ICONS.wifi}</i><div class="d-txt"><b>${esc(n.ssid)}</b><small>${n.secure ? 'Secured' : 'Open'}${n.known ? ' · Saved' : ''}</small></div>
            <span class="bars">${[1, 2, 3, 4].map((b) => `<i class="${b <= bars ? 'on' : ''}"></i>`).join('')}</span>
            <button class="d-act" data-act="${n.known ? 'wifi-connect' : 'wifi-settings'}" data-ssid="${esc(n.ssid)}">${n.known ? 'Connect' : 'Set up'}</button></div>`;
        }).join('');
        if (!w.needsLocation && !(w.networks || []).length) html += '<div class="dr-empty">No networks in range</div>';
      }
    }
    if (html === this.drawerSig) return; // unchanged: keep DOM (and its animations) as is
    this.drawerSig = html;
    $('#dr-body').innerHTML = html;
  },

  // ---- actions ----
  async toggle(kind) {
    const t = $(`[data-toggle="${kind}"]`);
    t.classList.add('busy');
    this.hold = Date.now() + 800;
    try {
      if (kind === 'wifi') { const r = this.radio('WiFi'); if (!r) throw new Error('No Wi-Fi adapter'); const on = r.state !== 'On'; await mc.sys('setRadio', { kind: 'WiFi', on }); r.state = on ? 'On' : 'Off'; }
      if (kind === 'bt') { const r = this.radio('Bluetooth'); if (!r) throw new Error('No Bluetooth adapter'); const on = r.state !== 'On'; await mc.sys('setRadio', { kind: 'Bluetooth', on }); r.state = on ? 'On' : 'Off'; }
      if (kind === 'sound') { const o = this.out(); await mc.sys('setMute', { id: o.id, muted: !o.muted }); o.muted = !o.muted; }
      if (kind === 'mic') { const i = this.inp(); await mc.sys('setMute', { id: i.id, muted: !i.muted }); i.muted = !i.muted; }
      if (kind === 'dark') { await mc.sys('setDark', { dark: !this.st.dark }); this.st.dark = !this.st.dark; }
      this.render();
    } catch (e) { toast(e.message); }
    t.classList.remove('busy');
  },
};

// Media visualiser bars, each with its own rhythm
$('#viz').innerHTML = Array.from({ length: 30 }, (_, i) => {
  const hi = (0.35 + Math.abs(Math.sin(i * 1.7)) * 0.6).toFixed(2);
  return `<i style="--d:${(0.45 + ((i * 37) % 50) / 100).toFixed(2)}s;--dl:-${((i * 13) % 10) / 10}s;--hi:${hi};--lo:${(0.06 + ((i * 7) % 5) / 40).toFixed(2)}"></i>`;
}).join('');
document.addEventListener('click', (e) => { const b = e.target.closest('[data-settings]'); if (b) mc.openSettings(b.dataset.settings); });

// Tiles: click toggles, chevron opens the picker
$('.tiles').addEventListener('click', (e) => {
  const chev = e.target.closest('[data-drawer]');
  if (chev) { e.stopPropagation(); controls.openDrawer(chev.dataset.drawer); return; }
  const t = e.target.closest('.tile');
  if (!t) return;
  if (t.dataset.openSettings) mc.openSettings(t.dataset.openSettings);
  else if (t.dataset.toggle && controls.st) controls.toggle(t.dataset.toggle);
});
$('.sliders').addEventListener('click', (e) => { const n = e.target.closest('.sl-name[data-drawer]'); if (n) controls.openDrawer(n.dataset.drawer); });
$('#dr-back').addEventListener('click', () => controls.closeDrawer());
$('#dr-settings').addEventListener('click', () => mc.openSettings(SETTINGS_FOR[controls.drawer] || 'devices'));
$('#dr-body').addEventListener('click', async (e) => {
  const dev = e.target.closest('[data-dev]');
  if (dev && !e.target.closest('button')) {
    const list = controls.drawer === 'out' ? controls.st.audio.out : controls.st.audio.in;
    list.forEach((d) => { d.isDefault = d.id === dev.dataset.dev; });
    controls.renderDrawer(); controls.render();
    try { await mc.sys('setDefault', { id: dev.dataset.dev }); toast(`Switched to ${shortName(list.find((d) => d.isDefault).name)}`); } catch (err) { toast(err.message); }
    controls.refresh();
    return;
  }
  const b = e.target.closest('[data-act]');
  if (!b) return;
  const act = b.dataset.act;
  if (act === 'bt-on') controls.toggle('bt');
  if (act === 'wifi-on') controls.toggle('wifi');
  if (act === 'bt-settings') mc.openSettings('bluetooth');
  if (act === 'wifi-settings') mc.openSettings('wifi');
  if (act === 'location') mc.openSettings('location');
  if (act === 'wifi-off') { await mc.wifiDisconnect(); toast('Disconnected'); controls.refreshWifi(); }
  if (act === 'wifi-connect') {
    b.textContent = 'Connecting...';
    const r = await mc.wifiConnect(b.dataset.ssid);
    toast(r.ok ? `Connecting to ${b.dataset.ssid}` : `Could not connect: ${r.message}`);
    setTimeout(() => controls.refreshWifi(), 3000);
  }
});

// Sliders: drag, click or scroll
document.addEventListener('pointerdown', (e) => {
  const r = e.target.closest('.page[data-page="controls"] .range');
  if (!r) return;
  r.setPointerCapture(e.pointerId);
  const key = r.dataset.range;
  controls.dragging = key;
  r.classList.add('drag');
  const set = (ev, final) => {
    const rect = r.getBoundingClientRect();
    const v = Math.max(0, Math.min(1, (ev.clientX - rect.left) / rect.width));
    r.style.setProperty('--v', v.toFixed(3));
    const lbl = r.parentElement.querySelector('.sl-v');
    if (lbl) lbl.textContent = `${Math.round(v * 100)}%`;
    controls.sendRange(key, v, final);
  };
  set(e, false);
  const move = (ev) => set(ev, false);
  const up = (ev) => {
    set(ev, true);
    r.classList.remove('drag');
    controls.dragging = null;
    r.removeEventListener('pointermove', move);
    r.removeEventListener('pointerup', up);
  };
  r.addEventListener('pointermove', move);
  r.addEventListener('pointerup', up);
});
document.addEventListener('wheel', (e) => {
  const r = e.target.closest('.page[data-page="controls"] .range');
  if (!r) return;
  e.preventDefault();
  const v = Math.max(0, Math.min(1, parseFloat(r.style.getPropertyValue('--v') || '0') + (e.deltaY < 0 ? 0.02 : -0.02)));
  r.style.setProperty('--v', v.toFixed(3));
  const lbl = r.parentElement.querySelector('.sl-v');
  if (lbl) lbl.textContent = `${Math.round(v * 100)}%`;
  controls.sendRange(r.dataset.range, v, false);
}, { passive: false });

// Media
$('.m-ctl').addEventListener('click', async (e) => {
  const b = e.target.closest('[data-media]');
  if (!b) return;
  try { await mc.sys('media', { action: b.dataset.media }); } catch (err) { toast(err.message); }
  setTimeout(() => controls.refresh(), 300);
});

// Power plans + actions
$('#plans').addEventListener('click', async (e) => {
  const b = e.target.closest('[data-plan]');
  if (!b || b.classList.contains('on')) return;
  controls.plans.forEach((p) => { p.active = p.guid === b.dataset.plan; });
  controls.renderPlans();
  await mc.setPowerPlan(b.dataset.plan);
  controls.refreshPlans();
});
const armTimers = {};
$('.pw-actions').addEventListener('click', async (e) => {
  const b = e.target.closest('[data-power]');
  if (!b) return;
  const kind = b.dataset.power;
  if (kind === 'lock') { mc.sys('lock'); return; }
  // Sleep, restart and shut down need a second click within 3 s
  if (!b.classList.contains('arm')) {
    b.classList.add('arm');
    b.querySelector('span').textContent = 'Confirm?';
    armTimers[kind] = setTimeout(() => { b.classList.remove('arm'); b.querySelector('span').textContent = { sleep: 'Sleep', restart: 'Restart', shutdown: 'Shut down' }[kind]; }, 3000);
    return;
  }
  clearTimeout(armTimers[kind]);
  b.classList.remove('arm');
  b.querySelector('span').textContent = { sleep: 'Sleep', restart: 'Restart', shutdown: 'Shut down' }[kind];
  if (kind === 'sleep') mc.sys('sleep');
  else mc.session(kind);
});
window.addEventListener('resize', () => controls.measure());

// Scroll on the pet to change the volume, from anywhere
let wheelAcc = 0, wheelBusy = false;
$('#pet').addEventListener('wheel', async (e) => {
  e.preventDefault();
  wheelAcc += e.deltaY < 0 ? 2 : -2;
  if (wheelBusy) return;
  wheelBusy = true;
  try {
    while (wheelAcc) {
      const d = wheelAcc; wheelAcc = 0;
      const v = await mc.sys('stepVolume', { delta: d });
      toast(`Volume ${v}%`);
    }
  } catch (err) { toast(err.message); }
  wheelBusy = false;
}, { passive: false });

// ---------------- AI plan limits (Claude Code + Codex) ----------------
// limits = { claude: { windows: [{ label, pct, resetsAt }], at }, codex: { windows, plan, at } }
let limits = null, limShown = null;
const PROV_NAME = { claude: 'Claude Code', codex: 'Codex' };
const provList = () => ['claude', 'codex'].filter((p) => state.settings.ai?.[p]);
function fmtReset(ms) {
  const d = new Date(ms), now = new Date();
  const time = d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  if (d.toDateString() === now.toDateString()) return `today ${time}`;
  if (ms - now > 6 * 86400000) return d.toLocaleDateString([], { day: 'numeric', month: 'short' });
  return `${d.toLocaleDateString([], { weekday: 'short' })} ${time}`;
}
function renderLimits() {
  const now = Date.now(), lines = [], provs = provList();
  for (const p of ['claude', 'codex']) {
    const row = $(`#c-lim .lim-row[data-p="${p}"]`);
    row.style.display = provs.includes(p) ? '' : 'none';
    const L = limits?.[p];
    const wins = (L?.windows?.length ? L.windows : [{ label: '5h' }, { label: '7d' }]).slice(0, 2);
    row.querySelector('.wins').innerHTML = wins.map((w) => {
      // A window whose reset time has passed is back to 0%
      const pct = w.pct == null ? null : w.resetsAt && w.resetsAt < now ? 0 : w.pct;
      const cls = pct >= 90 ? 'crit' : pct >= 75 ? 'warn' : '';
      return `<span class="w ${cls}"><small>${esc(w.label)}</small><b>${pct == null ? '--' : `${Math.round(pct)}%`}</b></span>`;
    }).join('');
    if (!provs.includes(p) || !L?.windows) continue;
    lines.push(`${PROV_NAME[p]}${L.plan ? ` (${L.plan} plan)` : ''}`);
    for (const w of L.windows) {
      const pct = w.resetsAt && w.resetsAt < now ? 0 : w.pct;
      lines.push(`   ${w.label}: ${Math.round(pct)}% used${w.resetsAt > now ? ` · resets ${fmtReset(w.resetsAt)}` : ''}`);
      if (pct >= 90 && w.resetsAt > now) say(`${PROV_NAME[p]} ${w.label} limit at ${Math.round(pct)}%. Resets ${fmtReset(w.resetsAt)}.`, `lim-${p}-${w.label}`, 6000, 30 * 60000);
    }
  }
  $('#c-lim').title = `AI usage\n${lines.join('\n') || 'No data yet'}\nClick to refresh`;
  if (!provs.includes(limShown)) showLimRow(provs[0], false);
}
function showLimRow(p, animate = true) {
  if (!p) return;
  const prev = limShown && $(`#c-lim .lim-row[data-p="${limShown}"]`);
  const next = $(`#c-lim .lim-row[data-p="${p}"]`);
  limShown = p;
  if (prev && prev !== next) {
    prev.classList.remove('show');
    if (animate) { prev.classList.add('out'); setTimeout(() => prev.classList.remove('out'), 600); }
  }
  next.classList.remove('out');
  next.classList.add('show');
}
// With both installed, the chip alternates between Claude and Codex every 6 s
setInterval(() => {
  const provs = provList();
  if (provs.length < 2 || document.hidden) return;
  showLimRow(provs[(provs.indexOf(limShown) + 1) % provs.length]);
}, 6000);
mc.onLimits((l) => { $('#c-lim').classList.remove('loading'); if (l) limits = l; renderLimits(); });
$('#c-lim').addEventListener('click', () => { $('#c-lim').classList.add('loading'); mc.refreshLimits(); setTimeout(() => $('#c-lim').classList.remove('loading'), 25000); });
setInterval(renderLimits, 60000);

// ---------------- clock ----------------
function tickClock() {
  const d = new Date();
  $('#clock').textContent = `${d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })} · ${d.toLocaleDateString([], { weekday: 'short', day: 'numeric', month: 'short' })}`;
}
setInterval(tickClock, 10000); tickClock();

// ---------------- idle (fullscreen) ----------------
mc.onIdle((idle) => { if (idle) { setMode('collapsed'); hideBubble(); } });

// ---------------- boot ----------------
(async () => {
  state.static = await mc.getStatic();
  applySettings(await mc.getSettings());
  limits = await mc.getLimits();
  renderLimits();
  pet.refresh();
  const greet = state.provider ? `Hi! Click me for Mission Control, right-click to ask ${state.provider === 'codex' ? 'Codex' : 'Claude'}.` : 'Hi! Click me for Mission Control. Scroll on me to change the volume.';
  setTimeout(() => say(greet, 'hello', 5000, 1e12), 900);
})();
