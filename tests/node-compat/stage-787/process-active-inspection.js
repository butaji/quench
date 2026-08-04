"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi._getActiveHandles, "function");
assert.strictEqual(typeof processApi._getActiveRequests, "function");
assert.strictEqual(Array.isArray(processApi._getActiveHandles()), true);
assert.strictEqual(Array.isArray(processApi._getActiveRequests()), true);

console.log("process active inspection passed");
