"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.stdout.eventNames, "function");
assert.deepStrictEqual(processApi.stdout.eventNames(), []);

console.log("process stdout eventNames passed");
