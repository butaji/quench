const assert = require('assert');
assert.strictEqual(typeof process.getuid(), 'number');
assert.strictEqual(typeof process.geteuid(), 'number');
assert.strictEqual(typeof process.getgid(), 'number');
assert.strictEqual(typeof process.getegid(), 'number');
