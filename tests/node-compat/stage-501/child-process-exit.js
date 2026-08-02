const assert = require("assert");
const childProcess = require("child_process");

const child = childProcess.spawn("quench-node", ["exit.js", 23]);
child.on("exit", (code, signal) => {
  assert.strictEqual(code, 23);
  assert.strictEqual(signal, null);
});
