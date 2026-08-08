const assert = require("assert");

const workerThreads = require("node:worker_threads");
assert.strictEqual(workerThreads.isMainThread, true);
assert.strictEqual(typeof workerThreads.MessageChannel, "function");
console.log("node builtin normalization passed");
