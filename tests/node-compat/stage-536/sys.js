"use strict";

const assert = require("assert");
const sys = require("sys");
const util = require("util");

assert.strictEqual(sys, util);
assert.strictEqual(sys.format("%s:%d", "value", 2), "value:2");
assert.strictEqual(typeof sys.inspect, "function");

console.log("sys passed");
