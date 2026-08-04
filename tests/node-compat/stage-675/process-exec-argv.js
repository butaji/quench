"use strict";

const assert = require("assert");
const processApi = require("process");

assert(Array.isArray(processApi.execArgv));
for (const argument of processApi.execArgv) {
  assert.strictEqual(typeof argument, "string");
}

console.log("process execArgv passed");
