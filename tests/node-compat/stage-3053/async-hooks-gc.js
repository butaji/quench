'use strict';

const async_hooks = require('async_hooks');
if (typeof globalThis.gc !== 'function') throw new Error('gc unavailable');
const destroyed = new Set();
const hook = async_hooks.createHook({
  destroy(asyncId) {
    destroyed.add(asyncId);
  }
}).enable();

let automaticId;
{
  const automatic = new async_hooks.AsyncResource('automatic');
  automaticId = automatic.asyncId();
}

setImmediate(() => {
  globalThis.gc();
  setImmediate(() => {
    hook.disable();
    if (!destroyed.has(automaticId)) {
      throw new Error('async resource destroy lifecycle incomplete');
    }
  });
});
