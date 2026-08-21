const assert = require("assert");
let calls = 0;

function callback() {
  throw new Error("tick");
}

process.nextTick(callback);
process.on("uncaughtException", (error) => {
  calls++;
  assert.strictEqual(error.message, "tick");
});
process.on("exit", () => assert.strictEqual(calls, 1));
