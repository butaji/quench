const assert = require("node:assert");
const fs = require("node:fs");

fs.writeFile("write-file-basic.txt", "data", (error) => {
  assert.ifError(error);
  assert.strictEqual(fs.readFileSync("write-file-basic.txt", "utf8"), "data");
});
console.log("write file basic passed");
