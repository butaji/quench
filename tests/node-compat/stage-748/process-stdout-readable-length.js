"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(processApi.stdout.readableLength, 0);

console.log("process stdout readable length passed");
