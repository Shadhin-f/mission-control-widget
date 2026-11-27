const fs = require('fs');
const path = require('path');
const { app } = require('electron');

const DEFAULTS = {
  openAtLogin: true,
  idleOnFullscreen: true,
  chatMode: 'auto',
  collapseOnBlur: true,
  autoLimits: true,
  claudeLimits: null,
  speedHistory: [],
};

const file = () => path.join(app.getPath('userData'), 'settings.json');

function load() {
  try { return { ...DEFAULTS, ...JSON.parse(fs.readFileSync(file(), 'utf8')) }; } catch { return { ...DEFAULTS }; }
}

function save(s) {
  try { fs.mkdirSync(path.dirname(file()), { recursive: true }); fs.writeFileSync(file(), JSON.stringify(s, null, 2)); } catch {}
}

module.exports = { load, save };
