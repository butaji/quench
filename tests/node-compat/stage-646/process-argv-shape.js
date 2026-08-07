"use strict";

const assert = require("assert");
const processApi = require("process");

assert(Array.isArray(processApi.argv));
assert(processApi.argv.length >= 1);
for (const argument of processApi.argv) {
  assert.strictEqual(typeof argument, "string");
}

console.log("process argv shape passed");
