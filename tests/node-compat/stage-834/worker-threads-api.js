"use strict";

const assert = require("assert");
const workersApi = require("node:worker_threads");
for (
  const name of [
    "Worker",
    "MessageChannel",
    "MessagePort",
    "BroadcastChannel",
    "receiveMessageOnPort",
    "markAsUncloneable",
  ]
) {
  assert.strictEqual(typeof workersApi[name], "function");
}
assert.strictEqual(typeof workersApi.isMainThread, "boolean");
assert.strictEqual(typeof workersApi.threadId, "number");

console.log("worker threads api passed");
