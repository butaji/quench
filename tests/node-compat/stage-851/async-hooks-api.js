"use strict";

const assert = require("assert");
const asyncHooks = require("node:async_hooks");

for (
  const name of [
    "createHook",
    "executionAsyncId",
    "triggerAsyncId",
    "executionAsyncResource",
  ]
) {
  assert.strictEqual(typeof asyncHooks[name], "function");
}
assert.strictEqual(typeof asyncHooks.AsyncResource, "function");
assert.strictEqual(typeof asyncHooks.AsyncLocalStorage, "function");
assert.strictEqual(typeof asyncHooks.executionAsyncId(), "number");

console.log("async hooks api passed");
