"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.versions.acorn, "string");
assert(/^\d+\.\d+\.\d+/.test(processApi.versions.acorn));

console.log("process versions acorn passed");
