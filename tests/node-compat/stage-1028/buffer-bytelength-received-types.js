const assert = require("assert");
const { Buffer } = require("buffer");

for (
  const [value, received] of [
    [32, "Received type number (32)"],
    [NaN, "Received type number (NaN)"],
    [{}, "Received an instance of Object"],
    [[], "Received an instance of Array"],
  ]
) {
  assert.throws(() => Buffer.byteLength(value), {
    code: "ERR_INVALID_ARG_TYPE",
    message:
      `The "string" argument must be of type string or an instance of Buffer or ArrayBuffer. ${received}`,
  });
}
