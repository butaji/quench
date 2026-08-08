const assert = require("assert");
const fs = require("fs");
const missing = `${process.cwd()}/tests/node-compat/stage-2363/missing`;

fs.promises.access(missing).then(
  () => assert.fail("missing access resolved"),
  (error) => {
    assert.strictEqual(error.code, "ENOENT");
    assert.match(error.stack, /at async Object\.access/);
    console.log("fs promises access stack passed");
  }
);
