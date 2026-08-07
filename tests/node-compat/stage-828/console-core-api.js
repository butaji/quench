"use strict";

const assert = require("assert");
const consoleApi = require("node:console");

for (
  const name of [
    "Console",
    "log",
    "error",
    "warn",
    "dir",
    "time",
    "timeEnd",
    "assert",
    "table",
    "createTask",
  ]
) {
  assert.strictEqual(typeof consoleApi[name], "function");
}
assert.strictEqual(
  typeof new consoleApi.Console(process.stdout, process.stderr),
  "object",
);

console.log("console core api passed");
