const assert = require("assert");
const childProcess = require("child_process");

const child = childProcess.spawnSync(process.execPath, [
  "-e",
  "process.stdout.write(String(process.ppid));",
]);

assert.strictEqual(child.status, 0);
assert.strictEqual(child.signal, null);
assert.strictEqual(child.stderr.toString(), "");
assert.strictEqual(Number(child.stdout.toString()), process.pid);
