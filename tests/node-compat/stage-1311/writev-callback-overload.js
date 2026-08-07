const assert = require("node:assert");
const fs = require("node:fs");

assert.throws(() => fs.writev(false, () => {}), {
  code: "ERR_INVALID_ARG_TYPE",
});
console.log("writev callback overload passed");
