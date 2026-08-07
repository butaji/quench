const assert = require("assert");
const { StringDecoder } = require("string_decoder");

assert.throws(() => new StringDecoder(1), {
  code: "ERR_UNKNOWN_ENCODING",
  message: "Unknown encoding: 1",
});
assert.throws(() => new StringDecoder("test"), {
  code: "ERR_UNKNOWN_ENCODING",
  message: "Unknown encoding: test",
});
assert.throws(() => new StringDecoder().write(null), {
  code: "ERR_INVALID_ARG_TYPE",
});
