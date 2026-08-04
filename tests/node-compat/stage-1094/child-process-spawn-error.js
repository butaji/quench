const assert = require("node:assert");
const childProcess = require("node:child_process");

const child = childProcess.spawn("foo123", ["bar"]);
assert.strictEqual(child.pid, undefined);
child.on("spawn", () => assert.fail("spawn must not be emitted"));
child.on("error", (error) => {
  assert.strictEqual(error.code, "ENOENT");
  assert.strictEqual(error.syscall, "spawn foo123");
  assert.deepStrictEqual(error.spawnargs, ["bar"]);
});
