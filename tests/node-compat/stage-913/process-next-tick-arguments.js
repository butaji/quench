"use strict";

const assert = require("assert");

assert.strictEqual(typeof process.nextTick, "function");
assert.throws(() => process.nextTick(null), { code: "ERR_INVALID_ARG_TYPE" });
assert.throws(() => process.nextTick(1), { code: "ERR_INVALID_ARG_TYPE" });
assert.throws(() => process.nextTick("callback"), {
  code: "ERR_INVALID_ARG_TYPE",
});

let called = false;
process.nextTick(() => {
  called = true;
});
setImmediate(() => assert.strictEqual(called, true));

console.log("process next tick arguments passed");
