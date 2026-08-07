const assert = require("assert");

for (
  const name of [
    "asciiSlice",
    "base64Slice",
    "base64urlSlice",
    "latin1Slice",
    "hexSlice",
    "ucs2Slice",
    "utf8Slice",
    "asciiWrite",
    "base64Write",
    "base64urlWrite",
    "latin1Write",
    "hexWrite",
    "ucs2Write",
    "utf8Write",
  ]
) {
  assert.strictEqual(typeof Buffer.prototype[name], "function");
}
