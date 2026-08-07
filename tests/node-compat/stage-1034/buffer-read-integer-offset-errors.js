const assert = require("assert");
const { Buffer } = require("buffer");
const buffer = Buffer.alloc(4);

for (const offset of [NaN, 1.01]) {
  assert.throws(() => buffer.readInt8(offset), {
    code: "ERR_OUT_OF_RANGE",
    name: "RangeError",
    message:
      `The value of "offset" is out of range. It must be an integer. Received ${offset}`,
  });
}

assert.throws(() => buffer.readInt8(-1), {
  code: "ERR_OUT_OF_RANGE",
  name: "RangeError",
});
