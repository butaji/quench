const assert = require("node:assert");
const { execFile } = require("node:child_process");

const child = execFile("runtime", (error, stdout, stderr) => {
  assert.strictEqual(error.code, "EPERM");
  assert.strictEqual(error.killed, true);
  assert.strictEqual(error.signal, null);
  assert.strictEqual(error.cmd, "runtime");
  assert.strictEqual(stdout, "");
  assert.strictEqual(stderr, "");
});
child.emit("close", -1, null);

console.log("execFile close error passed");
