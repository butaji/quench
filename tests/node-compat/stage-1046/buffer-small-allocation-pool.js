const assert = require("assert");
const { Buffer } = require("buffer");
const { MessageChannel } = require("worker_threads");

const first = Buffer.from("hello");
const second = Buffer.from("world");

assert.strictEqual(first.buffer, second.buffer);
assert.deepStrictEqual([...first], [104, 101, 108, 108, 111]);
assert.deepStrictEqual([...second], [119, 111, 114, 108, 100]);

const { port1 } = new MessageChannel();
assert.throws(() => port1.postMessage(first, [first.buffer]), {
  code: 25,
  name: "DataCloneError",
});
assert.throws(() => first.buffer.transfer(), TypeError);
assert.strictEqual(first.buffer, second.buffer);
