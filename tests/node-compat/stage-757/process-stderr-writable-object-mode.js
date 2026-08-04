"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(processApi.stderr.writableObjectMode, false);

console.log("process stderr writableObjectMode passed");
