"use strict";

const assert = require("assert");
const processApi = require("process");

let removedCalls = 0;
let retainedCalls = 0;
const removed = () => removedCalls++;
const retained = () => retainedCalls++;
processApi.on("stage-641", removed);
processApi.on("stage-641", retained);
processApi.removeListener("stage-641", removed);

assert.strictEqual(processApi.emit("stage-641"), true);
assert.strictEqual(removedCalls, 0);
assert.strictEqual(retainedCalls, 1);
processApi.removeListener("stage-641", retained);

console.log("process removeListener passed");
