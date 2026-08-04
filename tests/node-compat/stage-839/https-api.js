"use strict";

const assert = require("assert");
const https = require("node:https");

for (const name of ["request", "get", "createServer", "Agent"]) {
  assert.strictEqual(typeof https[name], "function");
}
assert.strictEqual(typeof https.globalAgent, "object");
assert.strictEqual(typeof https.Server, "function");

console.log("https api passed");
