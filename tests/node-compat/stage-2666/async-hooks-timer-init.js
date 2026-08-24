"use strict";
const assert = require("assert");
const async_hooks = require("async_hooks");

let inits = 0;
const hook = async_hooks.createHook({ init() { inits++; } });
assert.strictEqual(hook.enable(), hook);
assert.strictEqual(hook.enable(), hook);
setImmediate(() => {
  assert.strictEqual(inits, 1);
});
assert.strictEqual(hook.disable(), hook);
assert.strictEqual(hook.disable(), hook);
