const assert = require("node:assert");

assert.strictEqual(process.env.hasOwnProperty, Object.prototype.hasOwnProperty);
assert.strictEqual(Object.hasOwn(process.env, "hasOwnProperty"), false);

console.log("process.env inherited properties passed");
