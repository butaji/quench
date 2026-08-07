const assert = require("node:assert");
const fs = require("node:fs");

assert.throws(() => fs.access(__filename, fs.constants.F_OK), {
  code: "ERR_INVALID_ARG_TYPE",
  name: "TypeError",
});

console.log("access callback code passed");
