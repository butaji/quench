"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.stdout.setMaxListeners, "function");
assert.strictEqual(processApi.stdout.setMaxListeners(5), processApi.stdout);
assert.strictEqual(processApi.stdout.getMaxListeners(), 5);
processApi.stdout.setMaxListeners(10);

console.log("process stdout set max listeners passed");
