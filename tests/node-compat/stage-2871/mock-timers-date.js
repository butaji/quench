const assert = require('assert');
const { test } = require('node:test');

test('node:test Date mock uses one virtual clock', (t) => {
  t.mock.timers.enable({ apis: ['Date'], now: 100 });
  assert.strictEqual(Date.now(), 100);
  t.mock.timers.tick(25);
  assert.strictEqual(new Date().getTime(), 125);
  t.mock.timers.setTime(7);
  assert.strictEqual(Date.now(), 7);
  t.mock.timers.reset();
});
