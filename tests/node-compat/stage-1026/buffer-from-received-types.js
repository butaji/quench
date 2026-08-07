const assert = require("assert");
const { Buffer } = require("buffer");

for (
  const [value, received] of [
    [{}, "Received an instance of Object"],
    [{ __proto__: null }, "Received [Object: null prototype] {}"],
    [new Boolean(true), "Received an instance of Boolean"],
    [Symbol(), "Received type symbol (Symbol())"],
    [5n, "Received type bigint (5n)"],
    [undefined, "Received undefined"],
    [null, "Received null"],
    [() => {}, "Received function "],
  ]
) {
  assert.throws(() => Buffer.from(value), {
    code: "ERR_INVALID_ARG_TYPE",
    message:
      `The first argument must be of type string or an instance of Buffer, ArrayBuffer, or Array or an Array-like Object. ${received}`,
  });
}
