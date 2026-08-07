"use strict";
const assert = require("assert");
const zlib = require("zlib");

const cases = [
  [undefined, "Received undefined"],
  [null, "Received null"],
  [true, "Received type boolean (true)"],
  [false, "Received type boolean (false)"],
  [0, "Received type number (0)"],
  [1, "Received type number (1)"],
  [[1, 2, 3], "Received an instance of Array"],
  [{ foo: "bar" }, "Received an instance of Object"],
];
for (const [input, suffix] of cases) {
  const error = assert.throws(() => zlib.deflateSync(input), {
    name: "TypeError",
    code: "ERR_INVALID_ARG_TYPE",
  });
  assert.strictEqual(
    error.message,
    'The "buffer" argument must be of type string or an instance of Buffer, TypedArray, DataView, or ArrayBuffer. ' +
      suffix,
  );
}
console.log("zlib input validation passed");
