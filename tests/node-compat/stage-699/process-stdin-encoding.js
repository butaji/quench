"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.stdin.setEncoding, "function");
assert.strictEqual(processApi.stdin.setEncoding("utf8"), processApi.stdin);

console.log("process stdin encoding passed");
