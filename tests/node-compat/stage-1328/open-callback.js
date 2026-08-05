const assert = require("node:assert");
const fs = require("node:fs");

fs.open("open-callback.txt", "w", (error, descriptor) => {
  assert.ifError(error);
  assert.strictEqual(typeof descriptor, "number");
  fs.closeSync(descriptor);
  fs.unlinkSync("open-callback.txt");
  console.log("open callback passed");
});
