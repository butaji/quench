"use strict";

const assert = require("assert");
const processApi = require("process");

const usage = processApi.memoryUsage();
for (
  const key of ["rss", "heapTotal", "heapUsed", "external", "arrayBuffers"]
) {
  assert.strictEqual(typeof usage[key], "number");
}

console.log("process memoryUsage shape passed");
