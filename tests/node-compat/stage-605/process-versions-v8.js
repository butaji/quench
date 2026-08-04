"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.versions.v8, "string");
assert(/^\d+\.\d+\.\d+/.test(processApi.versions.v8));

console.log("process versions v8 passed");
