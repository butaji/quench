const assert = require("node:assert");
const fs = require("node:fs");

fs.exists("/definitely/not/a/real/path", (exists) => {
  assert.strictEqual(exists, false);
});
fs.exists({}, (exists) => assert.strictEqual(exists, false));
assert.throws(() => fs.exists("x"), { code: "ERR_INVALID_ARG_TYPE" });
console.log("Filesystem exists callback passed");
