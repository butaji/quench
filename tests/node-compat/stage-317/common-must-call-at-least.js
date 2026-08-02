const assert = require("assert");
const common = require("../common");

const callback = common.mustCallAtLeast(() => {}, 2);
callback();
callback();
callback();
assert.strictEqual(callback.calls, 3);
