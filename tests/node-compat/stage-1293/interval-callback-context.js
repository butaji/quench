const assert = require("node:assert");

let calls = 0;
const handle = setInterval(function () {
  calls++;
  clearInterval(this);
}, 0);
setTimeout(() => assert.strictEqual(calls, 1), 5);

console.log("interval callback context passed");
