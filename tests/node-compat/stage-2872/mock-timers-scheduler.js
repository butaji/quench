const assert = require('assert');
const timers = require('timers/promises');
const { test } = require('node:test');

test('scheduler mock ticks deadlines without changing Date', async (t) => {
  t.mock.timers.enable({ apis: ['scheduler.wait'] });
  const before = Date.now();
  const pending = timers.scheduler.wait(1000);
  t.mock.timers.tick(1000);
  assert.strictEqual(await pending, undefined);
  assert.ok(Date.now() - before < 100);
});
