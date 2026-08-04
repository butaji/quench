"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(processApi.stdin.readableFlowing, null);

console.log("process stdin readableFlowing passed");
