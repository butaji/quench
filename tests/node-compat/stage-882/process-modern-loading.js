"use strict";

const assert = require("assert");
const processApi = require("node:process");

assert.strictEqual(typeof processApi.getBuiltinModule, "function");
assert.strictEqual(typeof processApi.loadEnvFile, "function");
assert.strictEqual(processApi.getBuiltinModule("assert"), require("assert"));

console.log("process modern loading passed");
