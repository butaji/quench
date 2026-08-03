const assert = require("assert");
const child = require("child_process").fork("child.js", []);

child.once("exit", (code, signal) => {
  assert.strictEqual(code, 0);
  assert.strictEqual(signal, null);
  console.log("child process fork exit passed");
});
