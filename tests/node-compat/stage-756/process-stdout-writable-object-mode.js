"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(processApi.stdout.writableObjectMode, false);

console.log("process stdout writableObjectMode passed");
