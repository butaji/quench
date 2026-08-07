const assert = require("node:assert");
const fs = require("node:fs");

assert.throws(() => fs.chmodSync(1, 1), {
  code: "ERR_INVALID_ARG_TYPE",
  message: /Received type number \(1\)/,
});

console.log("path number diagnostics passed");
