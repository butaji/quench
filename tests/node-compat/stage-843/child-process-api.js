"use strict";

const assert = require("assert");
const childProcess = require("node:child_process");

for (
  const name of [
    "exec",
    "execFile",
    "fork",
    "spawn",
    "spawnSync",
    "execFileSync",
    "execSync",
  ]
) {
  assert.strictEqual(typeof childProcess[name], "function");
}
for (const name of ["ChildProcess", "exec", "execFile", "fork", "spawn"]) {
  assert.ok(childProcess[name]);
}

console.log("child process api passed");
