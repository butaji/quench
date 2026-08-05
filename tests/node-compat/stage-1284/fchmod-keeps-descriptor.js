const assert = require("node:assert");
const fs = require("node:fs");

fs.writeFileSync("fchmod-file", "content");
const descriptor = fs.openSync("fchmod-file", "r");
fs.fchmod(descriptor, 0o777, (error) => {
  assert.strictEqual(error, null);
  assert.strictEqual(fs.fstatSync(descriptor).mode & 0o777, 0o777);
  fs.closeSync(descriptor);
});

console.log("fchmod descriptor passed");
