const assert = require('assert');
const asyncHooks = require('async_hooks');

let promiseInits = 0;
const hook = asyncHooks.createHook({
  init(_id, type) {
    if (type === 'PROMISE') promiseInits += 1;
  },
});

hook.enable();
new Promise((resolve) => resolve(1));
new Promise((resolve) => resolve(2));
hook.disable();
process.on('exit', () => assert.strictEqual(promiseInits, 2));
