"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.versions.nbytes, "string");
assert(/^\d+\.\d+\.\d+/.test(processApi.versions.nbytes));

console.log("process versions nbytes passed");
