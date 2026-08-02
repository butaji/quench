const assert = require("assert");
const os = require("os");

assert.strictEqual(os.devNull, "/dev/null");
assert.strictEqual(typeof os.availableParallelism(), "number");
assert.ok(os.availableParallelism() > 0);
