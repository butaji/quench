"use strict";
const assert = require("assert");
const b = Buffer.from("abc");
const cases = [
  [["ascii", 3], ""], [["ascii", Infinity], ""], [["ascii", 3.14, 3], ""],
  [["ascii", "Infinity", 3], ""], [["ascii", 1, 0], ""],
  [["ascii", 1, -1.2], ""], [["ascii", -1, 3], "abc"],
  [["ascii", "1", 3], "bc"], [["ascii", "3", 3], ""],
  [["ascii", 0, "node.js"], ""], [["ascii", 0, null], ""],
];
for (const [args, expected] of cases) {
  assert.strictEqual(b.toString(...args), expected, JSON.stringify(args));
}
assert.strictEqual(b.toString({ toString() { return "ascii"; } }), "abc");
for (const value of [0, null]) {
  assert.throws(() => b.toString(value, 1, 2), { code: "ERR_UNKNOWN_ENCODING" });
}

for (const [bytes, expectedUnits] of [
  [[0x00, 0xd8], [0xd800]],
  [[0x00, 0xdc], [0xdc00]],
  [[0x3d, 0xd8, 0x00, 0xde], [0xd83d, 0xde00]],
  [[0x41, 0x00, 0x42], [0x41]],
]) {
  const decoded = Buffer.from(bytes).toString("utf16le");
  assert.strictEqual(decoded.length, expectedUnits.length);
  assert.deepStrictEqual(
    Array.from({ length: decoded.length }, (_, index) => decoded.charCodeAt(index)),
    expectedUnits,
  );
}
