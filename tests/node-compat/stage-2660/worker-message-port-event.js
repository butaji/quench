"use strict";
const common = require("../../node/test/common");
const assert = require("assert");
const { MessageChannel, MessagePort } = require("worker_threads");
const c1 = new MessageChannel();
const c2 = new MessageChannel();
c1.port1.postMessage({ port: c2.port2 }, [c2.port2]);
c1.port2.addEventListener("message", common.mustCall((event) => {
  assert.strictEqual(event.ports.length, 1);
  assert.strictEqual(event.ports[0].constructor, MessagePort);
  c1.port1.close();
  c2.port1.close();
}));
