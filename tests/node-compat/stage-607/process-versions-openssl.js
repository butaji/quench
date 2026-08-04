"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.versions.openssl, "string");
assert(/^\d+\.\d+\.\d+/.test(processApi.versions.openssl));

console.log("process versions openssl passed");
