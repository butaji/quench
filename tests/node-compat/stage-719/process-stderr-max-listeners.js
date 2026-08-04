"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.stderr.getMaxListeners, "function");
assert.strictEqual(processApi.stderr.getMaxListeners(), 10);

console.log("process stderr max listeners passed");
