const assert = require("node:assert");
const fs = require("node:fs");

assert.throws(() => fs.close("fd"), {
  code: "ERR_INVALID_ARG_TYPE",
  message: /Received type string \('fd'\)/,
});
console.log("Filesystem async close validation passed");
