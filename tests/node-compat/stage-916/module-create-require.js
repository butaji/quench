"use strict";

const assert = require("assert");
const { createRequire } = require("module");

assert.strictEqual(typeof createRequire, "function");
const requireFromFile = createRequire("/tmp/quench-node-entry.js");
assert.strictEqual(typeof requireFromFile, "function");
assert.strictEqual(requireFromFile("path").basename("a/b"), "b");

console.log("module create require passed");
