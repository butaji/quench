"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.versions.unicode, "string");
assert(/^\d+\.\d+/.test(processApi.versions.unicode));

console.log("process versions unicode passed");
