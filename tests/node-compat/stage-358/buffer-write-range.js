const { Buffer } = require("buffer");
const assert = require("assert");
assert.throws(() => Buffer.alloc(9).write("foo", -1), {
  code: "ERR_OUT_OF_RANGE",
  name: "RangeError",
});
