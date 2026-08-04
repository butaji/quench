"use strict";

const assert = require("assert");
const processApi = require("process");

assert(processApi.stderr && typeof processApi.stderr === "object");
assert.strictEqual(typeof processApi.stderr.write, "function");

console.log("process stderr passed");
