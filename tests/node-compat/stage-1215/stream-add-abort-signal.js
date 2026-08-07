const assert = require("assert");
const { addAbortSignal } = require("stream");

assert.throws(() => addAbortSignal("invalid", {}), {
  code: "ERR_INVALID_ARG_TYPE",
});

const stream = {};
assert.strictEqual(
  addAbortSignal(new AbortController().signal, stream),
  stream,
);
