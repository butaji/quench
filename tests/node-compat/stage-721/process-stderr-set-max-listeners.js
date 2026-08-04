"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.stderr.setMaxListeners, "function");
assert.strictEqual(processApi.stderr.setMaxListeners(5), processApi.stderr);
assert.strictEqual(processApi.stderr.getMaxListeners(), 5);
processApi.stderr.setMaxListeners(10);

console.log("process stderr set max listeners passed");
