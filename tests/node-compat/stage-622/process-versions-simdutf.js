"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.versions.simdutf, "string");
assert(/^\d+\.\d+\.\d+/.test(processApi.versions.simdutf));

console.log("process versions simdutf passed");
