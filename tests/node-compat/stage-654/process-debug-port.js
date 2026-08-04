"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.debugPort, "number");
assert(Number.isFinite(processApi.debugPort));
assert(processApi.debugPort >= 0);

console.log("process debug port passed");
