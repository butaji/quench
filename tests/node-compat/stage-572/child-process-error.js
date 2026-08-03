const assert = require("assert");
const childProcess = require("child_process");

const child = childProcess.spawn("does-not-exist", ["arg"]);
assert.strictEqual(child.pid, undefined);
child.on("error", (error) => {
  assert.strictEqual(error.code, "ENOENT");
  assert.strictEqual(error.syscall, "spawn does-not-exist");
  assert.strictEqual(error.path, "does-not-exist");
  assert.deepStrictEqual(error.spawnargs, ["arg"]);
  console.log("child process error passed");
});
