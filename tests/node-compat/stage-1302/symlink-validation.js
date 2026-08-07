const assert = require("node:assert");
const fs = require("node:fs");

assert.throws(() => fs.symlink(false, "link", () => {}), {
  code: "ERR_INVALID_ARG_TYPE",
});
assert.throws(() => fs.symlink("target", "link", "bad", () => {}), {
  code: "ERR_INVALID_ARG_VALUE",
});

console.log("symlink validation passed");
