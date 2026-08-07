const assert = require("node:assert");
const fs = require("node:fs");

assert.throws(() => fs.fstat("invalid"), {
  code: "ERR_INVALID_ARG_TYPE",
});

console.log("fstat validation order passed");
