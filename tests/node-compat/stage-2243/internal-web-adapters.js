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
assert(
  new adapters.newStreamDuplexFromReadableWritablePair(new TransformStream())
);
assert.throws(() => adapters.newStreamWritableFromWritableStream(1), {
  code: "ERR_INVALID_ARG_TYPE"
});
assert.throws(() => adapters.newStreamDuplexFromReadableWritablePair({}), {
  code: "ERR_INVALID_ARG_TYPE"
});
console.log("internal Web Streams adapters passed");
