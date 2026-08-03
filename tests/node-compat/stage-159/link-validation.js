const fs = require("fs");
const assert = require("assert");
for (const value of [false, 1, [], {}, null, undefined]) {
  try {
    fs.linkSync(value, "");
    assert.fail("accepted invalid source");
  } catch (error) {
    assert.strictEqual(error.code, "ERR_INVALID_ARG_TYPE");
  }
  try {
    fs.linkSync("", value);
    assert.fail("accepted invalid target");
  } catch (error) {
    assert.strictEqual(error.code, "ERR_INVALID_ARG_TYPE");
  }
}
