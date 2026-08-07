const assert = require("node:assert");
const fs = require("node:fs");

fs.writeFileSync("mkdir-parent-file", "file");
assert.throws(
  () => fs.mkdirSync("mkdir-parent-file/child/grandchild", { recursive: true }),
  { code: "ENOTDIR", syscall: "mkdir" },
);

console.log("mkdir preserved errors passed");
