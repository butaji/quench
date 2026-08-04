"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.ppid, "number");
assert(Number.isInteger(processApi.ppid));
assert(processApi.ppid >= 0);

console.log("process ppid passed");
