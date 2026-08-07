const assert = require("node:assert");
const fs = require("node:fs");

fs.symlinkSync("realpath-cycle-b", "realpath-cycle-a", "dir");
fs.symlinkSync("realpath-cycle-a", "realpath-cycle-b", "dir");
assert.throws(() => fs.realpathSync("realpath-cycle-a"), {
  code: "ELOOP",
  syscall: "realpath",
});

console.log("realpath cycle passed");
