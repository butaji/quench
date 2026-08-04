"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.stderr.rawListeners, "function");
assert.deepStrictEqual(processApi.stderr.rawListeners("drain"), []);

console.log("process stderr rawListeners passed");
