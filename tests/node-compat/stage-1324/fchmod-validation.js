const assert = require("node:assert");
const fs = require("node:fs");

assert.throws(() => fs.fchmod(NaN, 0o644, () => {}), {
  code: "ERR_OUT_OF_RANGE",
});
assert.throws(() => fs.fchmod(1, "invalid", () => {}), {
  code: "ERR_INVALID_ARG_VALUE",
});
console.log("fchmod validation passed");
