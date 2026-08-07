"use strict";

const assert = require("assert");
const v8 = require("v8");

const stats = v8.getHeapStatistics();
assert.strictEqual(typeof stats.used_heap_size, "number");
assert.strictEqual(
  typeof v8.getHeapCodeStatistics().code_and_metadata_size,
  "number",
);
assert.strictEqual(v8.takeCoverage(), undefined);
assert.strictEqual(v8.stopCoverage(), undefined);
assert.throws(() => v8.writeHeapSnapshot(), { code: "ERR_V8_NOT_SUPPORTED" });

console.log("v8 surface passed");
