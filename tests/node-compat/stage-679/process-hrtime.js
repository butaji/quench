"use strict";

const assert = require("assert");
const processApi = require("process");

const sample = processApi.hrtime();
assert(Array.isArray(sample));
assert.strictEqual(sample.length, 2);
assert(Number.isInteger(sample[0]));
assert(Number.isInteger(sample[1]));
assert(sample[0] >= 0);
assert(sample[1] >= 0 && sample[1] < 1_000_000_000);

console.log("process hrtime passed");
