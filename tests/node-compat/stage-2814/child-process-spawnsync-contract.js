const assert = require("assert");
const { spawnSync } = require("child_process");

const result = spawnSync("echo", ["quench", "node"], { encoding: "utf8" });
assert.strictEqual(result.status, 0);
assert.strictEqual(result.signal, null);
assert.strictEqual(result.stdout, "quench node\n");
assert.strictEqual(result.stderr, "");
console.log("child process spawnsync contract pass");
