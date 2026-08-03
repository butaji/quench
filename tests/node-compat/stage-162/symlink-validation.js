const fs = require("fs");
const assert = require("assert");
for (const value of [false, 1, {}, [], null, undefined]) {
  try {
    fs.symlinkSync(value, "");
    assert.fail("accepted invalid target");
  } catch (error) {
    assert.strictEqual(error.code, "ERR_INVALID_ARG_TYPE");
  }
}
