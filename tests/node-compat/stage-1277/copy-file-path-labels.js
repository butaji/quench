const assert = require("node:assert");
const fs = require("node:fs");

for (const value of [false, 1, {}, [], null, undefined]) {
  assert.throws(() => fs.copyFileSync(value, "destination"), {
    code: "ERR_INVALID_ARG_TYPE",
    message: /src/,
  });
  assert.throws(() => fs.copyFileSync("source", value), {
    code: "ERR_INVALID_ARG_TYPE",
    message: /dest/,
  });
}

console.log("copyFile path labels passed");
