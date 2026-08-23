const assert = require('assert');
assert.strictEqual(process.exitCode, undefined);
process.exitCode = 7;
assert.strictEqual(process.exitCode, 7);
process.exitCode = undefined;
assert.strictEqual(process.exitCode, undefined);
