"use strict";

const assert = require("assert");
const processApi = require("process");

assert(Array.isArray(processApi.execArgv));
assert.strictEqual(processApi.execArgv.length, 0);

console.log("process execArgv passed");
