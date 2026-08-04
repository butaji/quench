"use strict";

const assert = require("assert");

for (const name of ["inspector", "node:inspector"]) {
  const inspector = require(name);
  assert.strictEqual(typeof inspector.open, "function");
  assert.strictEqual(typeof inspector.Session, "function");
}
for (const name of ["inspector/promises", "node:inspector/promises"]) {
  const inspector = require(name);
  assert.strictEqual(typeof inspector.open, "function");
  assert.strictEqual(typeof inspector.Session, "function");
}

console.log("inspector api passed");
