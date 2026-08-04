"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.stderr.eventNames, "function");
assert.deepStrictEqual(processApi.stderr.eventNames(), []);

console.log("process stderr eventNames passed");
