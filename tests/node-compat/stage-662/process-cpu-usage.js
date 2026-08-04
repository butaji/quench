"use strict";

const assert = require("assert");
const processApi = require("process");

const usage = processApi.cpuUsage();
for (const field of ["user", "system"]) {
  assert.strictEqual(typeof usage[field], "number");
  assert(Number.isFinite(usage[field]));
  assert(usage[field] >= 0);
}

console.log("process cpu usage passed");
