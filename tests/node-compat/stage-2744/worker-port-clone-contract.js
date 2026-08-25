"use strict";

const assert = require("assert");
const { MessageChannel } = require("worker_threads");

const { port1, port2 } = new MessageChannel();
port2.once("message", () => assert.fail("a rejected clone must not deliver"));
assert.throws(() => port1.postMessage(function foo() {}), {
  name: "DataCloneError",
  message: /function foo\(\) \{\} could not be cloned\./,
});
port1.close();
