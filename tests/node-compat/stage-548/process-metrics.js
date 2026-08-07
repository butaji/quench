"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.uptime(), "number");
for (
  const key of ["rss", "heapTotal", "heapUsed", "external", "arrayBuffers"]
) {
  assert.strictEqual(typeof processApi.memoryUsage()[key], "number");
}
assert.strictEqual(typeof processApi.cpuUsage().user, "number");
assert.strictEqual(typeof processApi.cpuUsage().system, "number");

console.log("process metrics passed");
