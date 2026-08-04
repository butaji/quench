"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(processApi.stderr.readableObjectMode, false);

console.log("process stderr readableObjectMode passed");
