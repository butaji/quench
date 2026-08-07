const assert = require("assert");
const { Buffer } = require("buffer");
const buffer = Buffer.alloc(2, "a");
const view = new Uint8Array([0x61, 0x61]);

assert.strictEqual(buffer.compare(view), 0);
assert.strictEqual(Buffer.compare(buffer, view), 0);
assert.strictEqual(Buffer.compare(view, view), 0);
assert.strictEqual(buffer.equals(view), true);

for (const value of ["abc", 42, null]) {
  const received = typeof value === "string"
    ? `Received type string ('${value}')`
    : value === null
    ? "Received null"
    : `Received type number (${value})`;
  assert.throws(() => Buffer.compare(buffer, value), {
    code: "ERR_INVALID_ARG_TYPE",
    message:
      `The "buf2" argument must be an instance of Buffer or Uint8Array. ${received}`,
  });
  assert.throws(() => buffer.equals(value), {
    code: "ERR_INVALID_ARG_TYPE",
    message:
      `The "otherBuffer" argument must be an instance of Buffer or Uint8Array. ${received}`,
  });
}
