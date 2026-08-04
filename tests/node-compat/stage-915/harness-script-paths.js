"use strict";

const assert = require("assert");
const path = require("path");

assert.ok(path.isAbsolute(__filename));
assert.ok(path.isAbsolute(__dirname));
assert.strictEqual(path.basename(__filename), "harness-script-paths.js");
assert.ok(__dirname.endsWith("stage-915"));

console.log("harness script paths passed");
