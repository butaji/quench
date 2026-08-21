const assert = require("assert");

assert.strictEqual(/a/.flags, "");
assert.strictEqual(/a/gi.flags, "gi");
assert.strictEqual(RegExp.prototype.flags, "");
