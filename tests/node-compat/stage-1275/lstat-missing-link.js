const assert = require("node:assert");
const fs = require("node:fs");

assert.throws(() => fs.lstatSync("missing-lstat-target"), {
  code: "ENOENT",
  syscall: "lstat",
});

console.log("lstat missing link passed");
