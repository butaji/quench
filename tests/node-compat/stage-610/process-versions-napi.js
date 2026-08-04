"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.versions.napi, "string");
assert(/^\d+$/.test(processApi.versions.napi));

console.log("process versions napi passed");
