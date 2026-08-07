const assert = require("node:assert");
const { execFile } = require("node:child_process");

execFile(process.execPath, ["fixture.js", 42], (error, stdout, stderr) => {
  assert.strictEqual(
    error.message,
    "Command failed: " + process.execPath + " fixture.js 42",
  );
  assert.strictEqual(error.code, 42);
  assert.strictEqual(stdout, "");
  assert.strictEqual(stderr, "");
});

console.log("execFile failure passed");
