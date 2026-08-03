const assert = require("assert");
const childProcess = require("child_process");

childProcess.exec("does-not-exist", (error, stdout, stderr) => {
  assert.strictEqual(error.code, 127);
  assert.strictEqual(error.cmd, "does-not-exist");
  assert.strictEqual(stdout, "");
  assert.strictEqual(stderr, "");
});

childProcess.execFile("does-not-exist", ["arg"], (error, stdout, stderr) => {
  assert.strictEqual(error.code, "ENOENT");
  assert.strictEqual(error.path, "does-not-exist");
  assert.deepStrictEqual(error.spawnargs, ["arg"]);
  console.log("child process exec errors passed");
});
