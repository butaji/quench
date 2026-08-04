"use strict";

const assert = require("assert");

const reporters = require("node:test/reporters");
assert.strictEqual(typeof reporters.spec, "function");
assert.strictEqual(typeof reporters.tap, "function");

console.log("test reporters api passed");
