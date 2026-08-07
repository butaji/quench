const assert = require("assert");
const { PassThrough } = require("stream");
const { buffer, bytes, json, text } = require("stream/consumers");

const createStream = (value) => {
  const stream = new PassThrough();
  queueMicrotask(() => stream.end(value));
  return stream;
};

Promise.all([
  buffer(createStream("hello")).then((value) =>
    assert.strictEqual(value.toString(), "hello")
  ),
  bytes(createStream("hello")).then((value) => {
    assert(value instanceof Uint8Array);
    assert.strictEqual(Buffer.from(value).toString(), "hello");
  }),
  text(createStream("hello")).then((value) =>
    assert.strictEqual(value, "hello")
  ),
  json(createStream('{"ok":true}')).then((value) =>
    assert.deepStrictEqual(value, { ok: true })
  ),
]);
