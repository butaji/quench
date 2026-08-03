const assert = require("assert");
const os = require("os");

assert.strictEqual(os.constants.signals.SIGTERM, 15);
assert.strictEqual(os.constants.signals.SIGINT, 2);
assert.strictEqual(os.constants.signals.SIGKILL, 9);
assert.strictEqual(os.constants.signals.SIGUSR1, 30);
assert.strictEqual(os.constants.signals.SIGIO, 23);
assert.ok(Object.isFrozen(os.constants.signals));
assert.ok(Object.isFrozen(os.constants));

console.log("os constants signals passed");
