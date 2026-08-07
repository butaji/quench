"use strict";

const assert = require("assert");
const processApi = require("process");

const usage = processApi.memoryUsage();
for (
  const field of [
    "rss",
    "heapTotal",
    "heapUsed",
    "external",
    "arrayBuffers",
  ]
) {
  assert.strictEqual(typeof usage[field], "number");
  assert(Number.isFinite(usage[field]));
  assert(usage[field] >= 0);
}

console.log("process memory usage passed");
