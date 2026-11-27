module.exports = require(process.platform === 'darwin' ? './macos/native' : './native-windows');
