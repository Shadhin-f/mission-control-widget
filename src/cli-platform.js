const fs = require('fs');
const os = require('os');
const path = require('path');
function findUnixCli(name) {
  const dirs = [...(process.env.PATH || '').split(path.delimiter), path.join(os.homedir(), '.local/bin'), path.join(os.homedir(), '.npm-global/bin'), '/opt/homebrew/bin', '/usr/local/bin', '/usr/bin'];
  for (const dir of dirs) { if (!dir) continue; const p = path.join(dir,name); try { fs.accessSync(p,fs.constants.X_OK); return p; } catch {} }
  return null;
}
function stopTree(child) {
  if (process.platform === 'win32') require('child_process').execFile('taskkill',['/PID',String(child.pid),'/T','/F'],{ windowsHide: true },() => {});
  else { try { process.kill(-child.pid, 'SIGTERM'); } catch { try { child.kill('SIGTERM'); } catch {} } }
}
module.exports = { findUnixCli, stopTree };
