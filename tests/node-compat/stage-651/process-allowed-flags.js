"use strict";

const assert = require("assert");
const processApi = require("process");

assert(processApi.allowedNodeEnvironmentFlags instanceof Set);
assert.strictEqual(
  typeof processApi.allowedNodeEnvironmentFlags.has,
  "function",
);

console.log("process allowed flags passed");
