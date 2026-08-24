"use strict";
const common = require("../../node/test/common");
const assert = require("assert");
const { MessageChannel, MessagePort, Worker } = require("worker_threads");
const channel = new MessageChannel();
const w = new Worker(`
  const { MessagePort } = require("worker_threads");
  const assert = require("assert");
  require("worker_threads").parentPort.on("message", ({ port }) => {
    assert(port instanceof MessagePort);
    port.postMessage("works");
  });
`, { eval: true });
w.postMessage({ port: channel.port2 }, [channel.port2]);
assert(channel.port1 instanceof MessagePort);
assert(channel.port2 instanceof MessagePort);
channel.port1.on("message", common.mustCall((message) => {
  assert.strictEqual(message, "works");
  w.terminate();
}));
