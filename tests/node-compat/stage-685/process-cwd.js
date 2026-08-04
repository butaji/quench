"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.cwd, "function");
const current = processApi.cwd();
assert.strictEqual(typeof current, "string");
assert(current.length > 0);

console.log("process cwd passed");
