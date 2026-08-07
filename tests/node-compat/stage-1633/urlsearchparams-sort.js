const assert = require("node:assert");

const params = new URLSearchParams("b=2&a=1&a=0");
params.sort();
assert.strictEqual(params.toString(), "a=1&a=0&b=2");
assert.throws(() => URLSearchParams.prototype.sort.call({}), {
  code: "ERR_INVALID_THIS",
});
console.log("URLSearchParams sort passed");
