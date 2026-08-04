"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.memoryUsage.rss, "function");
const rss = processApi.memoryUsage.rss();
assert.strictEqual(typeof rss, "number");
assert(Number.isFinite(rss));
assert(rss >= 0);

console.log("process memory rss passed");
