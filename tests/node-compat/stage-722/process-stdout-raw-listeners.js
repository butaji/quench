"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.stdout.rawListeners, "function");
assert.deepStrictEqual(processApi.stdout.rawListeners("drain"), []);

console.log("process stdout rawListeners passed");
