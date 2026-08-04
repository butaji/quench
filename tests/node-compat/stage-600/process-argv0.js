"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.argv0, "string");
assert.strictEqual(processApi.argv0, "node");

console.log("process argv0 passed");
