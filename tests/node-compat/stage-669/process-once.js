"use strict";

const assert = require("assert");
const processApi = require("process");

let calls = 0;
const listener = () => {
  calls += 1;
};
processApi.once("stage-669", listener);
processApi.emit("stage-669");
processApi.emit("stage-669");
assert.strictEqual(calls, 1);

console.log("process once passed");
