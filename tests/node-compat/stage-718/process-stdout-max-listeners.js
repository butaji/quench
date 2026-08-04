"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.stdout.getMaxListeners, "function");
assert.strictEqual(processApi.stdout.getMaxListeners(), 10);

console.log("process stdout max listeners passed");
