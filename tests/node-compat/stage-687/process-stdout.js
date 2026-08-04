"use strict";

const assert = require("assert");
const processApi = require("process");

assert(processApi.stdout && typeof processApi.stdout === "object");
assert.strictEqual(typeof processApi.stdout.write, "function");

console.log("process stdout passed");
