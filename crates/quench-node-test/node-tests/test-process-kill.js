const assert = require('assert');
const calls = [];
const original = process._kill;
process._kill = (pid, signal) => calls.push([pid, signal]);
assert.strictEqual(process.kill('0', 'SIGHUP'), true);
assert.deepStrictEqual(calls[0], [0, 1]);
assert.throws(() => process.kill(0, 'not-a-signal'), { code: 'ERR_UNKNOWN_SIGNAL' });
assert.throws(() => process.kill(0, 987), { code: 'EINVAL' });
process._kill = original;
