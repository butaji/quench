"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.hrtime.bigint, "function");
const sample = processApi.hrtime.bigint();
assert.strictEqual(typeof sample, "bigint");
assert(sample >= 0n);

console.log("process hrtime bigint passed");
