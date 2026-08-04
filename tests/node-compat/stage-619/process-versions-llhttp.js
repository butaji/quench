"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.versions.llhttp, "string");
assert(/^\d+\.\d+\.\d+/.test(processApi.versions.llhttp));

console.log("process versions llhttp passed");
