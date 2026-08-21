const assert = require("assert");
const { compose } = require("stream");

const sourceToSink = compose(
  (async function* () {
    yield "value";
  })(),
  async function* (source) {
    yield* source;
  },
  async function (source) {
    for await (const _value of source);
  },
);
assert.strictEqual(sourceToSink.writable, false);
assert.strictEqual(sourceToSink.readable, false);

const transform = compose(async function* (source) {
  yield* source;
});
assert.strictEqual(transform.writable, true);
assert.strictEqual(transform.readable, true);

const readable = compose(
  (async function* () {
    yield "value";
  })(),
  async function* (source) {
    yield* source;
  },
);
assert.strictEqual(readable.writable, false);
assert.strictEqual(readable.readable, true);

const writable = compose(async function (source) {
  for await (const _value of source);
});
assert.strictEqual(writable.writable, true);
assert.strictEqual(writable.readable, false);
