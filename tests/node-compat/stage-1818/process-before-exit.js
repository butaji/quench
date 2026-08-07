const assert = require("assert");
const events = [];

process.once("beforeExit", (code) => {
  events.push(["beforeExit", code]);
});
process.once("exit", (code) => {
  events.push(["exit", code]);
  assert.deepStrictEqual(events, [
    ["beforeExit", 0],
    ["exit", 0],
  ]);
  console.log("process beforeExit passed");
});
