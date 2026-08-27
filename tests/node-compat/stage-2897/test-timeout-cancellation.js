const assert = require('assert');
const { test } = require('node:test');
const { setTimeout } = require('timers/promises');

let signal;
test({ timeout: 10 }, async ({ signal: testSignal }) => {
  signal = testSignal;
  assert.strictEqual(signal.aborted, false);
  await setTimeout(50);
}).finally(() => {
  test(() => assert.strictEqual(signal.aborted, true));
});
