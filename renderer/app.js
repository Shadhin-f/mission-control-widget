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
  if (m === state.mode) return;
  state.mode = m;
  clearTimeout(shrinkTimer);
  if (m === 'collapsed') shrinkTimer = setTimeout(() => { if (state.mode === 'collapsed') mc.setSize('small'); }, 750);
  else { await mc.setSize('full'); await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))); }
  if (state.mode !== m) return;
  const shell = $('#shell');
  shell.classList.remove('mode-panel', 'mode-chat', 'hover');
  if (m !== 'collapsed') shell.classList.add(`mode-${m}`);
  if (m !== 'collapsed') { mc.focus(); hideBubble(); }
  if (m === 'chat') setTimeout(() => $('#chat-in').focus(), 350);
  if (m === 'panel') { onTab(state.tab); requestAnimationFrame(moveInk); }
  if (m !== 'panel') stopAppsPolling();
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
  if (e.key === 'Escape') { if ($('#cl-confirm').classList.contains('show')) closeConfirm(); else setMode('collapsed'); }
});
mc.onBlur(() => { if (state.settings.collapseOnBlur && !state.pinned && !$('#cl-confirm').classList.contains('show')) setMode('collapsed'); });
mc.onOpen((m) => setMode(m));
$('#btn-pin').addEventListener('click', () => { state.pinned = !state.pinned; $('#btn-pin').classList.toggle('on', state.pinned); toast(state.pinned ? 'Pinned open' : 'Unpinned'); });

// ---------------- tabs ----------------
function moveInk() {
  const t = $(`.tab[data-tab="${state.tab}"]`), ink = $('.tab-ink');
  if (!t) return;
  ink.style.width = `${t.offsetWidth - 20}px`;
  ink.style.transform = `translateX(${t.offsetLeft + 10}px)`;
}
const TAB_ORDER = ['overview', 'network', 'apps', 'cleaner', 'settings'];
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
  onTab(state.tab);
}));
function onTab(tab) {
  if (tab === 'apps') startAppsPolling(); else stopAppsPolling();
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
  mc.claudeSend(text, $('#chat-mode').value);
}
mc.onClaude((e) => {
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
    if (state.mode !== 'chat') say(`Claude: ${(e.text || 'done').replace(/\s+/g, ' ').slice(0, 90)}${(e.text || '').length > 90 ? '...' : ''}`, null, 6000);
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
$('#chat-send').addEventListener('click', () => { if (state.claudeBusy) mc.claudeStop(); else sendChat($('#chat-in').value); });
$('#chat-new').addEventListener('click', () => {
  mc.claudeNew();
  $('#msgs').innerHTML = '';
  const empty = document.createElement('div');
  empty.className = 'empty'; empty.id = 'chat-empty';
  empty.innerHTML = '<p>New conversation. What should I do?</p>';
  $('#msgs').appendChild(empty);
  setBusy(false);
});
$('.sugs').addEventListener('click', (e) => { if (e.target.tagName === 'BUTTON') sendChat(e.target.textContent); });

// ---------------- Claude plan limits (5-hour + weekly) ----------------
let limits = null;
function fmtReset(ms) {
  const d = new Date(ms), now = new Date();
  const time = d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  if (d.toDateString() === now.toDateString()) return `today ${time}`;
  return `${d.toLocaleDateString([], { weekday: 'short' })} ${time}`;
}
function renderLimits() {
  const chip = $('#c-lim');
  const now = Date.now();
  const lines = [];
  for (const [key, id, label] of [['fiveHour', '#lim-5h', '5-hour'], ['sevenDay', '#lim-7d', 'Weekly']]) {
    const w = limits?.[key];
    const el = $(id);
    // A window whose reset time has passed is back to 0%
    const pct = !w ? null : w.resetsAt && w.resetsAt < now ? 0 : w.pct;
    el.querySelector('b').textContent = pct == null ? '--' : `${Math.round(pct)}%`;
    el.classList.toggle('warn', pct >= 75 && pct < 90);
    el.classList.toggle('crit', pct >= 90);
    if (w) lines.push(`${label}: ${Math.round(pct)}% used${w.resetsAt > now ? ` · resets ${fmtReset(w.resetsAt)}` : ' · reset'}`);
  }
  if (limits?.at) lines.push(`Checked ${new Date(limits.at).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}`);
  chip.title = `Claude Code usage\n${lines.join('\n') || 'No data yet'}\nClick to refresh`;
  const five = limits?.fiveHour;
  if (five && five.resetsAt > now && five.pct >= 90) say(`Claude 5-hour limit at ${Math.round(five.pct)}%. Resets ${fmtReset(five.resetsAt)}.`, 'lim5', 6000, 30 * 60000);
}
mc.onLimits((l) => { $('#c-lim').classList.remove('loading'); if (l) limits = l; renderLimits(); });
$('#c-lim').addEventListener('click', () => { $('#c-lim').classList.add('loading'); mc.refreshLimits(); });
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
  if (!state.settings.claudeFound) $('#chat-model').textContent = 'CLI not found';
  limits = await mc.getLimits();
  renderLimits();
  pet.refresh();
  setTimeout(() => say('Hi! Click me for Mission Control, right-click to ask Claude.', 'hello', 5000, 1e12), 900);
})();
