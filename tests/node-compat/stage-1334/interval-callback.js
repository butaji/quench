const assert = require("node:assert");

let calls = 0;
const handle = setInterval(() => {
  calls += 1;
  clearInterval(handle);
}, 0);
setTimeout(() => {
  assert.strictEqual(calls, 1);
  console.log("interval callback passed");
}, 1);
