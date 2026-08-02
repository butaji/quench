const assert = require('assert');
assert.strictEqual(Array.isArray(process.argv), true);
assert.strictEqual(process.argv.length >= 1, true);
assert.strictEqual(typeof process.argv[0], 'string');
