// Live system stats: CPU, RAM, GPU (nvidia-smi stream), disks, network throughput.
const os = require('os');
const fs = require('fs');
const { spawn } = require('child_process');
const { EventEmitter } = require('events');
const native = require('./native');

// Non-blocking check (no child process): look for nvidia-smi on PATH
function hasNvidiaSmi() {
  const path = require('path');
  return (process.env.PATH || '').split(';').some((d) => d && fs.existsSync(path.join(d, 'nvidia-smi.exe')));
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
      os: `${process.platform === 'darwin' ? 'macOS' : 'Windows'} ${os.release()}`,
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
    if (process.platform === 'darwin') {
      const { helper, run } = require('./macos/run');
      run('/usr/bin/sw_vers', ['-productVersion']).then(v => { this.static.os = `macOS ${v}`; }).catch(() => {});
      helper('gpu').then(g => { this.gpu = { name: g.name, unified: g.unified, util: null, vramUsed: null, vramTotal: null, temp: null, power: null }; }).catch(() => {});
      const read = async () => {
        if (this.macReading) return; this.macReading = true;
        try {
          const text = await run('/usr/sbin/ioreg', ['-r', '-c', 'IOAccelerator', '-l']);
          const match = text.match(/"Device Utilization %"\s*=\s*(\d+)/);
          if (this.gpu) this.gpu.util = match ? Number(match[1]) : null;
          const vm = await run('/usr/bin/vm_stat');
          const page = Number(vm.match(/page size of (\d+)/)?.[1]) || 16384;
          const n = key => Number(vm.match(new RegExp(key + ':\\s+(\\d+)'))?.[1]) || 0;
          this.macFree = (n('Pages free') + n('Pages inactive') + n('Pages speculative')) * page;
        } catch {} finally { this.macReading = false; }
      };
      read(); this.macTimer = setInterval(read, 5000);
    }
    this.timer = setInterval(() => this.tick(), 1000);
  }

  stop() {
    clearInterval(this.macTimer);
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
    if (process.platform === 'darwin') {
      const roots = ['/'];
      try { roots.push(...fs.readdirSync('/Volumes').map(n => require('path').join('/Volumes', n)).filter(p => !fs.lstatSync(p).isSymbolicLink() && !['Recovery','Preboot','VM','Update'].includes(require('path').basename(p)))); } catch {}
      for (const root of roots) { try { const s = fs.statfsSync(root); if (s.blocks) out.push({ drive: root, total: s.blocks*s.bsize, free: s.bavail*s.bsize }); } catch {} }
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

    const total = os.totalmem(), free = this.macFree ?? os.freemem();
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
