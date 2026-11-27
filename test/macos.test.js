const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const os = require('node:os');
const path = require('node:path');
const { helper } = require('../src/macos/run');
const { agedFiles } = require('../src/cleaner');
const { tasklist } = require('../src/macos/processes');
const { findUnixCli } = require('../src/cli-platform');

test('native macOS queries return usable audio, GPU, display and window data', { skip: process.platform !== 'darwin' }, async () => {
  const s = await helper('state');
  assert.ok(s.audio.out.length); assert.ok(s.audio.in.length);
  assert.ok(s.audio.out.some(d => d.isDefault));
  assert.ok(s.radios.some(r => r.kind === 'WiFi'));
  assert.equal(typeof (await helper('windows')).fullscreen,'boolean');
  assert.ok((await helper('gpu')).name);
  for (const m of (await helper('brightness')).monitors) assert.ok(m.cur >= m.min && m.cur <= m.max);
  assert.equal(typeof (await helper('wifi')).available,'boolean');
  assert.ok(Array.isArray((await helper('bt')).devices));
});
test('process list reports this process and actual resident memory', { skip: process.platform !== 'darwin' }, async () => {
  const rows = await tasklist(); const own = rows.find(p => p.pid === process.pid);
  assert.ok(own); assert.ok(own.mem > 0); assert.equal(own.uid,process.getuid());
});
test('temporary cleaner preserves young nested files and symlink targets', { skip: process.platform !== 'darwin' }, async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(),'mc-clean-test-'));
  try {
    const dir = path.join(root,'nested'); await fs.mkdir(dir);
    const old = path.join(dir,'old'), young = path.join(dir,'young'), target = path.join(root,'target');
    await fs.writeFile(old,'old'); await fs.writeFile(young,'new'); await fs.writeFile(target,'safe');
    await fs.symlink(target,path.join(dir,'link'));
    const past = new Date(Date.now()-3*3600000); await fs.utimes(old,past,past); await fs.utimes(dir,past,past);
    const scan = await agedFiles(dir,2*3600000); assert.equal(scan.size,3); assert.equal(scan.files,1);
    await agedFiles(dir,2*3600000,true);
    await assert.rejects(fs.stat(old)); assert.equal(await fs.readFile(young,'utf8'),'new'); assert.equal(await fs.readFile(target,'utf8'),'safe');
  } finally { await fs.rm(root,{ recursive:true,force:true }); }
});
test('Unix CLI discovery uses executable PATH entries', { skip: process.platform !== 'darwin' }, async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(),'mc-cli-test-')); const before = process.env.PATH;
  try { const p = path.join(root,'mission-test'); await fs.writeFile(p,'#!/bin/sh\n'); await fs.chmod(p,0o755); process.env.PATH = root; assert.equal(findUnixCli('mission-test'),p); } finally { process.env.PATH = before; await fs.rm(root,{recursive:true,force:true}); }
});
test('invalid native commands and invalid audio IDs fail explicitly', { skip: process.platform !== 'darwin' }, async () => {
  await assert.rejects(helper('unknown'));
  await assert.rejects(helper('setVolume',{ id:'invalid',value:10 }));
});
