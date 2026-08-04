"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(processApi.stdin.readable, true);
assert.strictEqual(processApi.stdin.readableEnded, false);

console.log("process stdin readable state passed");
