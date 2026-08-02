const assert = require('assert');
assert.strictEqual(typeof console.count, 'function');
assert.strictEqual(typeof console.countReset, 'function');
console.countReset('stage-51');
console.count('stage-51');
console.countReset('stage-51');
console.clear();
