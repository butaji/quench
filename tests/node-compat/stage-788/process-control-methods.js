"use strict";

const assert = require("assert");
const processApi = require("process");

for (const name of ["kill", "abort", "execve", "reallyExit"]) {
  assert.strictEqual(typeof processApi[name], "function");
}

console.log("process control methods passed");
