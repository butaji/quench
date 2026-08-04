"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.stderr.listenerCount, "function");
assert.strictEqual(processApi.stderr.listenerCount("drain"), 0);

console.log("process stderr listenerCount passed");
