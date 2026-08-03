const fs = require("fs");
const assert = require("assert");
const path = `/tmp/quench-node-stage-135-${process.pid}`;
fs.writeFileSync(path, "abc");
for (const value of [-1.5, 1.5]) {
  try {
    fs.truncateSync(path, value);
    assert.fail("accepted fractional length");
  } catch (error) {
    assert.strictEqual(error.code, "ERR_OUT_OF_RANGE");
  }
}
fs.rmSync(path);
