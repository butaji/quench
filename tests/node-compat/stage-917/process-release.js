"use strict";

const assert = require("assert");

assert.strictEqual(process.release.name, "node");
assert.ok(process.release && typeof process.release === "object");
assert.strictEqual(typeof process.release.sourceUrl, "string");

console.log("process release passed");
