"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.versions.ada, "string");
assert(/^\d+\.\d+\.\d+/.test(processApi.versions.ada));

console.log("process versions ada passed");
