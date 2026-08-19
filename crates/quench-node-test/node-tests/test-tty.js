// Node compat: tty module.
const tty = require('node:tty');
const fd = 0;
if (typeof tty.isatty(fd) !== 'boolean') throw new Error('isatty: ' + typeof tty.isatty(fd));
// We don't assert the value (CI vs interactive may differ).
console.log('tty: %s', tty.isatty(fd));
