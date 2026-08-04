"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.versions.nghttp2, "string");
assert(/^\d+\.\d+\.\d+/.test(processApi.versions.nghttp2));

console.log("process versions nghttp2 passed");
