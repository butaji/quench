"use strict";

const assert = require("assert");
const { createHook } = require("async_hooks");
const { Worker } = require("worker_threads");

let resource;
const hook = createHook({
  init(_asyncId, type, _triggerAsyncId, candidate) {
    if (type === "WORKER") resource = candidate;
  },
});
hook.enable();

const worker = new Worker("", { eval: true });
assert.ok(resource);
assert.strictEqual(typeof resource.hasRef, "function");
assert.strictEqual(resource.hasRef(), true);
worker.unref();
assert.strictEqual(resource.hasRef(), false);
worker.ref();
assert.strictEqual(resource.hasRef(), true);

worker.terminate().then(() => hook.disable());
