const assert = require('assert');
const { scheduler } = require('timers/promises');

assert.throws(() => scheduler.yield.call({}), { code: 'ERR_INVALID_THIS' });
assert.throws(() => scheduler.wait.call({}, 1), { code: 'ERR_INVALID_THIS' });
assert.throws(() => new scheduler.constructor(), { code: 'ERR_ILLEGAL_CONSTRUCTOR' });
(async () => {
  let value = 0;
  setTimeout(() => value++, 1);
  await scheduler.wait(2);
  assert.strictEqual(value, 1);
  const ac = new AbortController();
  const pending = scheduler.wait(1000, { signal: ac.signal });
  ac.abort();
  await assert.rejects(pending, { code: 'ABORT_ERR' });
})();
