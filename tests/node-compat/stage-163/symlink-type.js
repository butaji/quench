const fs = require("fs");
const assert = require("assert");
try {
  fs.symlinkSync("", "", "invalid");
  assert.fail("accepted invalid symlink type");
} catch (error) {
  assert.strictEqual(error.code, "ERR_INVALID_ARG_VALUE");
}
