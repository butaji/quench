"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.versions.nghttp3, "string");
assert(/^\d+\.\d+\.\d+/.test(processApi.versions.nghttp3));

console.log("process versions nghttp3 passed");
