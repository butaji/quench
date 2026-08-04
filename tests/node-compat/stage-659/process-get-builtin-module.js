"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.getBuiltinModule, "function");
assert.strictEqual(processApi.getBuiltinModule("assert"), assert);
assert.strictEqual(processApi.getBuiltinModule("node:assert"), assert);

console.log("process getBuiltinModule passed");
