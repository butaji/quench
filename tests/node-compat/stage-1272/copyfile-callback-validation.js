const assert = require("node:assert");
const fs = require("node:fs");

assert.throws(() => fs.copyFile(__filename, __filename, 0, 0), {
  code: "ERR_INVALID_ARG_TYPE",
});

console.log("copyFile callback validation passed");
