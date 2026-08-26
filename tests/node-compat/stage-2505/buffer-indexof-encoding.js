const assert = require('node:assert');
const buffer = Buffer.from('abcdef');
assert.strictEqual(buffer.indexOf('a'), 0);
assert.strictEqual(buffer.indexOf('bc'), 1);
assert.throws(() => buffer.indexOf('bad', 'enc'), /Unknown encoding: enc/);
for (const value of [{}, []]) {
  assert.throws(() => buffer.indexOf(value), {
    code: 'ERR_INVALID_ARG_TYPE',
    name: 'TypeError',
  });
}
