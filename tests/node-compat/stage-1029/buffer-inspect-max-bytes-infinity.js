const assert = require("assert");
const buffer = require("buffer");

buffer.INSPECT_MAX_BYTES = 2;
assert.strictEqual(buffer.INSPECT_MAX_BYTES, 2);
buffer.INSPECT_MAX_BYTES = Infinity;
assert.strictEqual(buffer.INSPECT_MAX_BYTES, Infinity);

assert.throws(
  () => {
    buffer.INSPECT_MAX_BYTES = -1;
  },
  {
    code: "ERR_OUT_OF_RANGE",
    name: "RangeError",
    message: "INSPECT_MAX_BYTES is out of range",
  },
);
