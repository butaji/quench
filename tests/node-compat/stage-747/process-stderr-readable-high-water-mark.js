"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(processApi.stderr.readableHighWaterMark, 65536);

console.log("process stderr readable high water mark passed");
