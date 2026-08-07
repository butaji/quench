const assert = require("assert");
const { Buffer } = require("buffer");

for (
  const name of [
    "readFloatLE",
    "readFloatBE",
    "writeFloatLE",
    "writeFloatBE",
  ]
) {
  const buffer = Buffer.alloc(4);
  const call = name.startsWith("read")
    ? () => buffer[name](1.01)
    : () => buffer[name](0, 1.01);
  assert.throws(call, {
    code: "ERR_OUT_OF_RANGE",
    message:
      'The value of "offset" is out of range. It must be an integer. Received 1.01',
  });
}
