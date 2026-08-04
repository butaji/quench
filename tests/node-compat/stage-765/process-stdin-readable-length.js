"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(processApi.stdin.readableLength, 0);

console.log("process stdin readable length passed");
