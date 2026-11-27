module.exports = require(process.platform === 'darwin' ? './macos/syscontrol' : './syscontrol-windows');
