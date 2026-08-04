"use strict";

const assert = require("assert");
const inspector = require("node:inspector");

for (const name of ["open", "close", "url", "waitForDebugger"]) {
  assert.strictEqual(typeof inspector[name], "function");
}
assert.strictEqual(typeof inspector.Session, "function");
assert.strictEqual(typeof inspector.console, "object");

console.log("inspector api passed");
