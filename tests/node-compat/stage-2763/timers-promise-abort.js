"use strict";
const assert = require("assert");
const { setTimeout } = require("timers/promises");
const controller = new AbortController();
const pending = setTimeout(100, "bad", { signal: controller.signal });
controller.abort();
pending.then(() => assert.fail("resolved"), (error) => {
  assert.strictEqual(error.name, "AbortError");
  assert.strictEqual(error.code, "ABORT_ERR");
});
const pre = AbortSignal.abort();
setTimeout(100, "bad", { signal: pre }).then(
  () => assert.fail("pre-aborted resolved"),
  (error) => assert.strictEqual(error.name, "AbortError"),
);
