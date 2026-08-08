const assert = require("assert");
const common = require("../../node/test/common");

const callback = common.mustSucceed((value) => assert.strictEqual(value, 7), 2);
callback(null, 7);
callback(null, 7);
assert.strictEqual(callback.calls, 2);
console.log("common mustSucceed count passed");
