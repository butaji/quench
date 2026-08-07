const assert = require("assert");
const { Buffer } = require("buffer");

const buffer = Buffer.alloc(4);
assert.throws(() => buffer.writeUInt16BE(0xfffff, 0), {
  code: "ERR_OUT_OF_RANGE",
  message:
    'The value of "value" is out of range. It must be >= 0 and <= 65535. Received 1048575',
});

assert.throws(() => buffer.writeInt8(128, 0), {
  code: "ERR_OUT_OF_RANGE",
});
