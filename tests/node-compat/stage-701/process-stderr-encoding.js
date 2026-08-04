"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.stderr.setEncoding, "function");
assert.strictEqual(processApi.stderr.setEncoding("utf8"), processApi.stderr);

console.log("process stderr encoding passed");
