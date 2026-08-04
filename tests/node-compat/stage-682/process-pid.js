"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.pid, "number");
assert(Number.isInteger(processApi.pid));
assert(processApi.pid > 0);

console.log("process pid passed");
