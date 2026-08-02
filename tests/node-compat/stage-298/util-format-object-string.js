const assert = require("assert");
const { format } = require("util");

assert.strictEqual(format("%o", "foo"), "'foo'");
assert.strictEqual(format("%o", "a'b"), "'a\\'b'");
