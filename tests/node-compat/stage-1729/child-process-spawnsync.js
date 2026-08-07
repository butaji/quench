const assert = require("node:assert");
const child = require("node:child_process");

const missing = child.spawnSync("command_does_not_exist", ["x"]);
assert.strictEqual(missing.error.code, "ENOENT");
assert.strictEqual(missing.error.syscall, "spawnSync command_does_not_exist");
assert.deepStrictEqual(missing.error.spawnargs, ["x"]);
const result = child.spawnSync("pwd", [], {
  cwd: process.cwd(),
  encoding: "utf8",
});
assert.strictEqual(result.status, 0);
assert.strictEqual(result.stdout.trim(), process.cwd());
assert.deepStrictEqual(result.output, [null, `${process.cwd()}\n`, ""]);
console.log("child process spawnSync passed");
