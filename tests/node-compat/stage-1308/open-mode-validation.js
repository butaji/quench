const assert = require("node:assert");
const fs = require("node:fs");

assert.throws(() => fs.openSync(__filename, "r", "not-an-octal-mode"), {
  code: "ERR_INVALID_ARG_VALUE",
});
console.log("open mode validation passed");
