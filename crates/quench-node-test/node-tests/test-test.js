// Node compat: test stub.
const test = require('node:test');
if (typeof test !== 'function') throw new Error('test: ' + typeof test);
if (typeof test.test !== 'function') throw new Error('test.test: ' + typeof test.test);
if (typeof test.skip !== 'function') throw new Error('test.skip: ' + typeof test.skip);
console.log('test: ok');
