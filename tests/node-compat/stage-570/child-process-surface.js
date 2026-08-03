const assert = require("assert");
const childProcess = require("child_process");

const child = childProcess.spawn("node", ["-e", ""]);
for (const name of ["stdin", "stdout", "stderr", "stdio"]) {
  assert.ok(child[name]);
}
assert.strictEqual(child.stdio.length, 3);
assert.strictEqual(child.connected, false);
assert.strictEqual(child.killed, false);
assert.deepStrictEqual(child.spawnargs, ["-e", ""]);
assert.strictEqual(child.spawnfile, "node");

console.log("child process surface passed");
