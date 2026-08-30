const assert = require("assert");
const cluster = require("cluster");

assert.strictEqual(typeof AbortSignal.timeout, "function");
const signal = AbortSignal.timeout(1);
assert.strictEqual(typeof signal.throwIfAborted, "function");

const worker = cluster.fork();
assert.strictEqual(typeof worker.on, "function");
assert.strictEqual(typeof worker.send, "function");
assert.strictEqual(worker.send("message"), true);
assert.strictEqual(worker.isConnected(), true);
worker.disconnect();
assert.strictEqual(worker.isConnected(), false);
