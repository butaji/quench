const assert = require("assert");
let calls = 0;

process.on("beforeExit", () => {
  calls++;
  if (calls === 1) setImmediate(() => {});
});
process.on("exit", () => assert.strictEqual(calls, 2));
