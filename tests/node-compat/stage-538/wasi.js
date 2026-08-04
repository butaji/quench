"use strict";

const assert = require("assert");

for (const name of ["wasi", "node:wasi"]) {
  const wasi = require(name);
  assert.strictEqual(typeof wasi.WASI, "function");
  assert.strictEqual(typeof wasi.getImportObject, "function");
}

console.log("wasi api passed");
