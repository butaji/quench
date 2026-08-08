const assert = require("assert");
const adapters = require("internal/webstreams/adapters");
const {
  ReadableStream,
  WritableStream,
  TransformStream
} = require("stream/web");

assert.strictEqual(
  typeof adapters.newStreamReadableFromReadableStream,
  "function"
);
assert.strictEqual(
  typeof adapters.newStreamWritableFromWritableStream,
  "function"
);
assert.strictEqual(
  typeof adapters.newStreamDuplexFromReadableWritablePair,
  "function"
);
assert(adapters.newStreamReadableFromReadableStream(new ReadableStream()));
assert(adapters.newStreamWritableFromWritableStream(new WritableStream()));
assert(adapters.newStreamDuplexFromReadableWritablePair(new TransformStream()));
console.log("internal Web Streams adapters passed");
