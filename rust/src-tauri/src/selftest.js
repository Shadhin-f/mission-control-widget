// Dev only (MC_SELFTEST=<file>): drives the same window.mc API the interface uses and reports what each
// call returned. Read-only or no-op calls only (volume/mute/theme/power plan are set to their current values).
(async () => {
  const out = { started: new Date().toISOString(), results: {} };
  const t0 = performance.now();
  const rec = async (name, fn) => {
    const s = performance.now();
    try { const v = await fn(); out.results[name] = { ok: true, ms: Math.round(performance.now() - s), value: v }; }
    catch (e) { out.results[name] = { ok: false, ms: Math.round(performance.now() - s), error: String(e && e.message || e) }; }
  };
  const nextEvent = (sub, ms) => new Promise((res, rej) => { const t = setTimeout(() => rej(new Error('no event in ' + ms + ' ms')), ms); sub((d) => { clearTimeout(t); res(d); }); });
  const short = (v, n = 1500) => { const s = JSON.stringify(v); return s && s.length > n ? JSON.parse(JSON.stringify(v, (k, x) => Array.isArray(x) && x.length > 4 ? [...x.slice(0, 4), `…+${x.length - 4}`] : x)) : v; };

  await rec('getSettings', async () => { const s = await mc.getSettings(); return { platform: s.platform, ai: s.ai, keys: Object.keys(s).length }; });
  await rec('getStatic', () => mc.getStatic());
  await rec('event:stats', async () => short(await nextEvent(mc.onStats, 5000)));
  await rec('event:latency', async () => nextEvent(mc.onLatency, 8000));
  await rec('event:cursor', async () => nextEvent(mc.onCursor, 3000).catch(() => 'no movement (expected when the mouse is still)'));
  await rec('event:user-idle', async () => nextEvent(mc.onUserIdle, 7000));
  await rec('setSize:full', () => mc.setSize('full'));
  await rec('window:full', async () => ({ w: innerWidth, h: innerHeight, x: screenX, y: screenY }));
  await rec('setSize:small', () => mc.setSize('small'));
  await rec('listProcs', async () => { const r = await mc.listProcs(); return { total: r.total, apps: r.apps.length, background: r.background.length, topApp: r.apps[0] && { name: r.apps[0].name, title: r.apps[0].title, mem: r.apps[0].mem }, own: [...r.apps, ...r.background].filter((g) => g.own).map((g) => g.name) }; });
  await rec('sys:state', async () => short(await mc.sys('state')));
  await rec('sys:bt', async () => short(await mc.sys('bt')));
  await rec('sys:brightness', async () => mc.sys('brightness'));
  await rec('wifi', async () => short(await mc.wifi()));
  await rec('powerPlans', () => mc.powerPlans());
  await rec('getLimits', () => mc.getLimits());
  // No-op writes: set each value to what it already is
  await rec('sys:setVolume(same)', async () => { const s = await mc.sys('state'); const d = s.audio.out.find((x) => x.isDefault); return d ? mc.sys('setVolume', { id: d.id, value: d.volume }) : 'no output device'; });
  await rec('sys:setMute(same)', async () => { const s = await mc.sys('state'); const d = s.audio.out.find((x) => x.isDefault); return d ? mc.sys('setMute', { id: d.id, muted: d.muted }) : 'no output device'; });
  await rec('sys:setDefault(same)', async () => { const s = await mc.sys('state'); const d = s.audio.out.find((x) => x.isDefault); return d ? mc.sys('setDefault', { id: d.id }) : 'no output device'; });
  await rec('sys:stepVolume(0)', () => mc.sys('stepVolume', { delta: 0 }));
  await rec('sys:setDark(same)', async () => { const s = await mc.sys('state'); return mc.sys('setDark', { dark: s.dark }); });
  await rec('setPowerPlan(same)', async () => { const p = (await mc.powerPlans()).find((x) => x.active); return p ? mc.setPowerPlan(p.guid) : 'no active plan'; });
  await rec('sys:unknown(rejects)', () => mc.sys('nope'));
  await rec('scanCache', async () => { let progress = 0; mc.onCleaner(() => progress++); const r = await mc.scanCache(); return { categories: r.map((c) => `${c.id}:${(c.size / 1048576).toFixed(1)}MB${c.running ? '(running)' : ''}`), progressEvents: progress }; });
  await rec('getPower', async () => {
    await new Promise((r) => setTimeout(r, 2500)); // first readings arrive about a second after start
    const p = await mc.getPower();
    return { ready: p.ready, wallW: p.wallW, dcW: p.dcW, measured: p.measured, parts: p.parts.map((x) => `${x.kind}:${x.name}:${x.watts.toFixed(1)}W:${x.src}`), hist: p.hist.length, today: p.summary.today, basis: p.summary.basis, currency: p.currency };
  });
  await rec('getCredits', async () => { const c = await mc.getCredits(false); return { repo: c.repo, error: c.error, people: c.people.map((p) => `${p.login}(${p.commits})`) }; });
  await rec('openUrl(non-GitHub rejects)', () => mc.openUrl('https://example.com/'));
  await rec('tabs', () => [...document.querySelectorAll('.tab')].filter((t) => t.offsetWidth).map((t) => `${t.dataset.tab}:${t.offsetWidth}`).concat(`tabsRight:${Math.round(document.querySelector('.tabs').getBoundingClientRect().right)}`, `toolsLeft:${Math.round(document.querySelector('#tabs .tools').getBoundingClientRect().left)}`));
  await rec('killProcs(protected rejects)', async () => { const r = await mc.listProcs(); const g = r.background.find((x) => x.protected); return mc.killProcs(g ? g.pids.slice(0, 1) : [4]); });
  if (window.__MC_SELFTEST_SPEED) {
    await rec('runSpeedTest', async () => { let n = 0; mc.onSpeed(() => n++); const r = await mc.runSpeedTest(); return { ...r, progressEvents: n }; });
  }
  if (window.__MC_SELFTEST_CHAT) {
    await rec('ai:chat', () => new Promise((res) => {
      const ev = []; let text = '';
      mc.onAi((e) => { ev.push(e.kind); if (e.kind === 'delta') text += e.text; if (e.kind === 'end') res({ provider: e.provider, kinds: [...new Set(ev)], text: text.slice(0, 200) }); });
      mc.aiSend(window.__MC_SELFTEST_CHAT, 'Reply with exactly: OK', 'plan');
      setTimeout(() => res({ timeout: true, kinds: ev, text }), 90000);
    }));
  }
  out.totalMs = Math.round(performance.now() - t0);
  await window.__TAURI_INTERNALS__.invoke('dev_report', { report: out });
})();
