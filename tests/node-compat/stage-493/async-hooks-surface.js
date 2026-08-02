const assert = require("assert");
const asyncHooks = require("async_hooks");

assert.ok(asyncHooks.executionAsyncResource());
assert.strictEqual(typeof asyncHooks.executionAsyncId(), "number");
const hook = asyncHooks.createHook({});
assert.strictEqual(hook.enable(), hook);
assert.strictEqual(hook.disable(), hook);
