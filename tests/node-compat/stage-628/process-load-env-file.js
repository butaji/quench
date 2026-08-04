"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.loadEnvFile, "function");
assert.strictEqual(processApi.loadEnvFile(), undefined);

console.log("process loadEnvFile passed");
