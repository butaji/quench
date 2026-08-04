"use strict";

const assert = require("assert");
const repl = require("node:repl");

for (const name of ["start", "recoverable", "REPLServer"]) {
  assert.strictEqual(typeof repl[name], "function");
}

console.log("repl api passed");
