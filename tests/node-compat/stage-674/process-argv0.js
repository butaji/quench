"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.argv0, "string");
assert(processApi.argv0.length > 0);

console.log("process argv0 passed");
