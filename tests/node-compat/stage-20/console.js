const assert = require('assert');
assert.strictEqual(typeof console.log, 'function');
assert.strictEqual(typeof console.error, 'function');
console.log('stage-20', 42);
console.assert(true, 'not printed');
