const assert = require("node:assert");
const fs = require("node:fs");

assert.throws(() => fs.closeSync("fd"), {
  code: "ERR_INVALID_ARG_TYPE",
  message: /Received type string \('fd'\)/,
});
console.log("Filesystem close argument message passed");
