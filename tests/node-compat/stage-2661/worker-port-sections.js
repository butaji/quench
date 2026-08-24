"use strict";
const common = require("../../node/test/common");
const assert = require("assert");
const { MessageChannel } = require("worker_threads");

{
  const { port1, port2 } = new MessageChannel();
  const input = { a: 1 };
  port1.postMessage(input);
  port2.on("message", common.mustCall((received) => {
    assert.deepStrictEqual(received, input);
    port2.close(common.mustCall());
  }));
}
{
  const c1 = new MessageChannel();
  const c2 = new MessageChannel();
  c1.port1.postMessage({ port: c2.port2 }, [c2.port2]);
  c1.port2.addEventListener("message", common.mustCall((event) => {
    assert.strictEqual(event.ports.length, 1);
    assert.strictEqual(event.ports[0].constructor, require("worker_threads").MessagePort);
    c1.port1.close();
    c2.port1.close();
  }));
}
{
  const { port1, port2 } = new MessageChannel();
  port2.on("message", common.mustCall((msg) => assert.strictEqual(msg.ab.byteLength, 10), 4));
  for (const options of [
    [new ArrayBuffer(10)],
    { transfer: [new ArrayBuffer(10)] },
  ]) {
    const ab = Array.isArray(options) ? options[0] : options.transfer[0];
    port1.postMessage({ ab }, options);
    assert.strictEqual(ab.byteLength, 0);
  }
  {
    const ab = new ArrayBuffer(10);
    port1.postMessage({ ab }, (function* () { yield ab; })());
    assert.strictEqual(ab.byteLength, 0);
  }
  {
    const ab = new ArrayBuffer(10);
    port1.postMessage({ ab }, { transfer: (function* () { yield ab; })() });
    assert.strictEqual(ab.byteLength, 0);
  }
}
{
  const { port1, port2 } = new MessageChannel();
  port2.on("message", common.mustCall(6));
  port1.postMessage(1, null);
  port1.postMessage(2, undefined);
  port1.postMessage(3, []);
  port1.postMessage(4, {});
  port1.postMessage(5, { transfer: undefined });
  port1.postMessage(6, { transfer: [] });
  assert.throws(() => port1.postMessage(5, 0), { code: "ERR_INVALID_ARG_TYPE" });
  assert.throws(() => port1.postMessage(5, false), { code: "ERR_INVALID_ARG_TYPE" });
  assert.throws(() => port1.postMessage(5, "X"), { code: "ERR_INVALID_ARG_TYPE" });
  assert.throws(() => port1.postMessage(5, Symbol("X")), { code: "ERR_INVALID_ARG_TYPE" });
  assert.throws(() => port1.postMessage(5, { transfer: null }), { code: "ERR_INVALID_ARG_TYPE" });
  assert.throws(() => port1.postMessage(5, { transfer: 0 }), { code: "ERR_INVALID_ARG_TYPE" });
  assert.throws(() => port1.postMessage(5, { transfer: false }), { code: "ERR_INVALID_ARG_TYPE" });
  assert.throws(() => port1.postMessage(5, { transfer: {} }), { code: "ERR_INVALID_ARG_TYPE" });
  assert.throws(() => port1.postMessage(5, { transfer: { [Symbol.iterator]() { return {}; } } }), { code: "ERR_INVALID_ARG_TYPE" });
  assert.throws(() => port1.postMessage(5, { transfer: { [Symbol.iterator]() { return { next: 42 }; } } }), { code: "ERR_INVALID_ARG_TYPE" });
  assert.throws(() => port1.postMessage(5, { transfer: { [Symbol.iterator]() { return { next: null }; } } }), { code: "ERR_INVALID_ARG_TYPE" });
}
assert.deepStrictEqual(Object.getOwnPropertyNames(require("worker_threads").MessagePort.prototype).sort(), [
  "close", "constructor", "hasRef", "onmessage", "onmessageerror",
  "postMessage", "ref", "start", "unref",
]);
{
  const { port1, port2 } = new MessageChannel();
  const input = { a: 1 };
  const dummy = common.mustNotCall();
  port2.addListener("message", dummy);
  setImmediate(common.mustCall(() => {
    port2.removeListener("message", dummy);
    port1.postMessage(input);
    setImmediate(common.mustCall(() => {
      port2.on("message", common.mustCall((received) => {
        assert.deepStrictEqual(received, input);
        port2.close(common.mustCall());
      }));
    }));
  }));
}
{
  const { port2 } = new MessageChannel();
  port2.addEventListener("foo", common.mustCall((received) => {
    assert.strictEqual(received.type, "foo");
    assert.strictEqual(received.detail, "bar");
  }));
  port2.on("foo", common.mustCall((received) => assert.strictEqual(received, "bar")));
  port2.emit("foo", "bar");
}
{
  const { port1, port2 } = new MessageChannel();
  port1.onmessage = common.mustCall((message) => {
    assert.strictEqual(message.data, 4);
    port2.close(common.mustCall());
  });
  port1.postMessage(2);
  port2.onmessage = common.mustCall((message) => port2.postMessage(message.data * 2));
}
{
  const { port1, port2 } = new MessageChannel();
  const input = { a: 1 };
  port1.postMessage(input);
  setImmediate(common.mustCall(() => {
    port2.on("message", common.mustCall((received) => {
      assert.deepStrictEqual(received, input);
      port2.close(common.mustCall());
    }));
  }));
}
