"use strict";

const assert = require("assert");
const processApi = require("process");

assert(processApi.allowedNodeEnvironmentFlags instanceof Set);
assert.strictEqual(
  typeof processApi.allowedNodeEnvironmentFlags.has,
  "function",
);
assert.strictEqual(
  typeof processApi.allowedNodeEnvironmentFlags.size,
  "number",
);
assert.strictEqual(
  processApi.allowedNodeEnvironmentFlags.has("--trace-warnings"),
  false,
);

console.log("process allowed node flags passed");
