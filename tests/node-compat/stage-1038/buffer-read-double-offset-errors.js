const assert = require("assert");
const { Buffer } = require("buffer");

const buffer = Buffer.alloc(8);
for (const offset of [Infinity, -1, 1]) {
  assert.throws(() => buffer.readDoubleLE(offset), {
    code: "ERR_OUT_OF_RANGE",
    message:
      `The value of "offset" is out of range. It must be >= 0 and <= 0. Received ${offset}`,
  });
}

assert.throws(() => Buffer.alloc(1).readDoubleLE(1), {
  code: "ERR_BUFFER_OUT_OF_BOUNDS",
});
