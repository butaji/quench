const assert = require("assert");
const fs = require("fs");
const missing = `${process.cwd()}/tests/node-compat/stage-2362/missing`;
let callbackCalls = 0;
let rejectionCalls = 0;
fs.access(missing, (error) => {
  callbackCalls++;
  assert.strictEqual(error.code, "ENOENT");
});
fs.promises.access(missing).then(
  () => assert.fail("missing access resolved"),
  (error) => {
    rejectionCalls++;
    assert.strictEqual(error.code, "ENOENT");
  },
);
setTimeout(() => {
  assert.strictEqual(callbackCalls, 1);
  assert.strictEqual(rejectionCalls, 1);
  console.log("fs access error order passed");
}, 20);
