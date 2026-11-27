// Live latency/loss monitor and the on-demand internet speed test (Cloudflare endpoints).
const net = require('net');
const crypto = require('crypto');
const { EventEmitter } = require('events');
const native = require('./native');

const TARGET = '1.1.1.1';

function tcpPing(host = TARGET, port = 443, timeout = 2000) {
  return new Promise((resolve) => {
    const t0 = process.hrtime.bigint();
    const s = net.connect({ host, port });
    const done = (v) => { s.destroy(); resolve(v); };
    s.setTimeout(timeout, () => done(null));
    s.once('connect', () => done(Number(process.hrtime.bigint() - t0) / 1e6));
    s.once('error', () => done(null));
  });
}

async function probe() {
  const t = await native.ping(TARGET, 1500);
  if (t !== null) return t;
  const tcp = await tcpPing(); // ICMP may be filtered on some networks
  return tcp === null ? null : Math.round(tcp);
}

function summarize(samples) {
  const ok = samples.filter((x) => x !== null);
  const loss = samples.length ? ((samples.length - ok.length) / samples.length) * 100 : 0;
  const avg = ok.length ? ok.reduce((a, b) => a + b, 0) / ok.length : null;
  let jitter = 0;
  for (let i = 1; i < ok.length; i++) jitter += Math.abs(ok[i] - ok[i - 1]);
  jitter = ok.length > 1 ? jitter / (ok.length - 1) : 0;
  return { avg, min: ok.length ? Math.min(...ok) : null, jitter, loss, count: samples.length };
}

class NetMon extends EventEmitter {
  constructor() {
    super();
    this.samples = [];
    this.timer = null;
    this.testing = false;
  }

  startLive() {
    if (this.timer) return;
    const run = async () => {
      const t = await probe();
      this.samples.push(t);
      if (this.samples.length > 40) this.samples.shift();
      const s = summarize(this.samples);
      const recent = this.samples.slice(-3);
      let status = 'online';
      if (recent.length === 3 && recent.every((x) => x === null)) status = 'offline';
      else if (t === null || s.loss > 5 || (s.avg ?? 0) > 150) status = 'unstable';
      this.emit('live', { last: t, ...s, status });
    };
    run();
    this.timer = setInterval(run, 3000);
  }

  stopLive() { clearInterval(this.timer); this.timer = null; }

  async speedTest(progress) {
    if (this.testing) throw new Error('A test is already running');
    this.testing = true;
    try {
      let meta = {};
      try {
        const r = await fetch('https://speed.cloudflare.com/meta', { signal: AbortSignal.timeout(4000) });
        meta = await r.json();
      } catch {}

      // 1) Latency / jitter / packet loss: 30 native ICMP echoes, 80 ms apart
      progress({ phase: 'ping', pct: 0 });
      const pings = [];
      const jobs = [];
      for (let i = 0; i < 30; i++) {
        jobs.push(native.ping(TARGET, 1000).then((t) => { pings.push(t); progress({ phase: 'ping', pct: pings.length / 30, value: t }); }));
        await new Promise((r) => setTimeout(r, 80));
      }
      await Promise.all(jobs);
      let ping = summarize(pings);
      if (ping.loss === 100) { // ICMP blocked, fall back to TCP handshakes
        const tcp = [];
        for (let i = 0; i < 10; i++) tcp.push(await tcpPing());
        ping = summarize(tcp);
      }

      // 2) Download: 6 parallel streams for 10 s
      // 10 MB chunks: the server can stall on very large single requests
      const download = await this.measure('download', 10000, 6, async (signal, add) => {
        const r = await fetch(`https://speed.cloudflare.com/__down?bytes=10000000&r=${Math.random()}`, { signal, cache: 'no-store' });
        if (!r.ok) throw new Error(`HTTP ${r.status}`);
        const reader = r.body.getReader();
        for (;;) { const { done, value } = await reader.read(); if (done) break; add(value.byteLength); }
      }, progress);

      // 3) Upload: 4 parallel streams of 1 MB posts for 8 s
      const chunk = crypto.randomBytes(1 << 20);
      const upload = await this.measure('upload', 8000, 4, async (signal, add) => {
        const r = await fetch('https://speed.cloudflare.com/__up', { method: 'POST', body: chunk, signal, headers: { 'Content-Type': 'application/octet-stream' } });
        if (!r.ok) throw new Error(`HTTP ${r.status}`);
        add(chunk.length);
      }, progress);
      // A phase that moved no data at all failed; don't record it as "0 Mbps"
      if (!download) throw new Error('The download test got no data from the server. Try again in a minute.');
      if (!upload) throw new Error('The upload test could not reach the server. Try again in a minute.');

      const result = {
        t: Date.now(),
        ping: ping.avg, jitter: ping.jitter, loss: ping.loss,
        down: download, up: upload,
        server: meta.city ? `${meta.city} (${meta.colo})` : 'Cloudflare',
        isp: meta.asOrganization || '',
      };
      progress({ phase: 'done', result });
      return result;
    } finally {
      this.testing = false;
    }
  }

  // Runs `streams` loops of `worker` until `ms` elapses. Returns Mbps, ignoring the first 1.5 s ramp-up.
  async measure(phase, ms, streams, worker, progress) {
    const ctrl = new AbortController();
    const start = Date.now();
    let bytes = 0, steadyBytes = 0, steadyStart = null;
    const window = [];
    const add = (b) => {
      bytes += b;
      const now = Date.now();
      if (now - start > 1500) { if (steadyStart === null) steadyStart = now; steadyBytes += b; }
      window.push([now, b]);
    };
    const tick = setInterval(() => {
      const now = Date.now();
      while (window.length && now - window[0][0] > 1000) window.shift();
      const recent = window.reduce((a, [, b]) => a + b, 0);
      progress({ phase, pct: Math.min(1, (now - start) / ms), mbps: (recent * 8) / 1e6 });
    }, 150);
    const loops = Array.from({ length: streams }, async () => {
      while (!ctrl.signal.aborted) {
        try { await worker(ctrl.signal, add); } catch { if (!ctrl.signal.aborted) await new Promise((r) => setTimeout(r, 200)); }
      }
    });
    await new Promise((r) => setTimeout(r, ms));
    ctrl.abort();
    clearInterval(tick);
    await Promise.allSettled(loops);
    const end = Date.now();
    if (steadyStart && end - steadyStart > 500) return (steadyBytes * 8) / ((end - steadyStart) / 1000) / 1e6;
    return (bytes * 8) / ((end - start) / 1000) / 1e6;
  }
}

module.exports = NetMon;
