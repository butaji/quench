"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.uptime, "function");
const before = processApi.uptime();
assert.strictEqual(typeof before, "number");
assert(before >= 0);
const after = processApi.uptime();
assert(after >= before);

console.log("process uptime passed");
