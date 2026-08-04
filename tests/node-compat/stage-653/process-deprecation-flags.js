"use strict";

const assert = require("assert");
const processApi = require("process");

for (const name of ["noDeprecation", "traceDeprecation", "throwDeprecation"]) {
  assert.strictEqual(typeof processApi[name], "boolean");
}

console.log("process deprecation flags passed");
