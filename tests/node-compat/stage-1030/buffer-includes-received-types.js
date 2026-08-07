const assert = require("assert");
const { Buffer } = require("buffer");

const buffer = Buffer.from("abc");
for (
  const [value, received] of [
    [() => {}, "Received function "],
    [{}, "Received an instance of Object"],
    [[], "Received an instance of Array"],
  ]
) {
  assert.throws(() => buffer.includes(value), {
    code: "ERR_INVALID_ARG_TYPE",
    message:
      `The "value" argument must be one of type number or string or an instance of Buffer or Uint8Array. ${received}`,
  });
}
