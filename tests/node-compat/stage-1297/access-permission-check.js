const assert = require("node:assert");
const fs = require("node:fs");

fs.writeFileSync("access-read-only", "content");
fs.chmodSync("access-read-only", 0o444);
assert.throws(() => fs.accessSync("access-read-only", fs.constants.W_OK), {
  code: "EACCES",
  errno: -13,
  syscall: "access",
});

console.log("access permission check passed");
