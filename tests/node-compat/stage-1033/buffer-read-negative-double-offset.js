const assert = require("assert");
const { Buffer } = require("buffer");

const buffer = Buffer.alloc(8);
for (const method of ["readDoubleBE", "readDoubleLE"]) {
  assert.throws(() => buffer[method](-1), {
    code: "ERR_OUT_OF_RANGE",
    name: "RangeError",
    message: 'The value of "offset" is out of range'
  });
}
