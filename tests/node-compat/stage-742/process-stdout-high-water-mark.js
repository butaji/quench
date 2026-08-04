"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.stdout.writableHighWaterMark, "number");
assert(processApi.stdout.writableHighWaterMark > 0);

console.log("process stdout high water mark passed");
