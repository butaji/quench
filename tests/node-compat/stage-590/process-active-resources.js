"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.getActiveResourcesInfo, "function");
assert.strictEqual(Array.isArray(processApi.getActiveResourcesInfo()), true);

console.log("process active resources passed");
