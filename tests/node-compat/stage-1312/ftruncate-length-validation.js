const assert = require("node:assert");
const fs = require("node:fs");

assert.throws(() => fs.ftruncate(1, "bad"), {
  code: "ERR_INVALID_ARG_TYPE",
});
console.log("ftruncate length validation passed");
