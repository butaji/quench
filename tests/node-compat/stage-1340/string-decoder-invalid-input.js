const assert = require("node:assert");
const { StringDecoder } = require("node:string_decoder");

assert.throws(() => new StringDecoder("utf8").write(null), {
  code: "ERR_INVALID_ARG_TYPE",
  message:
    'The "buf" argument must be an instance of Buffer, TypedArray, or DataView. Received null',
});
console.log("string decoder invalid input passed");
