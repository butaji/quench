"use strict";

const assert = require("assert");
const inspector = require("node:inspector/promises");

assert.strictEqual(typeof inspector.open, "function");
assert.strictEqual(typeof inspector.close, "function");
assert.strictEqual(typeof inspector.url, "function");
assert.strictEqual(typeof inspector.waitForDebugger, "function");
assert.strictEqual(typeof inspector.Session, "function");

console.log("inspector promises api passed");
