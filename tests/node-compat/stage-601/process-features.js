"use strict";

const assert = require("assert");
const processApi = require("process");

assert(processApi.features && typeof processApi.features === "object");
assert.strictEqual(Array.isArray(processApi.features), false);

console.log("process features passed");
