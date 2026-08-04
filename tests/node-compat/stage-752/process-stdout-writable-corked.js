"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(processApi.stdout.writableCorked, 0);

console.log("process stdout writableCorked passed");
