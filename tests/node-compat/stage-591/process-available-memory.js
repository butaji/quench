"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.availableMemory, "function");
assert.strictEqual(typeof processApi.availableMemory(), "number");
assert.ok(processApi.availableMemory() >= 0);

console.log("process available memory passed");
