"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(processApi.stdout.readableObjectMode, false);

console.log("process stdout readableObjectMode passed");
