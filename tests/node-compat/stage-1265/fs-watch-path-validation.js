const assert = require("node:assert");
const fs = require("node:fs");

assert.throws(() => fs.watch(false, () => {}), {
  code: "ERR_INVALID_ARG_TYPE",
});

console.log("fs watch path validation passed");
