"use strict";
const assert = require("assert");
const a = Buffer.from([1, 2, 3, 4]);
const b = Buffer.from([1, 2, 3, 4]);

assert.strictEqual(a.compare(b, 255), 1);
assert.strictEqual(a.compare(b, 0, 0, 0, 0), 0);
for (const args of [[b, "0"], [b, 0, 100], [b, -1], [b, 0, 1, -1]]) {
  assert.throws(() => a.compare(...args), { code: args[1] === "0" ? "ERR_INVALID_ARG_TYPE" : "ERR_OUT_OF_RANGE" });
}
