"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.stdin[Symbol.asyncDispose], "function");
assert.strictEqual(
  processApi.stdin[Symbol.asyncDispose].constructor.name,
  "AsyncFunction",
);

console.log("process stdin async dispose passed");
