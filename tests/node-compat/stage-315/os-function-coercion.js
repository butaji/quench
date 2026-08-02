const assert = require("assert");
const os = require("os");

assert.strictEqual(typeof +os.uptime, "number");
assert.ok(!Number.isNaN(+os.uptime));
assert.ok(!Number.isNaN(+os.availableParallelism));
assert.ok(!Number.isNaN(+os.freemem));
