const assert = require("assert");
const path = require("path").posix;

assert.strictEqual(path.dirname("/foo"), "/");
assert.strictEqual(path.dirname("/foo///"), "/");
assert.strictEqual(path.dirname("foo"), ".");
