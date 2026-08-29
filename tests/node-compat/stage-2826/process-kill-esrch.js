const assert = require('assert');

assert.strictEqual(typeof process.kill, 'function');
assert.throws(() => process.kill(0), /^Error: kill ESRCH$/);
assert.strictEqual(process.kill(process.pid), true);
