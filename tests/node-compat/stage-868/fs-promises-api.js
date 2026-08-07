"use strict";

const assert = require("assert");
const fs = require("node:fs/promises");

for (
  const name of [
    "access",
    "copyFile",
    "mkdir",
    "open",
    "readFile",
    "readdir",
    "rename",
    "rm",
    "stat",
    "writeFile",
  ]
) {
  assert.strictEqual(typeof fs[name], "function");
}
assert.strictEqual(typeof fs.FileHandle, "function");

console.log("fs promises api passed");
