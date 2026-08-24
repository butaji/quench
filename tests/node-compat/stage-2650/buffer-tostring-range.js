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
