const assert = require("node:assert");
const { URLSearchParams } = require("node:url");
const params = new URLSearchParams("a=1&a=2");
assert.strictEqual(params.size, 2);
assert.strictEqual(
  Object.getOwnPropertyDescriptor(URLSearchParams.prototype, "size").enumerable,
  true,
);
console.log("URLSearchParams size passed");
