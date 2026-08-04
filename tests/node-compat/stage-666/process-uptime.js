"use strict";

const assert = require("assert");
const processApi = require("process");

const before = processApi.uptime();
assert.strictEqual(typeof before, "number");
assert(Number.isFinite(before));
assert(before >= 0);
const after = processApi.uptime();
assert(after >= before);

console.log("process uptime passed");
