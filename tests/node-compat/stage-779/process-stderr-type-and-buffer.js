"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(processApi.stderr.constructor.name, "Socket");
assert.strictEqual(processApi.stderr.writableHighWaterMark, 65536);

console.log("process stderr type and buffer passed");
