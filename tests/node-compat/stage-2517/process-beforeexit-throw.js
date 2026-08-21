const assert = require("assert");
process.on("beforeExit", () => {
  throw new Error("before-exit");
});
process.on("exit", (code) => {
  assert.strictEqual(code, 0);
  process.exitCode = 0;
});
