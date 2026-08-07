"use strict";

const assert = require("assert");
const processApi = require("process");

assert(
  processApi.permission && typeof processApi.permission.has === "function",
);
assert.strictEqual(typeof processApi.permission.has("fs.read"), "boolean");

console.log("process permission passed");
