const { execFile } = require('child_process');
const path = require('path');
function run(file, args = [], timeout = 15000) {
  return new Promise((resolve, reject) => execFile(file, args, { timeout, maxBuffer: 16 << 20 }, (e, out, err) => e ? reject(new Error(String(err || e.message).trim())) : resolve(out.trim())));
}
const helper = (cmd, args = {}, scan = false) => run(path.join(__dirname, 'MissionControlHelper.app/Contents/MacOS/helper'), [cmd, JSON.stringify(args), ...(scan ? ['scan'] : [])], scan ? 25000 : 15000).then(JSON.parse);
const apple = (script) => run('/usr/bin/osascript', ['-e', script]);
module.exports = { run, helper, apple };
