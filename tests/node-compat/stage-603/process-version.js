"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.version, "string");
assert(/^v\d+\.\d+\.\d+$/.test(processApi.version));

console.log("process version passed");
