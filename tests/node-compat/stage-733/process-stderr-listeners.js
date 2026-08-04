"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.stderr.listeners, "function");
assert.deepStrictEqual(processApi.stderr.listeners("drain"), []);

console.log("process stderr listeners passed");
