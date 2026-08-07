const assert = require("assert");
const { MessageChannel, MessagePort } = require("worker_threads");

const { port1, port2 } = new MessageChannel();
assert.ok(port1 instanceof MessagePort);
let received;
port2.on("message", (value) => {
  received = value;
});
port1.postMessage({ value: 3 });
setImmediate(() => {
  assert.deepStrictEqual(received, { value: 3 });
  port2.emit("custom", "value");
});
