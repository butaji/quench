"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.permission, "object");
assert.strictEqual(typeof processApi.permission.has, "function");
assert.strictEqual(processApi.permission.has("fs.read"), false);

console.log("process permission passed");
