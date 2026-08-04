"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.hrtime, "function");
assert.strictEqual(typeof processApi.hrtime.bigint, "function");
assert.strictEqual(typeof processApi.hrtime.bigint(), "bigint");

console.log("process hrtime passed");
