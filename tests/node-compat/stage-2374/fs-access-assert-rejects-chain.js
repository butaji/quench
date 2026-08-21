const assert = require("assert");
const fs = require("fs");

const missing = `${process.cwd()}/tests/node-compat/stage-2374/missing`;
let predicateCalls = 0;
const expectedError = (error) => {
  assert.strictEqual(error.code, "ENOENT");
};
fs.promises.access(missing).catch((error) => {
  expectedError(error);
});

assert
  .rejects(fs.promises.access(missing), (error) => {
    predicateCalls += 1;
    expectedError(error);
    assert.match(error.stack, /at async Object\.access/);
    return true;
  })
  .then(() => {
    assert.strictEqual(predicateCalls, 1);
    console.log("fs access assert rejects chain passed");
  });
