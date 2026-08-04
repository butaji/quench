"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.versions.cldr, "string");
assert(/^\d+\.\d+/.test(processApi.versions.cldr));

console.log("process versions cldr passed");
