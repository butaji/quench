const assert = require("node:assert");

let called = false;
setTimeout(() => {
  called = true;
  console.log("timer callback passed");
}, 0);
process.on("exit", () => assert.strictEqual(called, true));
