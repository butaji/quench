const assert = require("assert");
const fs = require("fs");

fs.opendir(__filename, (error) => {
  assert.strictEqual(error.code, "ENOTDIR");
  console.log("fs opendir file callback passed");
});
