"use strict";

const assert = require("assert");
const { MessageChannel, receiveMessageOnPort } = require("worker_threads");

const { port1, port2 } = new MessageChannel();
assert.strictEqual(receiveMessageOnPort(port2), undefined);
port1.postMessage({ first: true });
port1.postMessage({ second: true });
assert.deepStrictEqual(receiveMessageOnPort(port2), { message: { first: true } });
assert.deepStrictEqual(receiveMessageOnPort(port2), { message: { second: true } });
assert.strictEqual(receiveMessageOnPort(port2), undefined);

for (const value of [null, 0, -1, {}, []]) {
  assert.throws(() => receiveMessageOnPort(value), {
    name: "TypeError",
    code: "ERR_INVALID_ARG_TYPE",
    message: 'The "port" argument must be a MessagePort instance',
  });
}
