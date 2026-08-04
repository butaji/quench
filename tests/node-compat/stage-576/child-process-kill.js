"use strict";

const assert = require("assert");
const childProcess = require("child_process");

const child = childProcess.spawn("node", ["-e", "setTimeout(() => {}, 1000)"]);
assert.strictEqual(child.killed, false);
assert.strictEqual(child.kill("SIGTERM"), true);
assert.strictEqual(child.killed, true);
child.once("exit", (code, signal) => {
  assert.strictEqual(code, null);
  assert.strictEqual(signal, "SIGTERM");
  console.log("child process kill passed");
});
