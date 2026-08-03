const assert = require("assert");
const common = require("../common");
const callback = common.mustCall((value) => value + 1, 1);
assert.strictEqual(callback.calls, 0);
assert.strictEqual(callback(41), 42);
assert.strictEqual(callback.calls, 1);
assert.throws(() => common.mustNotCall()());
