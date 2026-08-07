const assert = require("assert");
const { Buffer } = require("buffer");

assert.strictEqual(new Buffer(4).length, 4);
assert.throws(() => new Buffer(42, "utf8"), {
  code: "ERR_INVALID_ARG_TYPE",
  name: "TypeError",
  message:
    'The "string" argument must be of type string. Received type number (42)',
});
