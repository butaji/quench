"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.versions.zlib, "string");
assert(/^\d+\.\d+\.\d+/.test(processApi.versions.zlib));

console.log("process versions zlib passed");
