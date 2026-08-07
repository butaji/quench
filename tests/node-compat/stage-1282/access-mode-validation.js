const assert = require("node:assert");
const fs = require("node:fs");

for (const mode of [false, 1n, {}, [1], "r"]) {
  assert.throws(() => fs.accessSync(__filename, mode), {
    code: "ERR_INVALID_ARG_TYPE",
  });
}
for (const mode of [-1, 8, Infinity, NaN]) {
  assert.throws(() => fs.accessSync(__filename, mode), {
    code: "ERR_OUT_OF_RANGE",
  });
}

console.log("access mode validation passed");
