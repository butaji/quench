"use strict";

const assert = require("assert");
const promisesApi = require("node:stream/promises");

assert.strictEqual(typeof promisesApi.pipeline, "function");
assert.strictEqual(typeof promisesApi.finished, "function");

console.log("stream promises api passed");
