const assert = require("assert");
assert.strictEqual(typeof process.pid, "number");
assert.strictEqual(process.pid > 0, true);
assert.strictEqual(typeof process.ppid, "number");
