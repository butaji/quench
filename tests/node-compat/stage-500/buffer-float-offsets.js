const assert = require("assert");

const buffer = Buffer.allocUnsafe(8);
assert.throws(() => buffer.writeFloatLE(1, 1.01), {
  code: "ERR_OUT_OF_RANGE",
  message:
    'The value of "offset" is out of range. It must be an integer. Received 1.01',
});
assert.throws(() => buffer.writeFloatLE(1, 5), {
  code: "ERR_OUT_OF_RANGE",
  message:
    'The value of "offset" is out of range. It must be >= 0 and <= 4. Received 5',
});
