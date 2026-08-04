"use strict";

const assert = require("assert");
const promises = require("node:stream/promises");

assert.strictEqual(typeof promises.pipeline, "function");
assert.strictEqual(typeof promises.finished, "function");

console.log("stream promises api passed");
