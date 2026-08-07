"use strict";

const assert = require("assert");
const processApi = require("process");

for (const name of ["memoryUsage", "cpuUsage", "resourceUsage"]) {
  assert.strictEqual(typeof processApi[name], "function");
}
for (
  const name of [
    "arrayBuffers",
    "external",
    "heapTotal",
    "heapUsed",
    "rss",
  ]
) {
  assert.strictEqual(typeof processApi.memoryUsage()[name], "number");
}
for (const name of ["user", "system"]) {
  assert.strictEqual(typeof processApi.cpuUsage()[name], "number");
}
for (
  const name of [
    "ipcReceived",
    "ipcSent",
    "sharedMemorySize",
    "signalsCount",
  ]
) {
  assert.strictEqual(typeof processApi.resourceUsage()[name], "number");
}

console.log("process resource metrics passed");
