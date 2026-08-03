const fs = require("fs");
const assert = require("assert");
const path = `/tmp/quench-node-stage-134-${process.pid}`;
fs.writeFileSync(path, "abc");
for (const value of ["", false, null, {}, []]) {
  try {
    fs.truncate(path, value, () => {});
    assert.fail("accepted invalid length");
  } catch (error) {
    assert.strictEqual(error.code, "ERR_INVALID_ARG_TYPE");
  }
}
fs.rmSync(path);
