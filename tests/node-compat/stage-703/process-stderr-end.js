"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.stderr.end, "function");
assert.strictEqual(processApi.stderr.end(), processApi.stderr);

console.log("process stderr end passed");
