"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.versions.icu, "string");
assert(/^\d+\.\d+/.test(processApi.versions.icu));

console.log("process versions icu passed");
