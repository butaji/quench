"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.stdout.setEncoding, "function");
assert.strictEqual(processApi.stdout.setEncoding("utf8"), processApi.stdout);

console.log("process stdout encoding passed");
