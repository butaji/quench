"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.emitWarning, "function");
assert.strictEqual(processApi.emitWarning("compat warning"), undefined);

console.log("process warning passed");
