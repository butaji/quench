const assert = require("node:assert");
const fs = require("node:fs");

const path = "close-callback.txt";
const descriptor = fs.openSync(path, "w");
fs.close(descriptor, (error) => {
  assert.ifError(error);
  fs.unlinkSync(path);
  console.log("close callback passed");
});
