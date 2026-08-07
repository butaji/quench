const assert = require("node:assert");
const fs = require("node:fs");

assert.throws(() => fs.accessSync("missing-access-file"), {
  code: "ENOENT",
  errno: -2,
  syscall: "access",
  path: "missing-access-file",
});

console.log("access error metadata passed");
