const assert = require("assert");
const util = require("util");

assert.strictEqual(util.format("%d", " -0.000"), "-0");
assert.strictEqual(util.format("%d", "-0"), "-0");
assert.strictEqual(
  util.format("%i", 1180591620717411303424n),
  "1180591620717411303424n",
);
