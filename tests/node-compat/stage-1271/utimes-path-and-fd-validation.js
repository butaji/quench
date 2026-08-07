const assert = require("node:assert");
const fs = require("node:fs");

assert.throws(() => fs.utimesSync(0, new Date(), new Date()), {
  code: "ERR_INVALID_ARG_TYPE",
});
assert.throws(() => fs.futimesSync(-1, new Date(), new Date()), {
  code: "ERR_OUT_OF_RANGE",
});

console.log("utimes path and fd validation passed");
