"use strict";

const assert = require("assert");
const moduleApi = require("module");

assert.strictEqual(typeof moduleApi.constants, "object");
assert.strictEqual(typeof moduleApi.constants.compileCacheStatus, "object");
assert.strictEqual(typeof moduleApi.SourceMap, "function");
assert.strictEqual(typeof moduleApi.Module, "function");

console.log("module metadata passed");
