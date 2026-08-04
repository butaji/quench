"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.versions.tz, "string");
assert(/^\d{4}[a-z]+$/.test(processApi.versions.tz));

console.log("process versions tz passed");
