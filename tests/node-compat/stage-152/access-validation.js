const fs = require("fs");
const assert = require("assert");
try {
  fs.accessSync(100);
  assert.fail("accepted numeric path");
} catch (error) {
  assert.strictEqual(error.code, "ERR_INVALID_ARG_TYPE");
}
try {
  fs.access(100, fs.constants.F_OK, () => {});
  assert.fail("accepted numeric path");
} catch (error) {
  assert.strictEqual(error.code, "ERR_INVALID_ARG_TYPE");
}
