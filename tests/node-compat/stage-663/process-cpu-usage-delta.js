"use strict";

const assert = require("assert");
const processApi = require("process");

const previous = processApi.cpuUsage();
const delta = processApi.cpuUsage(previous);
for (const field of ["user", "system"]) {
  assert.strictEqual(typeof delta[field], "number");
  assert(Number.isFinite(delta[field]));
  assert(delta[field] >= 0);
}

console.log("process cpu usage delta passed");
