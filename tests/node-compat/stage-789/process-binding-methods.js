"use strict";

const assert = require("assert");
const processApi = require("process");

for (const name of ["binding", "_linkedBinding", "dlopen"]) {
  assert.strictEqual(typeof processApi[name], "function");
}
console.log("process binding methods passed");
