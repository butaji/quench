const assert = require('assert');
const async_hooks = require('async_hooks');

const ids = [];
async_hooks.createHook({
  init(id, type, trigger) {
    if (type === 'PROMISE') {
      assert.strictEqual(trigger, ids[ids.length - 1] || 1);
      ids.push(id);
    }
  },
  before(id) { assert.strictEqual(id, ids[1]); },
  after(id) { assert.strictEqual(id, ids[1]); },
}).enable();

Promise.resolve(42).then(() => {
  assert.strictEqual(async_hooks.executionAsyncId(), ids[1]);
  assert.strictEqual(async_hooks.triggerAsyncId(), ids[0]);
  Promise.resolve(10);
});
