const assert = require("node:assert");
const fs = require("node:fs");

assert.throws(() => fs.read(0, { buffer: null }, commonMissingCallback), {
  code: "ERR_INVALID_ARG_TYPE",
});

function commonMissingCallback() {}
console.log("read validation order passed");
