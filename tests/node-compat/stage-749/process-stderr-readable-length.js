"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(processApi.stderr.readableLength, 0);

console.log("process stderr readable length passed");
