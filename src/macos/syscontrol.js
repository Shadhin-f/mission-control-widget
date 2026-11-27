const { helper, run, apple } = require('./run');
const { shell } = require('electron');
const fs = require('fs');
const blueutil = ['/opt/homebrew/bin/blueutil','/usr/local/bin/blueutil'].find(p => fs.existsSync(p));
const quote = v => JSON.stringify(String(v));
class SysControl {
  stop() {}
  async helper(cmd, a = {}) {
    if (['requestLocation','state','bt','brightness','setVolume','setMute','setDefault','setBrightness'].includes(cmd)) {
      const value = await helper(cmd,a);
      if (cmd === 'state') value.media = await this.mediaState();
      return value;
    }
    if (cmd === 'stepVolume') {
      const s = await helper('state'); const d = s.audio.out.find(d => d.isDefault);
      if (!d) throw new Error('No output device');
      const value = Math.max(0,Math.min(100,d.volume + Number(a.delta || 0)));
      await helper('setVolume',{ id: d.id, value }); return value;
    }
    if (cmd === 'setDark') { await apple(`tell application "System Events" to tell appearance preferences to set dark mode to ${a.dark ? 'true' : 'false'}`); return { ok: true }; }
    if (cmd === 'setRadio') {
      if (a.kind === 'WiFi') return helper('setWifi',{ on: !!a.on });
      if (a.kind !== 'Bluetooth') throw new Error('Unknown radio');
      if (!blueutil) { await shell.openExternal('x-apple.systempreferences:com.apple.BluetoothSettings'); throw new Error('Use Bluetooth Settings to change power. Install blueutil for the in-widget toggle.'); }
      await run(blueutil,['--power',a.on ? '1':'0']); return { ok: true };
    }
    if (cmd === 'media') {
      const state = await this.mediaState();
      if (!state) return helper('media', a);
      const action = { toggle: 'playpause', next: 'next track', prev: 'previous track' }[a.action];
      if (!action) throw new Error('Unknown media action');
      await apple(`tell application ${quote(state.app)} to ${action}`); return { ok: true };
    }
    if (cmd === 'lock') { await run('/System/Library/CoreServices/Menu Extras/User.menu/Contents/Resources/CGSession',['-suspend']); return { ok: true }; }
    if (cmd === 'sleep') { await run('/usr/bin/pmset',['sleepnow']); return { ok: true }; }
    throw new Error(`Unsupported command: ${cmd}`);
  }
  async mediaState() {
    // Check running apps first, so polling never launches a player or prompts for unrelated apps.
    const running = await run('/bin/ps',['-axo','comm=']);
    for (const app of ['Spotify','Music']) {
      if (!running.split('\n').some(p => p.endsWith(`/${app}.app/Contents/MacOS/${app}`))) continue;
      try {
        const out = await apple(`tell application "${app}"\nreturn (player state as text) & linefeed & (name of current track) & linefeed & (artist of current track)\nend tell`);
        const [status,title,artist] = out.split('\n'); return { app, title, artist, status: status === 'playing' ? 'Playing' : 'Paused' };
      } catch { /* Automation permission denied or no active track */ }
    }
    return null;
  }
  wifi() {
    if (this.wifiPending) return this.wifiPending;
    const scan = !this.lastWifiScan || Date.now()-this.lastWifiScan > 30000;
    if (scan) this.lastWifiScan = Date.now();
    this.wifiPending = helper('wifi',{},scan).catch(() => helper('wifi')).then(w => {
      if (scan) this.networks = w.networks;
      else w.networks = this.networks || [];
      return w;
    }).finally(() => { this.wifiPending = null; });
    return this.wifiPending;
  }
  async wifiConnect(ssid) {
    if (!ssid || ssid.startsWith('-')) throw new Error('Invalid network name');
    const w = await helper('wifi');
    try { await run('/usr/sbin/networksetup',['-setairportnetwork',w.interface,ssid]); return { ok: true }; } catch(e) { return { ok: false, message: `${e.message}. Connect using Wi-Fi Settings if a password is required.` }; }
  }
  async wifiDisconnect() {
    // CoreWLAN disconnects without changing radio power.
    return helper('disconnectWifi');
  }
  async powerPlans() {
    const p = await run('/usr/bin/pmset',['-g','custom']);
    const low = /lowpowermode\s+1/.test(p);
    return [{ guid: 'auto', name: 'Automatic', active: !low }, { guid: 'low', name: 'Low Power', active: low }];
  }
  async setPowerPlan(guid) {
    if (!['auto','low'].includes(guid)) throw new Error('Unknown energy mode');
    await apple(`do shell script "/usr/bin/pmset -a lowpowermode ${guid === 'low' ? '1' : '0'}" with administrator privileges`);
    return { ok: true };
  }

  async session(kind) {
    const action = { restart: 'restart', shutdown: 'shut down', signout: 'log out' }[kind];
    if (!action) throw new Error('Unknown session action');
    await apple(`tell application "System Events" to ${action}`); return { ok: true };
  }
}
module.exports = SysControl;
