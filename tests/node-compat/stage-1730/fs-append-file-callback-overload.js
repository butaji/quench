const assert = require("node:assert");
const fs = require("node:fs");
const path = require("node:path");

const file = path.join(process.cwd(), "append-file-callback-overload.txt");
try {
  fs.unlinkSync(file);
} catch (_) {}

fs.appendFile(file, "append", (error) => {
  assert.strictEqual(error, null);
  assert.strictEqual(fs.readFileSync(file, "utf8"), "append");
  fs.unlinkSync(file);
  console.log("fs appendFile callback overload passed");
});
