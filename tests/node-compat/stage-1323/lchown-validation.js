const assert = require("node:assert");
const fs = require("node:fs");

assert.throws(() => fs.lchown(__filename, "uid", 1, () => {}), {
  code: "ERR_INVALID_ARG_TYPE",
});
assert.throws(() => fs.lchownSync(false, 1, 1), {
  code: "ERR_INVALID_ARG_TYPE",
});
console.log("lchown validation passed");
