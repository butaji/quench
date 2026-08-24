"use strict";
const common = require("../../node/test/common");
const assert = require("assert");
const { Worker } = require("worker_threads");
const { createHook } = require("async_hooks");
let handle;
createHook({ init(asyncId, type, triggerAsyncId, resource) {
  if (type === "WORKER") { handle = resource; this.disable(); }
} }).enable();
const worker = new Worker("", { eval: true });
assert.strictEqual(handle.hasRef(), true);
worker.unref();
assert.strictEqual(handle.hasRef(), false);
worker.ref();
assert.strictEqual(handle.hasRef(), true);
worker.on("exit", common.mustCall((code) => {
  assert.strictEqual(code, 0);
  assert.strictEqual(handle.hasRef(), true);
  setTimeout(common.mustCall(() => assert.strictEqual(handle.hasRef(), undefined)), 0);
}));
