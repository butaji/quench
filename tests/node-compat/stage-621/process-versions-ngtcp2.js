"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.versions.ngtcp2, "string");
assert(/^\d+\.\d+\.\d+/.test(processApi.versions.ngtcp2));

console.log("process versions ngtcp2 passed");
