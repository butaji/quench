const assert = require("assert");
const util = require("util");

assert.strictEqual(util.format("foo", "bar", "baz"), "foo bar baz");
assert.strictEqual(util.format("%o", "foo"), "'foo'");
