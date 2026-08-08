const assert = require("assert");

for (const value of [false, "hello", {}]) {
  assert.throws(() => new Blob(value), { code: "ERR_INVALID_ARG_TYPE" });
}
const nested = new Blob(["hello"]);
assert.strictEqual(nested.size, 5);
console.log("blob validation passed");
