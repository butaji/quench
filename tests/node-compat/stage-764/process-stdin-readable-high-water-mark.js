"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(processApi.stdin.readableHighWaterMark, 65536);

console.log("process stdin readable high water mark passed");
