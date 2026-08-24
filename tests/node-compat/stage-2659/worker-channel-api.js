"use strict";

const assert = require("assert");
const { MessageChannel } = require("worker_threads");

const channel = new MessageChannel();
assert.strictEqual(typeof channel.port1.postMessage, "function");
assert.strictEqual(typeof channel.port2.postMessage, "function");
assert.throws(() => channel.port1.postMessage("x", [new ArrayBuffer(1)]), {
  code: 25,
  name: "DataCloneError",
});
