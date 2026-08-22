const assert = require('assert');
const nodeTest = require('node:test');

assert.strictEqual(typeof nodeTest.test, 'function');
assert.strictEqual(typeof nodeTest.before, 'function');
assert.strictEqual(typeof nodeTest.after, 'function');
console.log('node test exports: ok');
