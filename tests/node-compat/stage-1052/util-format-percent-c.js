const assert = require("assert");
const util = require("util");

assert.strictEqual(util.format("%c"), "%c");
assert.strictEqual(util.format("%cab", "color: blue"), "ab");
assert.strictEqual(util.format("%cab", "color: blue", "c"), "ab c");
