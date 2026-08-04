"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.stdin.isPaused, "function");
assert.strictEqual(processApi.stdin.isPaused(), false);

console.log("process stdin isPaused passed");
