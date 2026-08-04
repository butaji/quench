const assert = require("assert");
const os = require("os");

assert.strictEqual(typeof os.constants.signals.SIGINT, "number");
assert.strictEqual(typeof os.constants.signals.SIGTERM, "number");
assert.strictEqual(os.constants.signals.SIGKILL, 9);
assert.strictEqual(os.constants.signals.SIGSTOP, 17);
