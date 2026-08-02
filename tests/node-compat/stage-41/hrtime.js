const assert = require('assert');
const start = process.hrtime();
const elapsed = process.hrtime(start);
assert.strictEqual(Array.isArray(elapsed), true);
assert.strictEqual(elapsed.length, 2);
assert.strictEqual(typeof process.hrtime.bigint(), 'bigint');
