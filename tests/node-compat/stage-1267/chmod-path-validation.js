const assert = require("node:assert");
const fs = require("node:fs");

assert.throws(() => fs.chmodSync(false, 0o644), {
  code: "ERR_INVALID_ARG_TYPE",
});
assert.throws(() => fs.chmod(false, 0o644, () => {}), {
  code: "ERR_INVALID_ARG_TYPE",
});

console.log("chmod path validation passed");
