const assert = require("assert");
const util = require("util");
assert.strictEqual(util.inspect({ a: 1 }), "{ a: 1 }");
assert.strictEqual(
  util.inspect({ a: function named() {} }),
  "{ a: [Function: named] }",
);
console.log("util inspect object passed");
