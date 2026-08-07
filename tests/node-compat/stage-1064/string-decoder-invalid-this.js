const assert = require("assert");
const { StringDecoder } = require("string_decoder");

assert.throws(() => StringDecoder.prototype.write(Buffer.from("abc")), {
  code: "ERR_INVALID_THIS",
});
