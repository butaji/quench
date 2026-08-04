"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.memoryUsage.rss, "function");
assert.strictEqual(typeof processApi.memoryUsage.rss(), "number");

console.log("process memoryUsage rss passed");
