const assert = require("assert");
const fs = require("fs");
const missing = `${process.cwd()}/tests/node-compat/stage-2559/missing.txt`;
let callbackCalled = false;
fs.access(missing, fs.constants.F_OK, (error) => {
  callbackCalled = true;
  assert.ok(error);
});
setTimeout(() => {
  assert.strictEqual(callbackCalled, true);
  console.log("fs access callback error delivered");
}, 25);
