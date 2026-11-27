const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');
if (process.platform !== 'darwin') throw new Error('Build the macOS app on a Mac');
const root = path.resolve(__dirname,'..');
const dest = path.resolve(process.argv[2] || path.join(root,'dist','Mission Control.app'));
if (fs.existsSync(dest)) throw new Error(`Destination exists: ${dest}`);
const electron = require('electron');
const original = path.resolve(electron, '../../..');
fs.mkdirSync(path.dirname(dest),{ recursive: true });
execFileSync('/usr/bin/ditto',[original,dest]);
const resources = path.join(dest,'Contents/Resources');
fs.rmSync(path.join(resources,'default_app.asar'),{ force: true });
const app = path.join(resources,'app'); fs.mkdirSync(app);
for (const name of ['main.js','preload.js','package.json','renderer','src','LICENSE','MACOS.md']) fs.cpSync(path.join(root,name),path.join(app,name),{recursive:true,filter:p=>!p.endsWith('/macos/helper')});
// The Mac backend uses built-in modules only; koffi remains a Windows-only optional dependency.
const plist = path.join(dest,'Contents/Info.plist');
for (const [key,value] of Object.entries({ CFBundleName:'Mission Control', CFBundleDisplayName:'Mission Control', CFBundleIdentifier:'com.missioncontrol.widget', CFBundleShortVersionString:'1.0.0', NSAppleEventsUsageDescription:'Control appearance, media playback and session actions when you request them.', NSLocationUsageDescription:'Show the current Wi-Fi network and nearby networks.', NSBluetoothAlwaysUsageDescription:'Show paired Bluetooth devices and their status.' })) {
  try { execFileSync('/usr/libexec/PlistBuddy',['-c',`Set :${key} ${value}`,plist],{stdio:'pipe'}); }
  catch { execFileSync('/usr/libexec/PlistBuddy',['-c',`Add :${key} string ${value}`,plist]); }
}
execFileSync('/usr/libexec/PlistBuddy',['-c','Add :LSUIElement bool true',plist]);
execFileSync('/usr/bin/xattr',['-cr',dest]);
execFileSync('/usr/bin/codesign',['--force','--deep','--sign','-',dest],{stdio:'inherit'});
console.log(dest);
