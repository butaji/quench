"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(processApi.stderr.bytesWritten, 0);

console.log("process stderr bytesWritten passed");
