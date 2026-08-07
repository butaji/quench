const assert = require("node:assert");
const fs = require("node:fs");

assert.throws(() => fs.lchmod(__filename), { code: "ERR_INVALID_ARG_TYPE" });
assert.throws(() => fs.lchmodSync(false, 0o644), {
  code: "ERR_INVALID_ARG_TYPE",
});
console.log("lchmod validation passed");
