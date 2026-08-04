"use strict";

const assert = require("assert");
const processApi = require("process");

let calls = 0;
processApi.once("stage-639", (value) => {
  calls++;
  assert.strictEqual(value, "payload");
});

assert.strictEqual(processApi.emit("stage-639", "payload"), true);
assert.strictEqual(processApi.emit("stage-639", "payload"), false);
assert.strictEqual(calls, 1);

console.log("process once passed");
