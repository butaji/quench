const assert = require("assert");
const os = require("os");

assert.strictEqual(typeof os.uptime(), "number");
assert.strictEqual(typeof os.getPriority(), "number");
os.setPriority(os.constants.priority.PRIORITY_LOW);
assert.strictEqual(os.getPriority(), os.constants.priority.PRIORITY_LOW);
