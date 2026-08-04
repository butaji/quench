"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.hrtime.bigint, "function");
const start = processApi.hrtime.bigint();
assert.strictEqual(typeof start, "bigint");
assert.ok(processApi.hrtime.bigint() >= start);

console.log("process hrtime bigint passed");
