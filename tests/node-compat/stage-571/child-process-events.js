const assert = require("assert");
const childProcess = require("child_process");

const child = childProcess.spawn("node", ["-e", ""]);
let spawned = false;
child.on("spawn", () => {
  spawned = true;
});
child.once("exit", (code, signal) => {
  assert.strictEqual(spawned, true);
  assert.strictEqual(code, 0);
  assert.strictEqual(signal, null);
});
child.once("close", (code, signal) => {
  assert.strictEqual(spawned, true);
  assert.strictEqual(code, 0);
  assert.strictEqual(signal, null);
  console.log("child process events passed");
});
