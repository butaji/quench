const assert = require("assert");
const { spawnSync } = require("child_process");

const child = spawnSync(process.execPath, [
  "--max-http-header-size=10",
  "-p",
  "http.maxHeaderSize",
]);
assert.strictEqual(+child.stdout.toString().trim(), 10);
console.log("http maxHeaderSize child propagation passed");
