module.exports = require(process.platform === 'darwin' ? './macos/processes' : './processes-windows');
