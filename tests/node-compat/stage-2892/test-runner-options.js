const assert = require('assert');
const test = require('node:test');

for (const timeout of [-1, -Infinity, NaN, 2 ** 33]) {
  assert.throws(() => test({ timeout }), { code: 'ERR_OUT_OF_RANGE' });
}
for (const concurrency of [-1, 0, 1.1, -Infinity, NaN, 2 ** 33]) {
  assert.throws(() => test({ concurrency }), { code: 'ERR_OUT_OF_RANGE' });
}
test({ timeout: Infinity });
test({ timeout: 0 });
test({ concurrency: true });
