"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(processApi.stdout.constructor.name, "Socket");
assert.strictEqual(processApi.stdout.writableHighWaterMark, 65536);

console.log("process stdout type and buffer passed");
