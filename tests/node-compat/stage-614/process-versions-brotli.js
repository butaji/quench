"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.versions.brotli, "string");
assert(/^\d+\.\d+\.\d+/.test(processApi.versions.brotli));

console.log("process versions brotli passed");
