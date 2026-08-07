"use strict";

const assert = require("assert");
const processApi = require("process");

const usage = processApi.resourceUsage();
const fields = [
  "userCPUTime",
  "systemCPUTime",
  "maxRSS",
  "minorPageFault",
  "majorPageFault",
  "fsRead",
  "fsWrite",
  "involuntaryContextSwitches",
  "voluntaryContextSwitches",
];
for (const field of fields) {
  assert.strictEqual(typeof usage[field], "number");
  assert(Number.isFinite(usage[field]));
  assert(usage[field] >= 0);
}

console.log("process resource usage passed");
