const assert = require("assert");
const child = require("child_process").spawn("quench-node", ["-e", "0"]);

child.once("exit", (code, signal) => {
  assert.strictEqual(code, 0);
  assert.strictEqual(signal, null);
});
