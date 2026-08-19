// Node compat: test stub.
const test = require('node:test');
if (typeof test !== 'object') throw new Error('test: ' + typeof test);
if (typeof test.test !== 'function') throw new Error('test.test: ' + typeof test.test);
console.log('test: ok');
