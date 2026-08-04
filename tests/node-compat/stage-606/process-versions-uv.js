"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.versions.uv, "string");
assert(/^\d+\.\d+\.\d+/.test(processApi.versions.uv));

console.log("process versions uv passed");
