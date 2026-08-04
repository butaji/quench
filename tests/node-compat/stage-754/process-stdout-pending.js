"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(processApi.stdout.pending, false);

console.log("process stdout pending passed");
