"use strict";
const assert = require("assert");
const cases = [
  ["default-ascii", ["abc"]],
  ["default-wide", ["\u0222aa"]],
  ["utf8-wide", ["\u0222aa", "utf8"]],
  ["binary", ["abc", "binary"]],
  ["latin1", ["\u0222aa", "latin1"]],
  ["ucs2", ["\u0222aa", "ucs2"]],
  ["hex", ["c8a26161", "hex"]],
  ["base64", ["yKJhYQ==", "base64"]],
  ["base64url", ["yKJhYQ", "base64url"]],
  ["offset-encoding", ["abc", 4, 1, "utf8"]],
];
for (const [label, args] of cases) {
  try {
    Buffer.allocUnsafe(28).fill(...args);
  } catch (error) {
    assert.fail(`${label}: ${error && error.stack ? error.stack : error}`);
  }
}
const invalid = [
  ["negative-start", ["a", -1]],
  ["past-end", ["a", 0, 29]],
  ["empty", [""]],
  ["empty-alloc", [0]],
  ["unknown-encoding", ["a", 0, 0, "foo"]],
  ["bad-encoding-number", ["a", 0, 0, NaN]],
];
for (const [label, args] of invalid) {
  try {
    Buffer.allocUnsafe(28).fill(...args);
  } catch (error) {
    if (label === "empty" || label === "empty-alloc") throw error;
  }
}
const buf = Buffer.alloc(64, 10);
buf.fill(11, 0, 32);
buf.fill("h");
buf.fill(0);
buf.fill(null);
buf.fill(1, 16, 32);
assert.strictEqual(Buffer.alloc(10, "abc").toString(), "abcabcabca");
assert.strictEqual(Buffer.alloc(10, "abc").fill("\u0567").toString(), "\u0567\u0567\u0567\u0567\u0567");
const moduleValue = require("internal/test/binding");
const { internalBinding } = moduleValue;
assert.strictEqual(typeof internalBinding, "function");
assert.strictEqual(typeof internalBinding("buffer").fill, "function");
const fill = internalBinding("buffer").fill;
assert.throws(() => fill(Buffer.alloc(1), 1, -1, 0, 1), { code: "ERR_OUT_OF_RANGE" });
assert.throws(() => fill(Buffer.alloc(1), 1, 1, -2, 1), { code: "ERR_OUT_OF_RANGE" });
assert.throws(() => Buffer.alloc(1).fill(Buffer.alloc(1), 0, { [Symbol.toPrimitive]() { return 1; } }), { code: "ERR_INVALID_ARG_TYPE" });
const spoof = Buffer.from("w00t");
Object.defineProperty(spoof, "length", { value: 1337, enumerable: true });
assert.throws(() => spoof.fill(""), { code: "ERR_BUFFER_OUT_OF_BOUNDS" });
