const assert = require("assert");
const { Buffer } = require("buffer");
const buffer = Buffer.alloc(16);

for (const offset of [Infinity, -1, 9]) {
  assert.throws(() => buffer.writeDoubleLE(23, offset), {
    code: "ERR_OUT_OF_RANGE",
    message:
      `The value of "offset" is out of range. It must be >= 0 and <= 8. Received ${offset}`,
  });
}

for (const offset of [NaN, 1.01]) {
  assert.throws(() => buffer.writeDoubleLE(42, offset), {
    code: "ERR_OUT_OF_RANGE",
    message:
      `The value of "offset" is out of range. It must be an integer. Received ${offset}`,
  });
}
