const fs = require('fs');
const path = require('path');
const { app } = require('electron');

const DEFAULTS = {
  openAtLogin: true,
  idleOnFullscreen: true,
  chatMode: 'auto',
  collapseOnBlur: true,
  hideSidesUntilHover: false,
  autoLimits: true,
  claudeLimits: null,
  codexLimits: null,
  chatProvider: 'auto',
  codexFromEditor: false,
  extremeMode: false,
  extremeBackup: null,
  speedHistory: [],
};

const file = () => path.join(app.getPath('userData'), 'settings.json');

function load() {
  // Strip a byte-order mark (some editors add one) so a hand-edited file still loads
  try { return { ...DEFAULTS, ...JSON.parse(fs.readFileSync(file(), 'utf8').replace(/^﻿/, '')) }; } catch { return { ...DEFAULTS }; }
}

function save(s) {
  try { fs.mkdirSync(path.dirname(file()), { recursive: true }); fs.writeFileSync(file(), JSON.stringify(s, null, 2)); } catch {}
}

module.exports = { load, save };
