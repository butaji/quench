"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(processApi.stdout.readableHighWaterMark, 65536);

console.log("process stdout readable high water mark passed");
