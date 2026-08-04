"use strict";

const assert = require("assert");
const processApi = require("process");

const available = processApi.availableMemory();
assert.strictEqual(typeof available, "number");
assert(Number.isFinite(available));
assert(available >= 0);

console.log("process available memory passed");
