const assert = require("node:assert");
const fs = require("node:fs");

assert.throws(() => new fs.Dir(), { code: "ERR_MISSING_ARGS" });
console.log("directory missing path passed");
