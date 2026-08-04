"use strict";

const assert = require("assert");

for (const name of ["inspector", "node:inspector"]) {
  const inspector = require(name);
  assert.strictEqual(typeof inspector.open, "function");
  assert.strictEqual(typeof inspector.Session, "function");
}
for (const name of ["inspector/promises", "node:inspector/promises"]) {
  assert.throws(() => require(name), { code: "ERR_UNKNOWN_BUILTIN_MODULE" });
}

console.log("inspector api passed");
