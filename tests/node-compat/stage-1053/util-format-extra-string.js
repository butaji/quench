const assert = require("assert");
const util = require("util");

assert.strictEqual(util.format(1, "1"), "1 1");
assert.strictEqual(util.format(1, -0), "1 -0");
assert.strictEqual(util.format(1, "number"), "1 number");
