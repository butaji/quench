"use strict";

const assert = require("assert");
const processApi = require("process");

assert(processApi.features && typeof processApi.features === "object");
assert.strictEqual(typeof processApi.features.inspector, "boolean");

console.log("process features passed");
