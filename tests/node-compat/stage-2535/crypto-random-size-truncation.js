const assert = require("assert");
const { randomBytes } = require("crypto");

assert.strictEqual(randomBytes(101.2).length, 101);
assert.throws(() => randomBytes(NaN), { code: "ERR_OUT_OF_RANGE" });
