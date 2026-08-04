"use strict";

const assert = require("assert");
const fs = require("node:fs");

for (const name of ["watch", "watchFile", "unwatchFile"]) {
  assert.strictEqual(typeof fs[name], "function");
}
assert.strictEqual(typeof fs.FSWatcher, "function");
assert.strictEqual(typeof fs.StatWatcher, "function");

console.log("fs watch api passed");
