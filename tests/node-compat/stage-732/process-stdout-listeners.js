"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.stdout.listeners, "function");
assert.deepStrictEqual(processApi.stdout.listeners("drain"), []);

console.log("process stdout listeners passed");
