const assert = require("assert");
const childProcess = require("child_process");

const child = childProcess.spawnSync(process.execPath, [
  `${process.cwd()}/tests/node/test/parallel/test-process-ppid.js`,
  "child",
]);

assert.strictEqual(child.status, 0);
assert.strictEqual(child.signal, null);
assert.strictEqual(child.stderr.toString(), "");
assert.strictEqual(Number(child.stdout.toString().trim()), process.pid);
