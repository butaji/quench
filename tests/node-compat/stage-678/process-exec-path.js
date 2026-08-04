"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.execPath, "string");
assert(processApi.execPath.length > 0);

console.log("process execPath passed");
