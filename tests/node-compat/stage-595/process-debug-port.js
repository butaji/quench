"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.debugPort, "number");
const original = processApi.debugPort;
processApi.debugPort = original + 1;
assert.strictEqual(processApi.debugPort, original + 1);

console.log("process debug port passed");
