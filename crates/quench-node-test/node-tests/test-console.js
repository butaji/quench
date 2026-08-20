// Node compat: console.warn + console.error + console.info.
const console = require('node:console');
console.warn('warn: hello');
console.error('error: hello');
console.info('info: hello');
console.log('log: hello');
console.debug('debug: hello');
console.trace('trace: hello');
