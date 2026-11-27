// Live system stats: CPU, RAM, GPU (nvidia-smi stream), disks, network throughput.
const os = require('os');
const fs = require('fs');
const { spawn, execFileSync } = require('child_process');
const { EventEmitter } = require('events');
const native = require('./native');

function hasNvidiaSmi() {
  try { execFileSync('where', ['nvidia-smi'], { windowsHide: true, stdio: 'ignore' }); return true; } catch { return false; }
}

class Stats extends EventEmitter {
  constructor() {
    super();
    this.timer = null;
    this.diskTimer = null;
    this.gpuProc = null;
    this.gpu = null;
    this.disks = [];
    this.prevCpu = null;
    this.prevNet = null;
    this.adapter = null;
    this.nvidia = hasNvidiaSmi();
    this.static = {
      cpuModel: (os.cpus()[0]?.model || 'CPU').replace(/\s+/g, ' ').trim(),
      cores: os.cpus().length,
      host: os.hostname(),
      os: `Windows ${os.release()}`,
      totalMem: os.totalmem(),
    };
  }

  start() {
    if (this.timer) return;
    this.prevCpu = os.cpus().map((c) => c.times);
    this.prevNet = { ...native.netCounters(), t: Date.now() };
    this.readDisks();
    this.diskTimer = setInterval(() => this.readDisks(), 15000);
    this.startGpu();
    this.timer = setInterval(() => this.tick(), 1000);
  }

  stop() {
    clearInterval(this.timer); this.timer = null;
    clearInterval(this.diskTimer); this.diskTimer = null;
    if (this.gpuProc) { this.gpuProc.kill(); this.gpuProc = null; }
  }

  startGpu() {
    if (!this.nvidia || this.gpuProc) return;
    const p = spawn('nvidia-smi', [
      '--query-gpu=name,utilization.gpu,memory.used,memory.total,temperature.gpu,power.draw,fan.speed',
      '--format=csv,noheader,nounits', '-lms', '1000',
    ], { windowsHide: true });
    this.gpuProc = p;
    let buf = '';
    p.stdout.on('data', (d) => {
      buf += d;
      let i;
      while ((i = buf.indexOf('\n')) >= 0) {
        const line = buf.slice(0, i).trim(); buf = buf.slice(i + 1);
        const f = line.split(',').map((s) => s.trim());
        if (f.length < 5) continue;
        const num = (s) => (isFinite(parseFloat(s)) ? parseFloat(s) : null);
        this.gpu = { name: f[0], util: num(f[1]), vramUsed: num(f[2]), vramTotal: num(f[3]), temp: num(f[4]), power: num(f[5]), fan: num(f[6]) };
      }
    });
    p.on('exit', () => { if (this.gpuProc === p) this.gpuProc = null; });
    p.on('error', () => { this.nvidia = false; this.gpuProc = null; });
  }

  readDisks() {
    const out = [];
    for (let c = 67; c <= 90; c++) { // C..Z
      const root = `${String.fromCharCode(c)}:\\`;
      try {
        const s = fs.statfsSync(root);
        const total = s.blocks * s.bsize;
        if (!total) continue;
        out.push({ drive: root.slice(0, 2), total, free: s.bavail * s.bsize });
      } catch {}
    }
    this.disks = out;
  }

  tick() {
    const cpus = os.cpus();
    const cores = cpus.map((c, i) => {
      const p = this.prevCpu[i] || c.times;
      const idle = c.times.idle - p.idle;
      const total = Object.keys(c.times).reduce((a, k) => a + (c.times[k] - p[k]), 0);
      return total > 0 ? Math.max(0, Math.min(100, 100 * (1 - idle / total))) : 0;
    });
    this.prevCpu = cpus.map((c) => c.times);
    const cpu = cores.reduce((a, b) => a + b, 0) / cores.length;

    const now = Date.now();
    const n = native.netCounters();
    let down = 0, up = 0;
    if (n && this.prevNet && this.prevNet.rx != null) {
      const dt = (now - this.prevNet.t) / 1000;
      down = Math.max(0, (n.rx - this.prevNet.rx) / dt);
      up = Math.max(0, (n.tx - this.prevNet.tx) / dt);
    }
    if (n) { this.prevNet = { ...n, t: now }; this.adapter = n.adapter; }

    const total = os.totalmem(), free = os.freemem();
    this.emit('stats', {
      t: now,
      cpu, cores, cpuMhz: cpus[0]?.speed || 0,
      mem: { total, used: total - free },
      gpu: this.gpu,
      disks: this.disks,
      net: { down, up, adapter: this.adapter },
      uptime: os.uptime(),
    });
  }
}

module.exports = Stats;
