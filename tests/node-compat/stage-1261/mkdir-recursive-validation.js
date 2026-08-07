const assert = require("node:assert");
const fs = require("node:fs");

assert.throws(() => fs.mkdirSync("missing", { recursive: "yes" }), {
  code: "ERR_INVALID_ARG_TYPE",
});

console.log("mkdir recursive validation passed");
