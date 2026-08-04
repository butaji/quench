"use strict";

const assert = require("assert");

for (const name of ["node:sqlite", "sqlite"]) {
  const sqlite = require(name);
  assert.strictEqual(typeof sqlite.DatabaseSync, "function");
  assert.strictEqual(typeof sqlite.StatementSync, "function");
}

console.log("sqlite api passed");
