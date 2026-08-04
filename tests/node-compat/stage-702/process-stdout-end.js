"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.stdout.end, "function");
assert.strictEqual(processApi.stdout.end(), processApi.stdout);

console.log("process stdout end passed");
