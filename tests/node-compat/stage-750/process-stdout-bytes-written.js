"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(processApi.stdout.bytesWritten, 0);

console.log("process stdout bytesWritten passed");
