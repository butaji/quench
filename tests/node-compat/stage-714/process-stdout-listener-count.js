"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.stdout.listenerCount, "function");
assert.strictEqual(processApi.stdout.listenerCount("drain"), 0);

console.log("process stdout listenerCount passed");
