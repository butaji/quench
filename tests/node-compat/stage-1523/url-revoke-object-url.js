const assert = require("node:assert");

assert.throws(() => URL.revokeObjectURL(), {
  code: "ERR_MISSING_ARGS",
  name: "TypeError",
});
console.log("URL revokeObjectURL validation passed");
