const assert = require("node:assert");
const fs = require("node:fs");

const descriptor = fs.openSync(__filename, "r");
assert.throws(() => fs.fchmod(descriptor, {}), {
  code: "ERR_INVALID_ARG_TYPE",
});
fs.closeSync(descriptor);

console.log("fchmod mode order passed");
