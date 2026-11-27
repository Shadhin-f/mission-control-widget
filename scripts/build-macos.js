const { execFileSync } = require('child_process');
const path = require('path');
const fs = require('fs');
const os = require('os');
if (process.platform === 'darwin') {
  const stage = fs.mkdtempSync(path.join(os.tmpdir(), 'mission-control-build-'));
  const bundle = path.join(stage, 'MissionControlHelper.app');
  const contents = path.join(bundle, 'Contents');
  fs.mkdirSync(path.join(contents, 'MacOS'), { recursive: true });
  fs.writeFileSync(path.join(contents, 'Info.plist'), `<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>com.missioncontrol.helper</string><key>CFBundleName</key><string>Mission Control Helper</string><key>CFBundleExecutable</key><string>helper</string><key>CFBundleVersion</key><string>1</string><key>LSUIElement</key><true/><key>NSLocationUsageDescription</key><string>Show nearby Wi-Fi networks and the current network name.</string><key>NSLocationWhenInUseUsageDescription</key><string>Show nearby Wi-Fi networks and the current network name.</string></dict></plist>`);
  execFileSync('/usr/bin/xcrun', ['swiftc', '-O', path.join(__dirname, '../src/macos/Helper.swift'), '-o', path.join(contents, 'MacOS/helper')], { stdio: 'inherit' });
  execFileSync('/usr/bin/xattr', ['-cr', bundle]);
  execFileSync('/usr/bin/codesign', ['--force', '--sign', '-', '--identifier', 'com.missioncontrol.helper', bundle], { stdio: 'inherit' });
  const destination = path.join(__dirname, '../src/macos/MissionControlHelper.app');
  fs.rmSync(destination, { recursive: true, force: true });
  execFileSync('/usr/bin/ditto', ['--norsrc', '--noextattr', bundle, destination]);
  fs.rmSync(stage, { recursive: true, force: true });
}
