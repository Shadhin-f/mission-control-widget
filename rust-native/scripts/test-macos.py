#!/usr/bin/env python3
"""Read-only smoke checks for a packaged native macOS app; no login items or system changes."""
import argparse
import json
import os
from pathlib import Path
import plistlib
import subprocess
import tempfile
import time

parser = argparse.ArgumentParser()
parser.add_argument('app', type=Path)
parser.add_argument('--report-dir', type=Path, required=True)
a = parser.parse_args()
bundle = a.app.resolve()
report = a.report_dir.resolve()
report.mkdir(parents=True, exist_ok=True)
expected = ['collapsed', 'hover', 'overview', 'controls', 'network', 'apps', 'cleaner', 'settings', 'credits', 'chat']
for name in ['done.txt', 'result.json', *[f'{screen}.png' for screen in expected]]:
    (report / name).unlink(missing_ok=True)
contents = bundle / 'Contents'
info = plistlib.loads((contents / 'Info.plist').read_bytes())
assert info['CFBundleIdentifier'] == 'com.missioncontrol.widget.native'
assert info['LSUIElement'] is True
exe = contents / 'MacOS' / info['CFBundleExecutable']
helper = contents / 'Resources/MissionControlHelper.app/Contents/MacOS/helper'
subprocess.run(['/usr/bin/codesign', '--verify', '--deep', '--strict', str(bundle)], check=True)
libs = subprocess.check_output(['/usr/bin/otool', '-L', str(exe)], text=True)
assert 'Electron' not in libs and 'WebKit' not in libs, libs
for cmd, field in [('state', 'audio'), ('gpu', 'name'), ('windows', 'fullscreen'), ('brightness', 'monitors'), ('wifi', 'available'), ('bt', 'devices')]:
    result = subprocess.run([str(helper), cmd, '{}'], capture_output=True, text=True, check=True, timeout=20)
    data = json.loads(result.stdout)
    assert field in data, (cmd, data)
    print(f'PASS helper {cmd}')
with tempfile.TemporaryDirectory(prefix='mission-control-native-test-') as profile:
    env = {**os.environ, 'MC_SETTINGS_DIR': profile, 'MC_NO_LOGIN': '1', 'MC_DIAG': '1',
           'MC_SHOTS': str(report), 'MC_DEV_QUIT_AFTER': '55'}
    with (report / 'launch.log').open('w') as log:
        process = subprocess.Popen([str(exe)], env=env, stdout=log, stderr=subprocess.STDOUT)
        try:
            process.wait(timeout=80)
        except subprocess.TimeoutExpired:
            process.terminate()
            process.wait(timeout=10)
            raise AssertionError('Native app did not exit on its development timer')
    assert process.returncode == 0, (report / 'launch.log').read_text()
assert (report / 'done.txt').exists(), 'Screenshot sequence did not complete'
for name in expected:
    file = report / f'{name}.png'
    assert file.read_bytes().startswith(b'\x89PNG\r\n\x1a\n') and file.stat().st_size > 1000, name
    print(f'PASS screenshot {name}')
summary = {'passed': True, 'helper_queries': 6, 'screenshots': expected, 'app_exit_code': process.returncode,
           'system_settings_changed': False, 'architecture': os.uname().machine}
(report / 'result.json').write_text(json.dumps(summary, indent=2) + '\n')
print('PASS packaged pure Rust app: native helpers, all screens, normal exit')
