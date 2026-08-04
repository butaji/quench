"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(processApi.stderr.pending, false);

console.log("process stderr pending passed");
