"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(processApi.stdin.readableObjectMode, false);

console.log("process stdin readableObjectMode passed");
