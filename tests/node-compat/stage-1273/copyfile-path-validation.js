const assert = require("node:assert");
const fs = require("node:fs");

assert.throws(() => fs.copyFile(false, __filename, () => {}), {
  code: "ERR_INVALID_ARG_TYPE",
});
assert.throws(() => fs.copyFile(__filename, false, () => {}), {
  code: "ERR_INVALID_ARG_TYPE",
});

console.log("copyFile path validation passed");
