"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.stderr.writableHighWaterMark, "number");
assert(processApi.stderr.writableHighWaterMark > 0);

console.log("process stderr high water mark passed");
