const assert = require("node:assert");
const fs = require("node:fs");

fs.writeFileSync("copy-source", "source");
fs.writeFileSync("copy-destination", "destination");
assert.throws(() => fs.copyFileSync("copy-source", "copy-destination", 1), {
  code: "EEXIST",
  errno: -17,
  syscall: "copyfile",
  path: "copy-source",
  dest: "copy-destination",
});

console.log("copyFile exclusive passed");
