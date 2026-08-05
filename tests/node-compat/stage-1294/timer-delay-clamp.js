const assert = require("node:assert");

let called = false;
setTimeout(() => {
  called = true;
}, 2147483648);
setTimeout(() => assert.strictEqual(called, true), 5);

console.log("timer delay clamp passed");
