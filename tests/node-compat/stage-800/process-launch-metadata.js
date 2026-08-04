"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.execPath, "string");
assert.strictEqual(typeof processApi.argv0, "string");
assert.strictEqual(Array.isArray(processApi.argv), true);
assert.strictEqual(Array.isArray(processApi.execArgv), true);

console.log("process launch metadata passed");
