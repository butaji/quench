const { Buffer } = require("buffer");
const assert = require("assert");
assert.throws(() => Buffer.alloc(2).fill("a", 0, 0, false), {
  code: "ERR_INVALID_ARG_TYPE",
  name: "TypeError",
});
