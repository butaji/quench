"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.versions.undici, "string");
assert(/^\d+\.\d+\.\d+/.test(processApi.versions.undici));

console.log("process versions undici passed");
