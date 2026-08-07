const assert = require("assert");
const { Buffer } = require("buffer");

for (
  const allocate of [
    () => Buffer(-1),
    () => Buffer.alloc(-1),
    () => Buffer.allocUnsafe(-1),
    () => Buffer.allocUnsafeSlow(-1),
    () => Buffer(NaN),
  ]
) {
  assert.throws(allocate, { code: "ERR_OUT_OF_RANGE", name: "RangeError" });
}
