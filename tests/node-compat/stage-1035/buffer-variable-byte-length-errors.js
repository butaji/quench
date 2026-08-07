const assert = require("assert");
const { Buffer } = require("buffer");
const buffer = Buffer.alloc(8);

for (
  const [byteLength, message] of [
    [Infinity, "It must be >= 1 and <= 6."],
    [-1, "It must be >= 1 and <= 6."],
    [NaN, "It must be an integer."],
    [1.01, "It must be an integer."],
  ]
) {
  assert.throws(() => buffer.readIntBE(0, byteLength), {
    code: "ERR_OUT_OF_RANGE",
    message:
      `The value of "byteLength" is out of range. ${message} Received ${byteLength}`,
  });
}
