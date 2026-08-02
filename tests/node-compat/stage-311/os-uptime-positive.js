const assert = require("assert");
const os = require("os");

assert.strictEqual(typeof os.uptime(), "number");
assert.ok(os.uptime() > 0);
