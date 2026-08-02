const assert = require("assert");
const os = require("os");

assert.strictEqual(typeof +os.totalmem, "number");
assert.strictEqual(+os.totalmem, os.totalmem());
