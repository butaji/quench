"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.versions.modules, "string");
assert(/^\d+$/.test(processApi.versions.modules));

console.log("process versions modules passed");
