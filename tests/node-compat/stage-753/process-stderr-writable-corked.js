"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(processApi.stderr.writableCorked, 0);

console.log("process stderr writableCorked passed");
