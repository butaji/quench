"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.getBuiltinModule, "function");
const assertionModule = processApi.getBuiltinModule("node:assert");
assert.strictEqual(typeof assertionModule, "function");
assert.strictEqual(typeof assertionModule.strictEqual, "function");

console.log("process builtin module passed");
