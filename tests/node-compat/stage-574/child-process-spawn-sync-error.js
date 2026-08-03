const assert = require("assert");
const childProcess = require("child_process");

const result = childProcess.spawnSync("not_a_real_command", ["x"]);
assert.strictEqual(result.status, null);
assert.strictEqual(result.signal, null);
assert.strictEqual(result.error.code, "ENOENT");
assert.strictEqual(result.error.path, "not_a_real_command");
assert.deepStrictEqual(result.error.spawnargs, ["x"]);
assert.strictEqual(result.stdout, undefined);

console.log("child process spawnSync error passed");
